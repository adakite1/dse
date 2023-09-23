use std::collections::HashMap;
/// Example: .\smdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.smd -o unpack
/// Example: .\smdl_tool.exe from-xml .\unpack\*.smd.xml -o .\NDS_UNPACK\data\SOUND\BGM\

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, command, Subcommand};
use colored::Colorize;
use dse::smdl::midi::{open_midi, get_midi_tpb, get_midi_messages_flattened, TrkChunkWriter, copy_midi_messages};
use dse::smdl::create_smdl_shell;
use dse::swdl::ProgramInfo;
use dse::swdl::sf2::SongBuilderFlags;
use dse::{smdl::SMDL, swdl::SWDL};
use dse::dtype::{ReadWrite, DSEError, DSELinkBytes};

#[path = "../fileutils.rs"]
mod fileutils;
use fileutils::VERSION;
use fileutils::{get_final_output_folder, get_input_output_pairs, open_file_overwrite_rw, valid_file_of_type, get_file_last_modified_date_with_default};

#[derive(Parser)]
#[command(author = "Adakite", version = VERSION, about = "Tools for working with SMDL and SMDL.XML files", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands
}

#[derive(Subcommand)]
enum Commands {
    ToXML {
        /// Sets the path of the SMD files to be translated
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the translated files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    },
    FromXML {
        /// Sets the path of the source SMD.XML files
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the encoded files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    },
    FromMIDI {
        /// Sets the path of the source MIDI files
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the first link byte for linking to the correct SWDL bank. Will take precedence over the option `swdl`, which also sets the link bytes.
        #[arg(short = '1', long, value_name = "LINK_BYTE_1")]
        unk1: Option<u8>,

        /// Sets the second link byte for linking to the correct SWDL bank. Will take precedence over the option `swdl`, which also sets the link bytes.
        #[arg(short = '2', long, value_name = "LINK_BYTE_2")]
        unk2: Option<u8>,

        /// Sets the SWDL file or SWD.XML to pair the MIDI files with
        #[arg(short = 'b', long, value_name = "SWDL")]
        swdl: Option<PathBuf>,

        /// Sets the folder to output the encoded files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,

        /// Map Program Change and CC0 Bank Select events to DSE SWDL program id's. Without this option, by default all tracks are mapped to the 0th preset.
        #[arg(short = 'M', long, action)]
        midi_prgch: bool,

        // If `generate_optimized_swdl` is set, new swdl files specifically made for the inputted MIDI files will be generated. This is to handle larger bank files so that only the instruments needed for the MIDI file will be loaded.
        #[arg(long, action)]
        generate_optimized_swdl: bool
    }
}

