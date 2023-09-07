use core::panic;
use std::ffi::c_int;
/// Example: .\swdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.swd -o unpack
/// Example: .\swdl_tool.exe from-xml .\unpack\*.swd.xml -o .\NDS_UNPACK\data\SOUND\BGM\

use std::fs::File;
use std::io::{Write, Seek, Cursor, Read};
use std::path::PathBuf;

use byteorder::{WriteBytesExt, ReadBytesExt, LittleEndian};
use clap::{Parser, command, Subcommand};
use colored::Colorize;
use dse::swdl::{SWDL, SampleInfo, ADSRVolumeEnvelope, DSEString, ProgramInfo, SplitEntry, LFOEntry, PRGIChunk, KGRPChunk, Keygroup, PCMDChunk, WAVIChunk};
use dse::dtype::{ReadWrite, PointerTable, DSEError};

#[path = "../binutils.rs"]
mod binutils;
use binutils::{VERSION, valid_file_of_type};
use dse_dsp_sys::process_mono;
use phf::phf_map;
use soundfont::data::SampleHeader;
use soundfont::{SoundFont2, Zone};
use crate::binutils::{get_final_output_folder, get_input_output_pairs, open_file_overwrite_rw, get_file_last_modified_date_with_default};

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
        /// 4 - Ideal sample correction minus 100 cents (1 semitone)
        #[arg(short = 'C', long, default_value_t = 4)]
        sample_rate_adjustment_curve: usize,

        /// The lookahead for the ADPCM encoding process. A higher value allows the encoder to look further into the future to find the optimum coding sequence for the file. Default is 3, but experimentation with higher values is recommended.
        #[arg(short = 'l', long, default_value_t = 3)]
        adpcm_encoder_lookahead: c_int,
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
                    let mut raw = File::open(input_file_path)?;
                    let mut swdl = SWDL::default();
                    swdl.read_from_file(&mut raw)?;

                    let st = quick_xml::se::to_string(&swdl)?;
                    open_file_overwrite_rw(output_file_path)?.write_all(st.as_bytes())?;
                } else if source_file_format == "xml" {
                    let st = std::fs::read_to_string(input_file_path)?;
                    let mut swdl_recreated = quick_xml::de::from_str::<SWDL>(&st)?;
                    swdl_recreated.regenerate_read_markers()?;
                    swdl_recreated.regenerate_automatic_parameters()?;

                    swdl_recreated.write_to_file(&mut open_file_overwrite_rw(output_file_path)?)?;
                } else {
                    panic!("Whaaat?");
                }
                println!("done!");
            }

            println!("\nAll files successfully processed.");
        }
        Commands::AddSF2 { input_glob, output_folder, swdl: swdl_path, out_swdl: out_swdl_path, resample_threshold, sample_rate, sample_rate_adjustment_curve, adpcm_encoder_lookahead } => {
            let (source_file_format, change_ext) = ("sf2", "swd");
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext)?;
            
            let mut main_bank_swdl;
            if valid_file_of_type(swdl_path, "swd") {
                main_bank_swdl = SWDL::default();
                main_bank_swdl.read_from_file(&mut File::open(swdl_path)?)?;
            } else if valid_file_of_type(swdl_path, "xml") {
                let st = std::fs::read_to_string(swdl_path)?;
                main_bank_swdl = quick_xml::de::from_str::<SWDL>(&st)?;
                main_bank_swdl.regenerate_read_markers()?;
                main_bank_swdl.regenerate_automatic_parameters()?;
            } else {
                return Err(DSEError::Invalid("Provided Main Bank SWD file is not an SWD file!".to_string()));
            }

            // Start patching in the SF2 files one by one
            for (input_file_path, output_file_path) in input_file_paths {
                print!("Patching in {}... ", input_file_path.display());
                
                let sf2 = SoundFont2::load(&mut File::open(&input_file_path)?).map_err(|x| DSEError::SoundFontParseError(format!("{:?}", x)))?;
                
                struct DSPOptions {
                    resample_threshold: u32,
                    sample_rate: f64,
                    adpcm_encoder_lookahead: i32
                }
                fn copy_raw_sample_data<R>(mut sf2file: R, sf2: &SoundFont2, bank: &mut SWDL, dsp_options: DSPOptions, sample_rate_adjustment_curve: usize, filter_samples: fn(&(usize, &SampleHeader)) -> bool) -> Result<(usize, Vec<SampleInfo>), DSEError>
                where
                    R: Read + Seek {
                    let main_bank_swdl_pcmd = bank.pcmd.get_or_insert(PCMDChunk::default());
                    let main_bank_swdl_wavi = &mut bank.wavi;

                    let first_sample_pos = main_bank_swdl_wavi.data.objects.iter().map(|x| x.smplpos + (x.loopbeg + x.looplen) * 4).max().unwrap_or(0);

                    // Create the SampleInfo entries for all the samples
                    let mut sample_infos = Vec::with_capacity(sf2.sample_headers.len());
                    let first_available_id = main_bank_swdl_wavi.data.slots();
                    let mut pos_in_memory = 0;

                    for (i, sample_header) in sf2.sample_headers.iter().enumerate().filter(filter_samples).enumerate().map(|(i, (_, sample_header))| (i, sample_header)) {
                        // Create blank sampleinfo object
                        let mut sample_info = SampleInfo::default();
    
                        // ID
                        sample_info.id = (first_available_id + i) as u16;
    
                        sample_info.smplrate = sample_header.sample_rate;
                        if sample_header.origpitch >= 128 { // origpitch - 255 is reserved for percussion by convention, 128-254 is invalid, but either way the SF2 standard recommends defaulting to 60 when 128-255 is encountered.
                            sample_info.rootkey = 60;
                        } else {
                            sample_info.rootkey = sample_header.origpitch as i8;
                        }
                        sample_info.volume = 127; // SF2 does not have a volume parameter per sample
                        sample_info.pan = 64; // SF2 does not have a pan parameter per sample, and any panning work related to stereo samples are relegated to the Instruments layer anyways
                        sample_info.smplfmt = 0x0200; // SF2 supports 16-bit PCM and 24-bit PCM, and while DSE also supports 16-bit PCM, the problem comes with file size. 16-bit PCM is **massive**, and so it's very hard to fit many samples into the limited memory of the NDS, which could explain the abundant use of 4-bit ADPCM in the original game songs. With that in mind, here we will internally encode the sample data as ADPCM, and on top of that, lower the sample rate if necessary to compress the sample data as much as we possibly can.
                        sample_info.smplloop = sample_header.loop_start != sample_header.loop_end; // SF2 does not seem to have a direct parameter for not looping. This seems to be it.
                        // smplrate is up above with ctune and ftune
                        // smplpos is at the bottom
                        sample_info.loopbeg = (sample_header.loop_start - sample_header.start) / 2;
                        if sample_info.smplloop {
                            sample_info.looplen = (sample_header.loop_end - sample_header.loop_start) / 2;
                        } else {
                            // Not looping, so loop_end - loop_start is zero. Use end - loop_start instead
                            sample_info.looplen = (sample_header.end - sample_header.loop_start) / 2;
                        }
                        // Write sample into main bank
                        if let Some(chunk) = sf2.sample_data.smpl.as_ref() {
                            let sample_pos_bytes = chunk.offset() + 8 + sample_header.start as u64 * 2;
                            let mut raw_sample_data = vec![0_i16; (sample_info.loopbeg + sample_info.looplen) as usize * 2];

                            sf2file.seek(std::io::SeekFrom::Start(sample_pos_bytes)).map_err(|_| DSEError::SampleFindError(sample_header.name.clone(), sample_pos_bytes))?;
                            sf2file.read_i16_into::<LittleEndian>(&mut raw_sample_data).map_err(|_| DSEError::SampleReadError(sample_header.name.clone(), sample_pos_bytes, raw_sample_data.len()));
    
                            // Resample and encode to ADPCM
                            let new_sample_rate = if sample_header.sample_rate > dsp_options.resample_threshold {
                                dsp_options.sample_rate
                            } else {
                                sample_header.sample_rate as f64
                            };
                            let raw_sample_data_len = raw_sample_data.len();
                            let (raw_sample_data, mut new_loop_start) = process_mono(raw_sample_data.into(), sample_header.sample_rate as f64, new_sample_rate, dsp_options.adpcm_encoder_lookahead, ((raw_sample_data_len - 2) | 7) + 2, (sample_header.loop_start - sample_header.start) as usize);
                            if new_loop_start == 4 {
                                new_loop_start = 0;
                            }
                            sample_info.smplrate = new_sample_rate as u32; // Set new sample rate
                            let (ctune, ftune) = sample_rate_adjustment(new_sample_rate, sample_rate_adjustment_curve)?;
                            sample_info.ftune = ftune + sample_header.pitchadj;
                            sample_info.ctune = ctune;
                            sample_info.loopbeg = new_loop_start as u32 / 4; // Set new loopbeg
                            sample_info.looplen = (raw_sample_data.len() - new_loop_start) as u32 / 4; // Set new looplen
    
                            // Sample length is defined by `loopbeg` and `looplen`, which are both indices based around 32bits. To avoid overlapping samples, calculate how much padding is needed to align the samples to 4 bytes here
                            let alignment_padding_len = ((raw_sample_data.len() - 1) | 3) + 1 - raw_sample_data.len();
    
                            let mut cursor = Cursor::new(&mut main_bank_swdl_pcmd.data);
                            cursor.seek(std::io::SeekFrom::End(0)).map_err(|_| DSEError::_InMemorySeekFailed())?;
                            for sample in raw_sample_data {
                                cursor.write_u8(sample).map_err(|_| DSEError::_InMemoryWriteFailed())?;
                            }
    
                            // Write in the padding
                            for _ in 0..alignment_padding_len {
                                cursor.write_u8(0x00).map_err(|_| DSEError::_InMemoryWriteFailed())?; //Todo: might be better to use some other method of padding to avoid artifacts
                            }
                        } else {
                            println!("{}SF2 file does not contain any sample data!", "Warning: ".yellow());
                        }
                        sample_info.volume_envelope = ADSRVolumeEnvelope::default2();
    
                        let mut sample_info_track_swdl = sample_info.clone();
                        sample_info_track_swdl.smplpos = pos_in_memory;
    
                        sample_info.smplpos = pos_in_memory + first_sample_pos as u32;
    
                        // Update pos_in_memory with this sample (should probably also align all the added samples to 4 bytes then)
                        pos_in_memory += (sample_info.loopbeg + sample_info.looplen) * 4;
    
                        // Add the sampleinfo with the relative positions into the vec
                        sample_infos.push(sample_info_track_swdl);
                        // Add the other sampleinfo object into the main bank's swdl
                        main_bank_swdl_wavi.data.objects.push(sample_info);
                    }

                    Ok((first_available_id, sample_infos))
                }

                fn copy_presets(sf2: &SoundFont2, sample_infos: &Vec<SampleInfo>, first_available_id: usize, sample_rate_adjustment_curve: usize) -> PointerTable<ProgramInfo> {
                    // Loop through the presets and use it to fill in the track swdl object
                    let mut prgi_pointer_table = PointerTable::new(sf2.presets.len(), 0);
                    for preset in &sf2.presets {
                        // Create blank programinfo object
                        let mut program_info = ProgramInfo::default();

                        // ID
                        program_info.header.id = preset.header.bank * 128 + preset.header.preset;
                        program_info.header.prgvol = 127;
                        program_info.header.prgpan = 64;
                        program_info.header.PadByte = 170;

                        // Create the 4 LFOs (each preset in SF2 can have many instruments, with each instruments containing multiple samples, and each of those samples can have their own LFOs. 4 is just not enough to map all that, and so this is left to its default state. For now, please add LFOs manually to taste :)
                        let lfos: Vec<LFOEntry> = (0..4).map(|_| LFOEntry::default()).collect();
                        program_info.lfo_table.objects = lfos;

                        /// Function to apply data from a zone to a split
                        /// 
                        /// Returns `true` if the zone provided is a global zone
                        fn apply_zone_data_to_split(split_entry: &mut SplitEntry, zone: &Zone, is_first_zone: bool, other_zones: &[&Zone], sample_infos: &Vec<SampleInfo>, first_available_id: usize, sample_rate_adjustment_curve: usize) -> bool {
                            fn gain(decibels: f64) -> f64 {
                                10.0_f64.powf(decibels / 20.0)
                            }
                            fn decibels(gain: f64) -> f64 {
                                20.0 * gain.log10()
                            }
                            fn timecents_to_milliseconds(timecents: i16) -> i32 {
                                (1000.0_f64 * 2.0_f64.powf(timecents as f64 / 1200.0_f64)).round() as i32
                            }
                            fn timecents_to_index(timecents: i16) -> (u8, i8) {
                                let msec = timecents_to_milliseconds(timecents);
                                if msec <= 0x7FFF {
                                    (1_u8, lookup_env_time_value_i16(msec as i16))
                                } else {
                                    (0_u8, lookup_env_time_value_i32(msec))
                                }
                            }
                            fn lookup_env_time_value_i16(msec: i16) -> i8 {
                                match LOOKUP_TABLE_20_B0_F50.binary_search(&msec) {
                                    Ok(index) => index as i8,
                                    Err(index) => {
                                        if index == 0 { index as i8 }
                                        else if index == LOOKUP_TABLE_20_B0_F50.len() { 127 }
                                        else {
                                            if (LOOKUP_TABLE_20_B0_F50[index] - msec) > (msec - LOOKUP_TABLE_20_B0_F50[index-1]) {
                                                (index - 1) as i8
                                            } else {
                                                index as i8
                                            }
                                        }
                                    }
                                }
                            }
                            fn lookup_env_time_value_i32(msec: i32) -> i8 {
                                match LOOKUP_TABLE_20_B1050.binary_search(&msec) {
                                    Ok(index) => index as i8,
                                    Err(index) => {
                                        if index == 0 { index as i8 }
                                        else if index == LOOKUP_TABLE_20_B1050.len() { 127 }
                                        else {
                                            if (LOOKUP_TABLE_20_B1050[index] - msec) > (msec - LOOKUP_TABLE_20_B1050[index-1]) {
                                                (index - 1) as i8
                                            } else {
                                                index as i8
                                            }
                                        }
                                    }
                                }
                            }
                            // https://stackoverflow.com/questions/67016985/map-numeric-range-rust
                            fn map_range(from_range: (f64, f64), to_range: (f64, f64), s: f64) -> f64 {
                                to_range.0 + (s - from_range.0) * (to_range.1 - to_range.0) / (from_range.1 - from_range.0)
                            }
                            
                            let mut possibly_a_global_zone = true;
                            // Loop through all the generators in this zone
                            let (mut attack, mut hold, mut decay, mut release) = (
                                other_zones.iter().map(|x| x.gen_list.iter()).flatten()
                                    .find(|g| g.ty == soundfont::data::GeneratorType::AttackVolEnv)
                                    .map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0),
                                other_zones.iter().map(|x| x.gen_list.iter()).flatten()
                                    .find(|g| g.ty == soundfont::data::GeneratorType::HoldVolEnv)
                                    .map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0),
                                other_zones.iter().map(|x| x.gen_list.iter()).flatten()
                                    .find(|g| g.ty == soundfont::data::GeneratorType::DecayVolEnv)
                                    .map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0),
                                other_zones.iter().map(|x| x.gen_list.iter()).flatten()
                                    .find(|g| g.ty == soundfont::data::GeneratorType::ReleaseVolEnv)
                                    .map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0),
                            );
                            let mut ftune_overflowed = false;
                            for gen in zone.gen_list.iter() {
                                match gen.ty {
                                    soundfont::data::GeneratorType::StartAddrsOffset => {  },
                                    soundfont::data::GeneratorType::EndAddrsOffset => {  },
                                    soundfont::data::GeneratorType::StartloopAddrsOffset => {  },
                                    soundfont::data::GeneratorType::EndloopAddrsOffset => {  },
                                    soundfont::data::GeneratorType::StartAddrsCoarseOffset => {  },
                                    soundfont::data::GeneratorType::ModLfoToPitch => {  },
                                    soundfont::data::GeneratorType::VibLfoToPitch => {  },
                                    soundfont::data::GeneratorType::ModEnvToPitch => {  },
                                    soundfont::data::GeneratorType::InitialFilterFc => {  },
                                    soundfont::data::GeneratorType::InitialFilterQ => {  },
                                    soundfont::data::GeneratorType::ModLfoToFilterFc => {  },
                                    soundfont::data::GeneratorType::ModEnvToFilterFc => {  },
                                    soundfont::data::GeneratorType::EndAddrsCoarseOffset => {  },
                                    soundfont::data::GeneratorType::ModLfoToVolume => {  },
                                    soundfont::data::GeneratorType::Unused1 => {  },
                                    soundfont::data::GeneratorType::ChorusEffectsSend => {  },
                                    soundfont::data::GeneratorType::ReverbEffectsSend => {  },
                                    soundfont::data::GeneratorType::Pan => {
                                        split_entry.smplpan = map_range((-500.0, 500.0), (0.0, 127.0), *gen.amount.as_i16().unwrap() as f64).round() as i8;
                                    },
                                    soundfont::data::GeneratorType::Unused2 => {  },
                                    soundfont::data::GeneratorType::Unused3 => {  },
                                    soundfont::data::GeneratorType::Unused4 => {  },
                                    soundfont::data::GeneratorType::DelayModLFO => {  },
                                    soundfont::data::GeneratorType::FreqModLFO => {  },
                                    soundfont::data::GeneratorType::DelayVibLFO => {  },
                                    soundfont::data::GeneratorType::FreqVibLFO => {  },
                                    soundfont::data::GeneratorType::DelayModEnv => {  },
                                    soundfont::data::GeneratorType::AttackModEnv => {  },
                                    soundfont::data::GeneratorType::HoldModEnv => {  },
                                    soundfont::data::GeneratorType::DecayModEnv => {  },
                                    soundfont::data::GeneratorType::SustainModEnv => {  },
                                    soundfont::data::GeneratorType::ReleaseModEnv => {  },
                                    soundfont::data::GeneratorType::KeynumToModEnvHold => {  },
                                    soundfont::data::GeneratorType::KeynumToModEnvDecay => {  },
                                    soundfont::data::GeneratorType::DelayVolEnv => {  },
                                    soundfont::data::GeneratorType::AttackVolEnv => {
                                        attack = *gen.amount.as_i16().unwrap();
                                    },
                                    soundfont::data::GeneratorType::HoldVolEnv => {
                                        hold = *gen.amount.as_i16().unwrap();
                                    },
                                    soundfont::data::GeneratorType::DecayVolEnv => {
                                        decay = *gen.amount.as_i16().unwrap();
                                    },
                                    soundfont::data::GeneratorType::SustainVolEnv => {
                                        let decibels = -gen.amount.as_i16().unwrap() as f64 / 10.0_f64;
                                        split_entry.volume_envelope.sustain = (gain(decibels) * 127.0).round() as i8;
                                    },
                                    soundfont::data::GeneratorType::ReleaseVolEnv => {
                                        release = *gen.amount.as_i16().unwrap();
                                    },
                                    soundfont::data::GeneratorType::KeynumToVolEnvHold => {  },
                                    soundfont::data::GeneratorType::KeynumToVolEnvDecay => {  },
                                    soundfont::data::GeneratorType::Instrument => {
                                        possibly_a_global_zone = false;
                                    },
                                    soundfont::data::GeneratorType::Reserved1 => {  },
                                    soundfont::data::GeneratorType::KeyRange => {
                                        let key_range_value = gen.amount.as_range().unwrap();
                                        // let lowkey = ((key_range_value >> 8) & 0x00FF) as i8;
                                        // let hikey = (key_range_value & 0x00FF) as i8;
                                        split_entry.lowkey = key_range_value.low as i8;
                                        split_entry.hikey = key_range_value.high as i8;
                                    },
                                    soundfont::data::GeneratorType::VelRange => {
                                        let vel_range_value = gen.amount.as_range().unwrap();
                                        split_entry.lovel = vel_range_value.low as i8;
                                        split_entry.hivel = vel_range_value.high as i8;
                                    },
                                    soundfont::data::GeneratorType::StartloopAddrsCoarseOffset => {  },
                                    soundfont::data::GeneratorType::Keynum => {  },
                                    soundfont::data::GeneratorType::Velocity => {  },
                                    soundfont::data::GeneratorType::InitialAttenuation => {
                                        let decibels = -gen.amount.as_i16().unwrap() as f64 / 10.0_f64;
                                        split_entry.volume_envelope.atkvol = (gain(decibels) * 127.0).round() as i8;
                                    },
                                    soundfont::data::GeneratorType::Reserved2 => {  },
                                    soundfont::data::GeneratorType::EndloopAddrsCoarseOffset => {  },
                                    soundfont::data::GeneratorType::CoarseTune => {
                                        if !ftune_overflowed {
                                            let smpl;
                                            if let Some(&sample_i) = zone.sample() {
                                                smpl = &sample_infos[sample_i as usize];
                                            } else if let Some(&sample_i) = other_zones.iter().map(|x| x.sample()).find(Option::is_some).flatten() {
                                                smpl = &sample_infos[sample_i as usize];
                                            } else {
                                                println!("{}Some instrument zones contain no samples! Could not calculate necessary ctune to adjust for sample rate. Skipping...", "Warning: ".yellow());
                                                continue;
                                            }
                                            let (ctune, _) = sample_rate_adjustment(smpl.smplrate as f64, sample_rate_adjustment_curve).unwrap();
                                            split_entry.ctune = *gen.amount.as_i16().unwrap() as i8 + ctune;
                                        }
                                    },
                                    soundfont::data::GeneratorType::FineTune => {
                                        let smpl;
                                        if let Some(&sample_i) = zone.sample() {
                                            smpl = &sample_infos[sample_i as usize];
                                        } else if let Some(&sample_i) = other_zones.iter().map(|x| x.sample()).find(Option::is_some).flatten() {
                                            smpl = &sample_infos[sample_i as usize];
                                        } else {
                                            println!("{}Some instrument zones contain no samples! Could not calculate necessary ftune to adjust for sample rate. Skipping...", "Warning: ".yellow());
                                            continue;
                                        }
                                        let (ctune, ftune) = sample_rate_adjustment(smpl.smplrate as f64, sample_rate_adjustment_curve).unwrap();
                                        let tmp = *gen.amount.as_i16().unwrap() as i64 + ftune as i64;
                                        let (ctune_delta, ftune) = cents_to_ctune_ftune(tmp);
                                        if ctune_delta != 0 {
                                            // Overflow!
                                            ftune_overflowed = true;
                                            split_entry.ctune = zone.gen_list
                                                .iter()
                                                .find(|g| g.ty == soundfont::data::GeneratorType::CoarseTune)
                                                .map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) as i8 + ctune + ctune_delta;
                                        }
                                        split_entry.ftune = ftune;
                                    },
                                    soundfont::data::GeneratorType::SampleID => {
                                        possibly_a_global_zone = false;
                                        // Check if the zone specifies which sample we have to use!
                                        split_entry.SmplID = first_available_id as u16 + gen.amount.as_u16().unwrap();
                                    },
                                    soundfont::data::GeneratorType::SampleModes => {  },
                                    soundfont::data::GeneratorType::Reserved3 => {  },
                                    soundfont::data::GeneratorType::ScaleTuning => {  },
                                    soundfont::data::GeneratorType::ExclusiveClass => {  },
                                    soundfont::data::GeneratorType::OverridingRootKey => {
                                        let val = *gen.amount.as_i16().unwrap();
                                        if val != -1 {
                                            split_entry.rootkey = val as i8;
                                        }
                                    },
                                    soundfont::data::GeneratorType::Unused5 => {  },
                                    soundfont::data::GeneratorType::EndOper => {  },
                                }
                            }
                            let (envmult, _) = timecents_to_index(*[attack, hold, decay, release].iter().max().unwrap());
                            split_entry.volume_envelope.envmult = envmult;
                            if envmult == 0 { // Use i32 lookup
                                split_entry.volume_envelope.attack = lookup_env_time_value_i32(timecents_to_milliseconds(attack));
                                split_entry.volume_envelope.hold = lookup_env_time_value_i32(timecents_to_milliseconds(hold));
                                split_entry.volume_envelope.decay = lookup_env_time_value_i32(timecents_to_milliseconds(decay));
                                split_entry.volume_envelope.release = lookup_env_time_value_i32(timecents_to_milliseconds(release));
                            } else { // Use i16 lookup
                                split_entry.volume_envelope.attack = lookup_env_time_value_i16(timecents_to_milliseconds(attack) as i16);
                                split_entry.volume_envelope.hold = lookup_env_time_value_i16(timecents_to_milliseconds(hold) as i16);
                                split_entry.volume_envelope.decay = lookup_env_time_value_i16(timecents_to_milliseconds(decay) as i16);
                                split_entry.volume_envelope.release = lookup_env_time_value_i16(timecents_to_milliseconds(release) as i16);
                            }
                            
                            possibly_a_global_zone && is_first_zone
                        }

                        /// Function to create splits from zones
                        fn create_splits_from_zones(global_preset_zone: Option<&Zone>, preset_zone: &Zone, instrument_zones: &Vec<Zone>, sample_infos: &Vec<SampleInfo>, first_available_id: usize, sample_rate_adjustment_curve: usize) -> Vec<SplitEntry> {
                            let mut splits = Vec::with_capacity(instrument_zones.len());
                            let mut global_instrument_zone: Option<&Zone> = None;
                            for (i, instrument_zone) in instrument_zones.iter().enumerate() {
                                let mut split = SplitEntry::default();
                                split.lowkey = 0;
                                split.hikey = 127;
                                split.lovel = 0;
                                split.hivel = 127;
                                if let Some(&sample_i) = instrument_zone.sample() {
                                    let smpl_ref = &sample_infos[sample_i as usize];
                                    split.ctune = smpl_ref.ctune;
                                    split.ftune = smpl_ref.ftune;
                                    split.rootkey = smpl_ref.rootkey;
                                    split.volume_envelope = smpl_ref.volume_envelope.clone();
                                } else if i != 0 {
                                    println!("{}Some instrument zones contain no samples!", "Warning: ".yellow());
                                    continue;
                                } else {
                                    split.ctune = 0;
                                    split.ftune = 0;
                                    split.rootkey = 60;
                                    split.volume_envelope = ADSRVolumeEnvelope::default();
                                    println!("{}", "Global instrument zone detected!".green());
                                }
                                split.smplvol = 127;
                                split.smplpan = 64;
                                split.kgrpid = 0;
                                if let Some(global_preset_zone) = global_preset_zone {
                                    apply_zone_data_to_split(&mut split, global_preset_zone, false, &[instrument_zone], sample_infos, first_available_id, sample_rate_adjustment_curve);
                                }
                                apply_zone_data_to_split(&mut split, preset_zone, false, &[instrument_zone], sample_infos, first_available_id, sample_rate_adjustment_curve);
                                if let Some(global_instrument_zone) = global_instrument_zone {
                                    apply_zone_data_to_split(&mut split, global_instrument_zone, false, &[preset_zone], sample_infos, first_available_id, sample_rate_adjustment_curve);
                                }
                                if apply_zone_data_to_split(&mut split, instrument_zone, i == 0, &(|| {
                                    let mut other_zones = Vec::new();
                                    if let Some(global_instrument_zone) = global_instrument_zone {
                                        other_zones.push(global_instrument_zone);
                                    }
                                    other_zones.push(preset_zone);
                                    if let Some(global_preset_zone) = global_preset_zone {
                                        other_zones.push(global_preset_zone);
                                    }
                                    other_zones
                                })(), sample_infos, first_available_id, sample_rate_adjustment_curve) {
                                    global_instrument_zone = Some(instrument_zone);
                                }
                                splits.push(split);
                            }
                            splits
                        }

                        // Create splits
                        let mut global_preset_zone: Option<&Zone> = None;
                        let splits: Vec<SplitEntry> = preset.zones.iter().enumerate().map(|(i, preset_zone)| {
                            if let Some(&instrument_i) = preset_zone.instrument() {
                                let instrument = &sf2.instruments[instrument_i as usize];
                                create_splits_from_zones(global_preset_zone, preset_zone, &instrument.zones, &sample_infos, first_available_id, sample_rate_adjustment_curve)
                            } else if i == 0 {
                                global_preset_zone = Some(preset_zone);
                                println!("{}", "Global preset zone detected!".green());
                                Vec::new()
                            } else {
                                println!("{}Some preset zones contain no instruments!", "Warning: ".yellow());
                                Vec::new()
                            }
                        }).flatten().enumerate().map(|(i, mut x)| {
                            x.id = i as u8;
                            x
                        }).collect();
                        program_info.splits_table.objects = splits;

                        // Add to the prgi chunk
                        prgi_pointer_table.objects.push(program_info);
                    }
                    prgi_pointer_table
                }

                fn create_track_swdl(last_modified: (u16, u8, u8, u8, u8, u8, u8), fname: String) -> Result<SWDL, DSEError> {
                    let mut track_swdl = SWDL::default();
                    let (year, month, day, hour, minute, second, centisecond) = last_modified;
                    track_swdl.header.version = 0x415;
                    track_swdl.header.year = year;
                    track_swdl.header.month = month;
                    track_swdl.header.day = day;
                    track_swdl.header.hour = hour;
                    track_swdl.header.minute = minute;
                    track_swdl.header.second = second;
                    track_swdl.header.centisecond = centisecond;

                    track_swdl.header.fname = DSEString::<0xAA>::try_from(fname)?;

                    Ok(track_swdl)
                }

                let (first_id, sample_infos) = copy_raw_sample_data(&File::open(&input_file_path)?, &sf2, &mut main_bank_swdl, DSPOptions { resample_threshold: *resample_threshold, sample_rate: *sample_rate as f64, adpcm_encoder_lookahead: *adpcm_encoder_lookahead }, *sample_rate_adjustment_curve, |(i, sample_header)| true)?;

                // Create a blank track SWDL file
                let mut fname = input_file_path.file_name().ok_or(DSEError::_FileNameReadFailed(input_file_path.display().to_string()))?
                    .to_str().ok_or(DSEError::DSEFileNameConversionNonUTF8("SF2".to_string(), input_file_path.display().to_string()))?
                    .to_string();
                if !fname.is_ascii() {
                    return Err(DSEError::DSEFileNameConversionNonASCII("SF2".to_string(), fname));
                }
                fname.truncate(15);

                let mut track_swdl = create_track_swdl(get_file_last_modified_date_with_default(&input_file_path)?, fname)?;
                let prgi_pointer_table = copy_presets(&sf2, &sample_infos, first_id, *sample_rate_adjustment_curve);

                // Add the sample info objects we created before
                track_swdl.wavi.data.objects = sample_infos;
                let mut prgi = PRGIChunk::new(0);
                prgi.data = prgi_pointer_table;
                track_swdl.prgi = Some(prgi);

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
                track_swdl.regenerate_read_markers()?;
                track_swdl.regenerate_automatic_parameters()?;
                track_swdl.write_to_file(&mut open_file_overwrite_rw(output_file_path)?)?;

                println!("done!");
            }

            let out_swdl_path = out_swdl_path.clone().unwrap_or(std::env::current_dir()?.join("bgm.patched.swd"));
            main_bank_swdl.regenerate_read_markers()?;
            main_bank_swdl.regenerate_automatic_parameters()?;
            main_bank_swdl.write_to_file(&mut open_file_overwrite_rw(out_swdl_path)?)?;
        },
    }

    Ok(())
}

