use core::panic;
use std::ffi::c_int;
/// Example: .\swdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.swd -o unpack
/// Example: .\swdl_tool.exe from-xml .\unpack\*.swd.xml -o .\NDS_UNPACK\data\SOUND\BGM\

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, command, Subcommand};
use dse::swdl::sf2::{copy_raw_sample_data, copy_presets, DSPOptions, SongBuilderFlags};
use dse::swdl::{SWDL, PRGIChunk, KGRPChunk, Keygroup, create_swdl_shell};
use dse::dtype::DSEError;

#[path = "../fileutils.rs"]
mod fileutils;
use fileutils::{VERSION, valid_file_of_type};
use soundfont::SoundFont2;
use fileutils::{get_final_output_folder, get_input_output_pairs, open_file_overwrite_rw, get_file_last_modified_date_with_default};

use colored::Colorize;

#[derive(Parser)]
#[command(author = "Adakite", version = VERSION, about = "Tools for working with SWDL and SWDL.XML files", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands
}

#[derive(Subcommand)]
enum Commands {
    ToXML {
        /// Sets the path of the SWD files to be translated
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the translated files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    },
    FromXML {
        /// Sets the path of the source SWD.XML files
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the encoded files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    },
    AddSF2 {
        /// Sets the path of the source SF2 files
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the SWDL program files for each of the SF2 files for use with individual tracks
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,

        /// Sets the main bank SWDL file or SWD.XML to use as the base for the patching
        #[arg(value_name = "SWDL_MAIN_BANK_IN")]
        swdl: PathBuf,

        /// Sets the path to output the patched main bank SWDL file
        #[arg(value_name = "SWDL_MAIN_BANK_OUT")]
        out_swdl: Option<PathBuf>,

        /// If a sample has a sample rate above this, it will be resampled to the sample rate specified in `sample_rate`
        #[arg(short = 't', long, default_value_t = 25000)]
        resample_threshold: u32,

        /// Samples with a higher sample rate than the `resample_threshold` will be resampled at this sample rate
        #[arg(short = 'S', long, default_value_t = 22050)]
        sample_rate: u32,

        /// The sample-rate adjustment curve to use.
        /// 1 - Ideal sample correction for fixed 32728.5Hz hardware output rate
        /// 2 - Discrete lookup table based on the original EoS main bank (all samples must either match the `sample_rate` parameter *or* be converted to that sample rate in this mode!)
        /// 3 - Fitted curve
        #[arg(short = 'C', long, default_value_t = 1)]
        sample_rate_adjustment_curve: usize,

        /// The lookahead for the ADPCM encoding process. A higher value allows the encoder to look further into the future to find the optimum coding sequence for the file. Default is 3, but experimentation with higher values is recommended.
        #[arg(short = 'l', long, default_value_t = 3)]
        adpcm_encoder_lookahead: c_int,

        /// Adjusts the pitch of all samples (in cents)
        #[arg(short = 'P', long, default_value_t = 0, allow_hyphen_values = true)]
        pitch_adjust: i64
    }
}