fn main() -> Result<(), DSEError> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::FromXML { input_glob, output_folder } | Commands::ToXML { input_glob, output_folder } => {
            let (source_file_format, change_ext) = match &cli.command {
                Commands::FromXML { input_glob: _, output_folder: _ } => ("xml", ""),
                Commands::ToXML { input_glob: _, output_folder: _ } => ("smd", "smd.xml"),
                _ => panic!("Unreachable")
            };
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext)?;

            for (input_file_path, output_file_path) in input_file_paths {
                print!("Converting {}... ", input_file_path.display());
                if source_file_format == "smd" {
                    let mut raw = File::open(input_file_path)?;
                    let mut smdl = SMDL::default();
                    smdl.read_from_file(&mut raw)?;

                    let st = quick_xml::se::to_string(&smdl)?;
                    open_file_overwrite_rw(output_file_path)?.write_all(st.as_bytes())?;
                } else if source_file_format == "xml" {
                    let st = std::fs::read_to_string(input_file_path)?;
                    let mut smdl_recreated = quick_xml::de::from_str::<SMDL>(&st)?;
                    smdl_recreated.regenerate_read_markers()?;

                    smdl_recreated.write_to_file(&mut open_file_overwrite_rw(output_file_path)?)?;
                } else {
                    panic!("Whaaat?");
                }
                println!("done!");
            }

            println!("\nAll files successfully processed.");
        },
        Commands::FromMIDI { input_glob, unk1, unk2, swdl: swdl_path, output_folder, midi_prgch, generate_optimized_swdl } => {
            let (source_file_format, change_ext) = ("mid", "smd");
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext)?;
            let input_file_paths_2: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, "swd")?;

            let mut swdl = None;
            if let Some(swdl_path) = swdl_path {
                if valid_file_of_type(swdl_path, "swd") {
                    let flags = SongBuilderFlags::parse_from_swdl_file(&mut File::open(swdl_path.clone())?)?;

                    swdl = Some(SWDL::default());
                    if flags.intersects(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().read_from_file::<u32, u32, _>(&mut File::open(swdl_path)?)?;
                    } else if flags.intersects(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().read_from_file::<u32, u16, _>(&mut File::open(swdl_path)?)?;
                    } else if flags.intersects(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().read_from_file::<u16, u32, _>(&mut File::open(swdl_path)?)?;
                    } else {
                        swdl.as_mut().unwrap().read_from_file::<u16, u16, _>(&mut File::open(swdl_path)?)?;
                    }
                } else if valid_file_of_type(swdl_path, "xml") {
                    let st = std::fs::read_to_string(swdl_path)?;
                    swdl = Some(quick_xml::de::from_str::<SWDL>(&st)?);
                    let flags = SongBuilderFlags::parse_from_swdl(swdl.as_ref().unwrap());

                    if flags.intersects(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().regenerate_read_markers::<u32, u32>()?;
                    } else if flags.intersects(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().regenerate_read_markers::<u32, u16>()?;
                    } else if flags.intersects(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                        swdl.as_mut().unwrap().regenerate_read_markers::<u16, u32>()?;
                    } else {
                        swdl.as_mut().unwrap().regenerate_read_markers::<u16, u16>()?;
                    }

                    swdl.as_mut().unwrap().regenerate_automatic_parameters()?;
                } else {
                    return Err(DSEError::Invalid("Provided SWD file is not an SWD file!".to_string()));
                }
            }

            for ((input_file_path, output_file_path), (_, output_file_path_swd)) in input_file_paths.into_iter().zip(input_file_paths_2) {
                print!("Converting {}... ", input_file_path.display());

                let smf_source = std::fs::read(&input_file_path)?;
                let smf = open_midi(&smf_source)?;
                let tpb = get_midi_tpb(&smf)?;

                let fname = input_file_path.file_name().ok_or(DSEError::_FileNameReadFailed(input_file_path.display().to_string()))?
                    .to_str().ok_or(DSEError::DSEFileNameConversionNonUTF8("MIDI".to_string(), input_file_path.display().to_string()))?
                    .to_string();

                let mut smdl = create_smdl_shell(get_file_last_modified_date_with_default(&input_file_path)?, fname)?;

                // Fill in header and song information
                if let Some(swdl) = &swdl {
                    smdl.set_link_bytes(swdl.get_link_bytes());
                }
                if let Some(unk1) = unk1 {
                    smdl.set_unk1(*unk1);
                }
                if let Some(unk2) = unk2 {
                    smdl.set_unk2(*unk2);
                }
                smdl.song.tpqn = tpb;

                let midi_messages = get_midi_messages_flattened(&smf)?;

                let mut prgi_objects = None;
                if let Some(swdl) = &swdl {
                    prgi_objects = Some(&swdl.prgi.as_ref().ok_or(DSEError::DSESmdConverterSwdEmpty())?.data.objects);
                }

                // Vec of TrkChunkWriter's
                let mut trks: [TrkChunkWriter; 17] = std::array::from_fn(|i| TrkChunkWriter::create(i as u8, i as u8, smdl.get_link_bytes(), None).unwrap());
                // Copy midi messages
                let _ = copy_midi_messages(midi_messages, &mut trks, |bank, program, _| {
                    if *midi_prgch {
                        Some(bank * 128 + program)
                    } else {
                        None
                    }
                })?;
                
                // Get a list of swdl presets in the file provided
                let mut prgi_ids_prune_list: Option<Vec<u16>> = prgi_objects.map(|prgi_objects| prgi_objects.iter().map(|x| x.header.id).collect());

                // Fill the tracks into the smdl
                smdl.trks.objects = trks.into_iter().map(|x| {
                    for id in x.programs_used() {
                        if let Some(prgi_ids_prune_list) = prgi_ids_prune_list.as_mut() {
                            if let Some(idx) = prgi_ids_prune_list.iter().position(|&r| r == id.to_dse() as u16) {
                                prgi_ids_prune_list.remove(idx);
                            }
                        }
                    }
                    x.close_track()
                }).collect();

                if *generate_optimized_swdl {
                    if let Some(prgi_ids_prune_list) = prgi_ids_prune_list {
                        // Since the check for `prgi_ids_prune_list` has cleared, we can assume these will all exist.
                        let swdl = swdl.as_ref().unwrap();
                        let _ = prgi_objects.as_ref().unwrap(); // Added for completion and documentation

                        // Remove unnecessary presets and samples
                        let mut track_swdl = swdl.clone();
                        let prgi_objects = &mut track_swdl.prgi.as_mut().ok_or(DSEError::DSESmdConverterSwdEmpty())?.data.objects;
                        for unneeded_prgi in prgi_ids_prune_list {
                            if let Some(idx) = prgi_objects.iter().position(|prgm_info: &ProgramInfo| prgm_info.header.id == unneeded_prgi) {
                                prgi_objects.remove(idx);
                            }
                        }
                        let mut votes: HashMap<u16, usize> = HashMap::new();
                        let wavi_objects = &mut track_swdl.wavi.data.objects;
                        for prgi in prgi_objects {
                            for split in &prgi.splits_table.objects {
                                votes.insert(split.SmplID, 1); // Note that this will overwrite previous votes, but it shouldn't matter since as long as a single remaining preset depends on the sample, it should be kept.
                            }
                        }
                        wavi_objects.retain(|obj| votes.contains_key(&obj.id));
                        println!("\n{}", "Generating optimized SWDL file...".green());

                        let flags = SongBuilderFlags::parse_from_swdl(&track_swdl);

                        if flags.intersects(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                            track_swdl.regenerate_read_markers::<u32, u32>()?;
                            track_swdl.regenerate_automatic_parameters()?;
                            track_swdl.write_to_file::<u32, u32, _>(&mut open_file_overwrite_rw(output_file_path_swd)?)?;
                        } else if flags.intersects(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                            track_swdl.regenerate_read_markers::<u32, u16>()?;
                            track_swdl.regenerate_automatic_parameters()?;
                            track_swdl.write_to_file::<u32, u16, _>(&mut open_file_overwrite_rw(output_file_path_swd)?)?;
                        } else if flags.intersects(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                            track_swdl.regenerate_read_markers::<u16, u32>()?;
                            track_swdl.regenerate_automatic_parameters()?;
                            track_swdl.write_to_file::<u16, u32, _>(&mut open_file_overwrite_rw(output_file_path_swd)?)?;
                        } else {
                            track_swdl.regenerate_read_markers::<u16, u16>()?;
                            track_swdl.regenerate_automatic_parameters()?;
                            track_swdl.write_to_file::<u16, u16, _>(&mut open_file_overwrite_rw(output_file_path_swd)?)?;
                        }
                    } else {
                        println!("{}Failed to generate an optimized track-specific SWDL bank! Source SWDL bank unspecified!! Skipped.", "Error: ".red());
                    }
                }

                // Regenerate read markers for the SMDL
                smdl.regenerate_read_markers()?;

                // Write to file
                smdl.write_to_file(&mut open_file_overwrite_rw(output_file_path)?)?;
                
                println!("done!");
            }

            println!("\nAll files successfully processed.");
        }
    }

    Ok(())
}