pub fn sample_rate_adjustment_in_cents(sample_rate: f64) -> f64 {
    ((sample_rate - 1115.9471180474397) / 31832.602532753794).ln() / 0.0005990154279493774
}
pub fn cents_to_ctune_ftune(mut cents: i64) -> (i8, i8) {
    let mut sign = 1;
    if cents == 0 {
        return (0, 0);
    } else if cents < 0 {
        sign = -1;
    }
    cents = cents.abs();

    let mut ctune = 0;
    let mut ftune = cents;
    while ftune >= 100 {
        ftune -= 100;
        ctune += 1;
    }
    (sign * ctune as i8, sign * ftune as i8)
}
pub fn sample_rate_adjustment_3(sample_rate: f64) -> Result<(i8, i8), DSEError> {
    Ok(cents_to_ctune_ftune(sample_rate_adjustment_in_cents(sample_rate) as i64))
}
pub fn sample_rate_adjustment_1(sample_rate: f64) -> Result<(i8, i8), DSEError> {
    Ok(cents_to_ctune_ftune((1200.0 * (sample_rate / 32728.5).log2()).round() as i64))
}
pub fn sample_rate_adjustment_4(sample_rate: f64) -> Result<(i8, i8), DSEError> {
    Ok(cents_to_ctune_ftune((1200.0 * (sample_rate / 32728.5).log2()).round() as i64 - 100))
}
static SAMPLE_RATE_ADJUSTMENT_TABLE: phf::Map<u32, i64> = phf_map! {
    8000_u32 => -2600_i64,	11025_u32 => -1858_i64,	11031_u32 => -1856_i64,	11069_u32 => -1841_i64,	
    11281_u32 => -2013_i64,	14000_u32 => -1424_i64,	14002_u32 => -1423_i64,	14003_u32 => -1423_i64,	
    14004_u32 => -1422_i64,	14007_u32 => -1421_i64,	14008_u32 => -1421_i64,	16000_u32 => -1400_i64,	
    16002_u32 => -1399_i64,	16004_u32 => -1399_i64,	16006_u32 => -1398_i64,	16008_u32 => -1397_i64,	
    16011_u32 => -1397_i64,	16013_u32 => -1396_i64,	16014_u32 => -1396_i64,	16015_u32 => -1396_i64,	
    16016_u32 => -1395_i64,	16019_u32 => -1394_i64,	16020_u32 => -1394_i64,	16034_u32 => -1390_i64,	
    18000_u32 => -1190_i64,	18001_u32 => -1189_i64,	18003_u32 => -1189_i64,	20000_u32 => -779_i64,	
    22021_u32 => -664_i64,	22030_u32 => -662_i64,	22050_u32 => -658_i64,	22051_u32 => -658_i64,	
    22052_u32 => -658_i64,	22053_u32 => -658_i64,	22054_u32 => -657_i64,	22055_u32 => -657_i64,	
    22057_u32 => -657_i64,	22058_u32 => -657_i64,	22059_u32 => -656_i64,	22061_u32 => -656_i64,	
    22062_u32 => -656_i64,	22063_u32 => -656_i64,	22064_u32 => -655_i64,	22066_u32 => -655_i64,	
    22068_u32 => -655_i64,	22069_u32 => -654_i64,	22071_u32 => -654_i64,	22073_u32 => -654_i64,	
    22074_u32 => -653_i64,	22075_u32 => -653_i64,	22076_u32 => -653_i64,	22077_u32 => -653_i64,	
    22078_u32 => -653_i64,	22079_u32 => -652_i64,	22081_u32 => -652_i64,	22082_u32 => -652_i64,	
    22084_u32 => -651_i64,	22085_u32 => -651_i64,	22086_u32 => -651_i64,	22087_u32 => -651_i64,	
    22088_u32 => -651_i64,	22092_u32 => -650_i64,	22093_u32 => -650_i64,	22099_u32 => -648_i64,	
    22102_u32 => -648_i64,	22106_u32 => -647_i64,	22108_u32 => -647_i64,	22112_u32 => -646_i64,	
    22115_u32 => -645_i64,	22121_u32 => -644_i64,	22122_u32 => -644_i64,	22124_u32 => -643_i64,	
    22132_u32 => -642_i64,	22133_u32 => -642_i64,	22142_u32 => -640_i64,	22148_u32 => -639_i64,	
    22151_u32 => -638_i64,	22154_u32 => -637_i64,	22158_u32 => -637_i64,	22160_u32 => -636_i64,	
    22167_u32 => -635_i64,	22171_u32 => -634_i64,	22179_u32 => -632_i64,	22180_u32 => -632_i64,	
    22186_u32 => -631_i64,	22189_u32 => -630_i64,	22196_u32 => -629_i64,	22201_u32 => -628_i64,	
    22202_u32 => -628_i64,	22213_u32 => -626_i64,	22223_u32 => -624_i64,	22226_u32 => -623_i64,	
    22260_u32 => -616_i64,	22276_u32 => -613_i64,	22282_u32 => -612_i64,	22349_u32 => -599_i64,	
    22400_u32 => -588_i64,	22406_u32 => -587_i64,	22450_u32 => -579_i64,	22508_u32 => -823_i64,	
    22828_u32 => -761_i64,	22932_u32 => -740_i64,	22963_u32 => -734_i64,	23000_u32 => -727_i64,	
    23100_u32 => -708_i64,	24000_u32 => -695_i64,	24011_u32 => -693_i64,	24014_u32 => -692_i64,	
    24054_u32 => -685_i64,	25200_u32 => -378_i64,	26000_u32 => -396_i64,	26059_u32 => -386_i64,	
    32000_u32 => -200_i64,	32001_u32 => -200_i64,	32004_u32 => -199_i64,	32005_u32 => -199_i64,	
    32012_u32 => -198_i64,	32024_u32 => -196_i64,	32033_u32 => -195_i64,	32034_u32 => -195_i64,	
    32044_u32 => -194_i64,	32057_u32 => -192_i64,	32065_u32 => -191_i64,	32105_u32 => -185_i64,	
    32114_u32 => -184_i64,	32136_u32 => -181_i64,	44100_u32 => 542_i64,	44102_u32 => 542_i64,	
    44103_u32 => 542_i64,	44110_u32 => 543_i64,	44112_u32 => 543_i64,	44131_u32 => 545_i64,	
    44132_u32 => 545_i64,	44177_u32 => 549_i64,	44182_u32 => 550_i64,	44210_u32 => 553_i64,	
    44225_u32 => 554_i64,	44249_u32 => 557_i64,	44539_u32 => 586_i64,	45158_u32 => 391_i64,	
    45264_u32 => 401_i64,	45656_u32 => 439_i64
};
pub fn sample_rate_adjustment_2(sample_rate: f64) -> Result<(i8, i8), DSEError> {
    let smplrate = sample_rate.round() as u32;
    if let Some(&cents) = SAMPLE_RATE_ADJUSTMENT_TABLE.get(&smplrate) {
        println!("{:?}", cents_to_ctune_ftune(cents));
        Ok(cents_to_ctune_ftune(cents))
    } else {
        Err(DSEError::SampleRateUnsupported(sample_rate))
    }
}
pub fn sample_rate_adjustment(sample_rate: f64, curve: usize) -> Result<(i8, i8), DSEError> {
    match curve {
        1 => sample_rate_adjustment_1(sample_rate),
        2 => sample_rate_adjustment_2(sample_rate),
        3 => sample_rate_adjustment_3(sample_rate),
        4 => sample_rate_adjustment_4(sample_rate),
        _ => Err(DSEError::Invalid("Invalid sample rate adjustment curve number!".to_string()))
    }
}