fn main() -> Result<(), DSEError> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::FromXML { input_glob, output_folder } | Commands::ToXML { input_glob, output_folder } => {
            let (source_file_format, change_ext) = match &cli.command {
                Commands::FromXML { input_glob: _, output_folder: _ } => ("xml", ""),
                Commands::ToXML { input_glob: _, output_folder: _ } => ("swd", "swd.xml"),
                _ => panic!("Unreachable"),
            };
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext)?;

            for (input_file_path, output_file_path) in input_file_paths {
                print!("Converting {}... ", input_file_path.display());
                if source_file_format == "swd" {
                    let flags = SongBuilderFlags::parse_from_swdl_file(&mut File::open(input_file_path.clone())?)?;

                    let mut raw = File::open(input_file_path)?;
                    let mut swdl = SWDL::default();
                    if flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                        swdl.read_from_file::<u32, u32, _>(&mut raw)?;
                    } else if flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                        swdl.read_from_file::<u32, u16, _>(&mut raw)?;
                    } else if flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                        swdl.read_from_file::<u16, u32, _>(&mut raw)?;
                    } else {
                        swdl.read_from_file::<u16, u16, _>(&mut raw)?;
                    }

                    let st = quick_xml::se::to_string(&swdl)?;
                    open_file_overwrite_rw(output_file_path)?.write_all(st.as_bytes())?;
                } else if source_file_format == "xml" {
                    let st = std::fs::read_to_string(input_file_path)?;
                    let mut swdl_recreated = quick_xml::de::from_str::<SWDL>(&st)?;

                    let flags = SongBuilderFlags::parse_from_swdl(&swdl_recreated);

                    if flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                        swdl_recreated.regenerate_read_markers::<u32, u32>()?;
                        swdl_recreated.regenerate_automatic_parameters()?;
                        
                        swdl_recreated.write_to_file::<u32, u32, _>(&mut open_file_overwrite_rw(output_file_path)?)?;
                    } else if flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                        swdl_recreated.regenerate_read_markers::<u32, u16>()?;
                        swdl_recreated.regenerate_automatic_parameters()?;

                        swdl_recreated.write_to_file::<u32, u16, _>(&mut open_file_overwrite_rw(output_file_path)?)?;
                    } else if flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                        swdl_recreated.regenerate_read_markers::<u16, u32>()?;
                        swdl_recreated.regenerate_automatic_parameters()?;

                        swdl_recreated.write_to_file::<u16, u32, _>(&mut open_file_overwrite_rw(output_file_path)?)?;
                    } else {
                        swdl_recreated.regenerate_read_markers::<u16, u16>()?;
                        swdl_recreated.regenerate_automatic_parameters()?;

                        swdl_recreated.write_to_file::<u16, u16, _>(&mut open_file_overwrite_rw(output_file_path)?)?;
                    }
                } else {
                    panic!("Whaaat?");
                }
                println!("done!");
            }

            println!("\nAll files successfully processed.");
        }
        Commands::AddSF2 { input_glob, output_folder, swdl: swdl_path, out_swdl: out_swdl_path, resample_threshold, sample_rate, sample_rate_adjustment_curve, adpcm_encoder_lookahead, pitch_adjust } => {
            let (source_file_format, change_ext) = ("sf2", "swd");
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext)?;
            
            let mut main_bank_swdl;
            let main_bank_flags;
            if valid_file_of_type(swdl_path, "swd") {
                main_bank_flags = SongBuilderFlags::parse_from_swdl_file(&mut File::open(swdl_path)?)?;
                
                main_bank_swdl = SWDL::default();
                if main_bank_flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                    main_bank_swdl.read_from_file::<u32, u32, _>(&mut File::open(swdl_path)?)?;
                } else if main_bank_flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                    main_bank_swdl.read_from_file::<u32, u16, _>(&mut File::open(swdl_path)?)?;
                } else if main_bank_flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                    main_bank_swdl.read_from_file::<u16, u32, _>(&mut File::open(swdl_path)?)?;
                } else {
                    main_bank_swdl.read_from_file::<u16, u16, _>(&mut File::open(swdl_path)?)?;
                }
            } else if valid_file_of_type(swdl_path, "xml") {
                let st = std::fs::read_to_string(swdl_path)?;
                main_bank_swdl = quick_xml::de::from_str::<SWDL>(&st)?;
                main_bank_flags = SongBuilderFlags::parse_from_swdl(&main_bank_swdl);
                
                if main_bank_flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                    main_bank_swdl.regenerate_read_markers::<u32, u32>()?;
                } else if main_bank_flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                    main_bank_swdl.regenerate_read_markers::<u32, u16>()?;
                } else if main_bank_flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                    main_bank_swdl.regenerate_read_markers::<u16, u32>()?;
                } else {
                    main_bank_swdl.regenerate_read_markers::<u16, u16>()?;
                }
                main_bank_swdl.regenerate_automatic_parameters()?;
            } else {
                return Err(DSEError::Invalid("Provided Main Bank SWD file is not an SWD file!".to_string()));
            }

            // Start patching in the SF2 files one by one
            for (input_file_path, output_file_path) in input_file_paths {
                print!("Patching in {}... ", input_file_path.display());
                
                let sf2 = SoundFont2::load(&mut File::open(&input_file_path)?).map_err(|x| DSEError::SoundFontParseError(format!("{:?}", x)))?;
                
                let (sample_mappings, mut sample_infos) = copy_raw_sample_data(&File::open(&input_file_path)?, &sf2, &mut main_bank_swdl, DSPOptions { ppmdu_mainbank: false, resample_threshold: *resample_threshold, sample_rate: *sample_rate as f64, sample_rate_relative: false, adpcm_encoder_lookahead: *adpcm_encoder_lookahead }, *sample_rate_adjustment_curve, *pitch_adjust, |_, _| true)?;

                let fname = input_file_path.file_name().ok_or(DSEError::_FileNameReadFailed(input_file_path.display().to_string()))?
                    .to_str().ok_or(DSEError::DSEFileNameConversionNonUTF8("SF2".to_string(), input_file_path.display().to_string()))?
                    .to_string();

                // Create a blank track SWDL file
                let mut track_swdl = create_swdl_shell(get_file_last_modified_date_with_default(&input_file_path)?, fname)?;

                let mut prgi = PRGIChunk::new(0);
                copy_presets(&sf2, &mut sample_infos, &mut prgi.data, |i| Some(sample_mappings.get(&i).copied().ok_or(DSEError::WrapperString(format!("{}Failed to map sample {}!", "Internal Error: ".red(), i))).unwrap()), *sample_rate_adjustment_curve, *pitch_adjust, |_, _, _, _, _, _| true, |_, preset, _| Some(preset.header.bank * 128 + preset.header.preset));
                track_swdl.prgi = Some(prgi);

                // Add the sample info objects last
                track_swdl.wavi.data.objects = sample_infos.into_values().collect();

                // Keygroups
                let mut track_swdl_kgrp = KGRPChunk::default();
                track_swdl_kgrp.data.objects = vec![
                    Keygroup { id: 0, poly: -1, priority: 8, vclow: 0, vchigh: -1, unk50: 0, unk51: 0 },
                    Keygroup { id: 1, poly: 2, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 2, poly: 1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 3, poly: 1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 4, poly: 1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 5, poly: 1, priority: 1, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 6, poly: 2, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 7, poly: 1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 8, poly: 2, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 9, poly: -1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 10, poly: -1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                    Keygroup { id: 11, poly: -1, priority: 8, vclow: 0, vchigh: 15, unk50: 0, unk51: 0 },
                ]; // Just a quick template keygroup list. By default only the first kgrp is used!
                track_swdl.kgrp = Some(track_swdl_kgrp);

                // Write the track swdl file into the specified output directory
                track_swdl.regenerate_read_markers::<u16, u16>()?;
                track_swdl.regenerate_automatic_parameters()?;
                track_swdl.write_to_file::<u16, u16, _>(&mut open_file_overwrite_rw(output_file_path)?)?;

                println!("done!");
            }

            let out_swdl_path = out_swdl_path.clone().unwrap_or(std::env::current_dir()?.join("bgm.patched.swd"));

            if main_bank_flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
                main_bank_swdl.regenerate_read_markers::<u32, u32>()?;
                main_bank_swdl.regenerate_automatic_parameters()?;
                main_bank_swdl.write_to_file::<u32, u32, _>(&mut open_file_overwrite_rw(out_swdl_path)?)?;
            } else if main_bank_flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
                main_bank_swdl.regenerate_read_markers::<u32, u16>()?;
                main_bank_swdl.regenerate_automatic_parameters()?;
                main_bank_swdl.write_to_file::<u32, u16, _>(&mut open_file_overwrite_rw(out_swdl_path)?)?;
            } else if main_bank_flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
                main_bank_swdl.regenerate_read_markers::<u16, u32>()?;
                main_bank_swdl.regenerate_automatic_parameters()?;
                main_bank_swdl.write_to_file::<u16, u32, _>(&mut open_file_overwrite_rw(out_swdl_path)?)?;
            } else {
                main_bank_swdl.regenerate_read_markers::<u16, u16>()?;
                main_bank_swdl.regenerate_automatic_parameters()?;
                main_bank_swdl.write_to_file::<u16, u16, _>(&mut open_file_overwrite_rw(out_swdl_path)?)?;
            }
        },
    }

    Ok(())
}