// https://projectpokemon.org/docs/mystery-dungeon-nds/dse-swdl-format-r14/#SWDL_Header
const LOOKUP_TABLE_20_B0_F50: [i16; 128] = [
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007, 
    0x0008, 0x0009, 0x000A, 0x000B, 0x000C, 0x000D, 0x000E, 0x000F, 
    0x0010, 0x0011, 0x0012, 0x0013, 0x0014, 0x0015, 0x0016, 0x0017, 
    0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F, 
    0x0020, 0x0023, 0x0028, 0x002D, 0x0033, 0x0039, 0x0040, 0x0048, 
    0x0050, 0x0058, 0x0062, 0x006D, 0x0078, 0x0083, 0x0090, 0x009E, 
    0x00AC, 0x00BC, 0x00CC, 0x00DE, 0x00F0, 0x0104, 0x0119, 0x012F, 
    0x0147, 0x0160, 0x017A, 0x0196, 0x01B3, 0x01D2, 0x01F2, 0x0214, 
    0x0238, 0x025E, 0x0285, 0x02AE, 0x02D9, 0x0307, 0x0336, 0x0367, 
    0x039B, 0x03D1, 0x0406, 0x0442, 0x047E, 0x04C4, 0x0500, 0x0546, 
    0x058C, 0x0622, 0x0672, 0x06CC, 0x071C, 0x0776, 0x07DA, 0x0834, 
    0x0898, 0x0906, 0x096A, 0x09D8, 0x0A50, 0x0ABE, 0x0B40, 0x0BB8, 
    0x0C3A, 0x0CBC, 0x0D48, 0x0DDE, 0x0E6A, 0x0F00, 0x0FA0, 0x1040, 
    0x10EA, 0x1194, 0x123E, 0x12F2, 0x13B0, 0x146E, 0x1536, 0x15FE, 
    0x16D0, 0x17A2, 0x187E, 0x195A, 0x1A40, 0x1B30, 0x1C20, 0x1D1A, 
    0x1E1E, 0x1F22, 0x2030, 0x2148, 0x2260, 0x2382, 0x2710, 0x7FFF
];
const LOOKUP_TABLE_20_B1050: [i32; 128] = [
    0x00000000, 0x00000004, 0x00000007, 0x0000000A, 
    0x0000000F, 0x00000015, 0x0000001C, 0x00000024, 
    0x0000002E, 0x0000003A, 0x00000048, 0x00000057, 
    0x00000068, 0x0000007B, 0x00000091, 0x000000A8, 
    0x00000185, 0x000001BE, 0x000001FC, 0x0000023F, 
    0x00000288, 0x000002D6, 0x0000032A, 0x00000385, 
    0x000003E5, 0x0000044C, 0x000004BA, 0x0000052E, 
    0x000005A9, 0x0000062C, 0x000006B5, 0x00000746, 
    0x00000BCF, 0x00000CC0, 0x00000DBD, 0x00000EC6, 
    0x00000FDC, 0x000010FF, 0x0000122F, 0x0000136C, 
    0x000014B6, 0x0000160F, 0x00001775, 0x000018EA, 
    0x00001A6D, 0x00001BFF, 0x00001DA0, 0x00001F51, 
    0x00002C16, 0x00002E80, 0x00003100, 0x00003395, 
    0x00003641, 0x00003902, 0x00003BDB, 0x00003ECA, 
    0x000041D0, 0x000044EE, 0x00004824, 0x00004B73, 
    0x00004ED9, 0x00005259, 0x000055F2, 0x000059A4, 
    0x000074CC, 0x000079AB, 0x00007EAC, 0x000083CE, 
    0x00008911, 0x00008E77, 0x000093FF, 0x000099AA, 
    0x00009F78, 0x0000A56A, 0x0000AB80, 0x0000B1BB, 
    0x0000B81A, 0x0000BE9E, 0x0000C547, 0x0000CC17, 
    0x0000FD42, 0x000105CB, 0x00010E82, 0x00011768, 
    0x0001207E, 0x000129C4, 0x0001333B, 0x00013CE2, 
    0x000146BB, 0x000150C5, 0x00015B02, 0x00016572, 
    0x00017015, 0x00017AEB, 0x000185F5, 0x00019133, 
    0x0001E16D, 0x0001EF07, 0x0001FCE0, 0x00020AF7, 
    0x0002194F, 0x000227E6, 0x000236BE, 0x000245D7, 
    0x00025532, 0x000264CF, 0x000274AE, 0x000284D0, 
    0x00029536, 0x0002A5E0, 0x0002B6CE, 0x0002C802, 
    0x000341B0, 0x000355F8, 0x00036A90, 0x00037F79, 
    0x000394B4, 0x0003AA41, 0x0003C021, 0x0003D654, 
    0x0003ECDA, 0x000403B5, 0x00041AE5, 0x0004326A, 
    0x00044A45, 0x00046277, 0x00047B00, 0x7FFFFFFF
];

