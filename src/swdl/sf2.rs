use std::collections::{HashMap, BTreeMap};
use std::io::{Seek, Cursor, Read};

use byteorder::{ReadBytesExt, LittleEndian, WriteBytesExt};
use colored::Colorize;
use crate::math::{timecents_to_milliseconds, gain};
use crate::swdl::{SWDL, SampleInfo, ADSRVolumeEnvelope, ProgramInfo, SplitEntry, LFOEntry, PCMDChunk, Tuning};
use crate::dtype::{DSEError, PointerTable};

use dse_dsp_sys::{adpcm_encode_16bitpcm_byte_pos_preview_batch, block_alignment, process_16bitpcm_preserve_looping, resample_len_preview, SampleRateChoicePreference};
use soundfont::data::{SampleHeader, GeneratorType};
use soundfont::{SoundFont2, Zone, Preset, Instrument};

use super::{BUILT_IN_SAMPLE_RATE_ADJUSTMENT_TABLE, lookup_env_time_value_i16, lookup_env_time_value_i32, SWDLHeader};

pub struct DSPOptions {
    pub resample_threshold: u32,
    pub sample_rate: f64,
    pub sample_rate_relative: bool,
    pub adpcm_encoder_lookahead: i32,
    pub adpcm_encoder_noise_shaping: i32
}
pub fn copy_raw_sample_data<R>(mut sf2file: R, sf2: &SoundFont2, bank: &mut SWDL, dsp_options: DSPOptions, sample_rate_adjustment_curve: usize, pitch_adjust: i64, mut filter_samples: impl FnMut(usize, &SampleHeader) -> bool) -> Result<(HashMap<u16, u16>, BTreeMap<u16, SampleInfo>), DSEError>
where
    R: Read + Seek {
    let main_bank_swdl_pcmd = bank.pcmd.get_or_insert(PCMDChunk::default());
    let main_bank_swdl_wavi = &mut bank.wavi;

    let first_sample_pos = main_bank_swdl_wavi.data.objects.iter().map(|x| x.smplpos + (x.loopbeg + x.looplen) * 4).max().unwrap_or(0);

    // Create the SampleInfo entries for all the samples
    let mut sample_infos = BTreeMap::new(); //::with_capacity(sf2.sample_headers.len())
    let first_available_id = main_bank_swdl_wavi.data.slots();
    let mut pos_in_memory = 0;

    // Record the sample ID mappings
    let mut sample_mappings = HashMap::new();

    for (old_i, i, sample_header) in sf2.sample_headers.iter().enumerate().filter(|&(i, sample_header)| filter_samples(i, sample_header)).enumerate().map(|(i, (old_i, sample_header))| (old_i, i, sample_header)) {
        // Create blank sampleinfo object
        let mut sample_info = SampleInfo::default();

        // ID
        sample_info.id = (first_available_id + i) as u16;
        sample_mappings.insert(old_i as u16, sample_info.id);

        sample_info.smplrate = sample_header.sample_rate;
        if sample_header.origpitch >= 128 { // origpitch - 255 is reserved for percussion by convention, 128-254 is invalid, but either way the SF2 standard recommends defaulting to 60 when 128-255 is encountered.
            sample_info.rootkey = 60;
        } else {
            sample_info.rootkey = sample_header.origpitch as i8;
        }
        sample_info.volume = 127; // SF2 does not have a volume parameter per sample
        sample_info.pan = 64; // SF2 does not have a pan parameter per sample, and any panning work related to stereo samples are relegated to the Instruments layer anyways
        sample_info.smplfmt = 0x0200; // SF2 supports 16-bit PCM and 24-bit PCM, and while DSE also supports 16-bit PCM, the problem comes with file size. 16-bit PCM is **massive**, and so it's very hard to fit many samples into the limited memory of the NDS, which could explain the abundant use of 4-bit ADPCM in the original game songs. With that in mind, here we will internally encode the sample data as ADPCM, and on top of that, lower the sample rate if necessary to compress the sample data as much as we possibly can.
        sample_info.smplloop = false; // SF2 does not loop samples by default.
        // smplrate is up above with ctune and ftune
        // smplpos is at the bottom
        // WARNING FOR THE FUTURE:
        //  If you are implementing direct 16-bit PCM sample import for some reason, this needs to be checked over.
        // NOTE ABOUT THIS:
        //  The loopbeg and looplen are overwritten if the sample is resampled and never used. It will only be read and used if the sample is not being resampled.
        if sample_header.loop_start >= sample_header.start &&
            sample_header.loop_end > sample_header.loop_start {
            sample_info.loopbeg = (sample_header.loop_start - sample_header.start) / 2;
            sample_info.looplen = (sample_header.loop_end - sample_header.loop_start) / 2;
        } else {
            // Probably not looping, so loop_start could be zero. Manually set to zero instead.
            sample_info.loopbeg = 0;
            // Probably not looping, so loop_end - loop_start is zero. Use end - start instead.
            sample_info.looplen = (sample_header.end - sample_header.start) / 2;
        }
        // Write sample into main bank
        if let Some(chunk) = sf2.sample_data.smpl.as_ref() {
            let sample_pos_bytes = chunk.offset() + 8 + sample_header.start as u64 * 2;
            let mut raw_sample_data = vec![0_i16; (sample_header.end - sample_header.start) as usize];

            sf2file.seek(std::io::SeekFrom::Start(sample_pos_bytes)).map_err(|_| DSEError::SampleFindError(sample_header.name.clone(), sample_pos_bytes))?;
            sf2file.read_i16_into::<LittleEndian>(&mut raw_sample_data).map_err(|_| DSEError::SampleReadError(sample_header.name.clone(), sample_pos_bytes, raw_sample_data.len()))?;

            // Resample and encode to ADPCM
            let mut new_sample_rate = if sample_header.sample_rate > dsp_options.resample_threshold {
                if dsp_options.sample_rate_relative {
                    if dsp_options.sample_rate >= 1.0 {
                        dsp_options.sample_rate * (sample_header.sample_rate as f64)
                    } else {
                        let mut accum = sample_header.sample_rate as f64;
                        while accum > dsp_options.resample_threshold as f64 {
                            accum *= dsp_options.sample_rate;
                        }
                        accum
                    }
                } else {
                    dsp_options.sample_rate
                }
            } else {
                sample_header.sample_rate as f64
            }.round(); // Rounding is required since the smplrate value in DSE is u32
            let (mut raw_sample_data, new_loop_bounds) = {
                let raw_sample_data_pre_loop;
                let raw_sample_data_loop;
                if sample_header.loop_start >= sample_header.start &&
                    sample_header.loop_end > sample_header.loop_start {
                    let loopbeg_in_sample_points = (sample_header.loop_start - sample_header.start) as usize;
                    let loopend_in_sample_points = (sample_header.loop_end - sample_header.start) as usize;
                    raw_sample_data_pre_loop = &raw_sample_data[..loopbeg_in_sample_points];
                    raw_sample_data_loop = &raw_sample_data[loopbeg_in_sample_points..loopend_in_sample_points];
                } else {
                    raw_sample_data_pre_loop = &raw_sample_data[..0];
                    raw_sample_data_loop = &raw_sample_data[..];
                }
                let resampled;
                let samples_per_block;
                (resampled, new_sample_rate, samples_per_block) = process_16bitpcm_preserve_looping(
                    &[raw_sample_data_pre_loop],
                    &[raw_sample_data_loop],
                    sample_header.sample_rate as f64,
                    new_sample_rate,
                    dsp_options.adpcm_encoder_lookahead, dsp_options.adpcm_encoder_noise_shaping, 128, block_alignment::To8Bytes(), SampleRateChoicePreference::Higher,
                    None);
                new_sample_rate = new_sample_rate.round(); // Rounding is required since the smplrate value in DSE is u32
                let tracking = {
                    let pre_loop_resampled_length = resample_len_preview(sample_header.sample_rate as f64, new_sample_rate, raw_sample_data_pre_loop.len());
                    let loop_resampled_length = resample_len_preview(sample_header.sample_rate as f64, new_sample_rate, raw_sample_data_loop.len());
                    let loop_start_in_sample_points = pre_loop_resampled_length;
                    let loop_end_in_sample_points = if pre_loop_resampled_length == 0 && loop_resampled_length == 0 { 0 } else { pre_loop_resampled_length + loop_resampled_length - 1 };
                    adpcm_encode_16bitpcm_byte_pos_preview_batch(samples_per_block, 1, &[loop_start_in_sample_points, loop_end_in_sample_points])
                };
                (resampled, tracking)
            };
            sample_info.smplrate = new_sample_rate as u32; // Set new sample rate
            let mut tuning = sample_rate_adjustment(new_sample_rate, sample_rate_adjustment_curve, pitch_adjust)?;
            tuning.add_cents(sample_header.pitchadj as i64);
            sample_info.tuning = tuning;
            let raw_sample_data_len_32 = raw_sample_data.len() as u32 / 4;
            sample_info.loopbeg = (new_loop_bounds[0] as u32 / 4).min(raw_sample_data_len_32); // Set new loopbeg
            sample_info.looplen = raw_sample_data_len_32 - sample_info.loopbeg; // Set new looplen
            raw_sample_data.resize((sample_info.loopbeg as usize + sample_info.looplen as usize) * 4, 0);

            // Write the sample
            let mut cursor = Cursor::new(&mut main_bank_swdl_pcmd.data);
            cursor.seek(std::io::SeekFrom::Start(pos_in_memory as u64 + first_sample_pos as u64)).map_err(|_| DSEError::_InMemorySeekFailed())?;
            for sample in raw_sample_data.into_iter() {
                cursor.write_u8(sample).map_err(|_| DSEError::_InMemoryWriteFailed())?;
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
        sample_infos.insert(sample_info.id, sample_info_track_swdl);
        // Add the other sampleinfo object into the main bank's swdl
        main_bank_swdl_wavi.data.objects.push(sample_info);
    }

    Ok((sample_mappings, sample_infos))
}

pub fn find_gen_in_zones<'a>(zones: &'a [&Zone], ty: GeneratorType) -> Option<&'a soundfont::data::Generator> {
    zones.iter().map(|x| x.gen_list.iter()).flatten().find(|g| g.ty == ty)
}
pub fn copy_presets(sf2: &SoundFont2, sample_infos: &mut BTreeMap<u16, SampleInfo>, prgi_pointer_table: &mut PointerTable<ProgramInfo>, mut map_samples: impl FnMut(u16) -> Option<u16>, sample_rate_adjustment_curve: usize, pitch_adjust: i64, mut filter_instruments: impl FnMut(usize, &Preset, Option<&Zone>, usize, &Zone, u16, &Instrument) -> bool, mut map_presets: impl FnMut(usize, &Preset, &ProgramInfo) -> Option<u16>) {
    // Loop through the presets and use it to fill in the track swdl object
    for (preset_i, preset) in sf2.presets.iter().enumerate() {
        // Create blank programinfo object
        let mut program_info = ProgramInfo::default();

        // ID
        program_info.header.prgvol = 127;
        program_info.header.prgpan = 64;
        program_info.header.PadByte = 170;

        // Create the 4 LFOs (each preset in SF2 can have many instruments, with each instruments containing multiple samples, and each of those samples can have their own LFOs. 4 is just not enough to map all that, and so this is left to its default state. For now, please add LFOs manually to taste :)
        let lfos: Vec<LFOEntry> = (0..4).map(|_| LFOEntry::default()).collect();
        program_info.lfo_table.objects = lfos;

        /// Function to apply data from a zone to a split
        /// 
        /// Returns `true` if the zone provided is a global zone
        fn apply_zone_data_to_split(split_entry: &mut SplitEntry, additive: Option<&[&Zone]>, zone: &Zone, sample_infos: &mut BTreeMap<u16, SampleInfo>, sample_i: u16, mut map_samples: impl FnMut(u16) -> Option<u16>, sample_rate_adjustment_curve: usize, pitch_adjust: i64) {
            // https://stackoverflow.com/questions/67016985/map-numeric-range-rust
            fn map_range(from_range: (f64, f64), to_range: (f64, f64), s: f64) -> f64 {
                to_range.0 + (s - from_range.0) * (to_range.1 - to_range.0) / (from_range.1 - from_range.0)
            }
            
            let (mut attack, mut hold, mut decay, mut release) = (None, None, None, None);
            let fill_env_from_additive_source = |param: &mut Option<i16>, gen_ty: soundfont::data::GeneratorType| {
                if let None = param {
                    *param = additive.map_or(None,
                        |additive_source_zones| find_gen_in_zones(
                            additive_source_zones,
                            gen_ty
                        ).map(|g| *g.amount.as_i16().unwrap()));
                }
            };

            // Loop through all the generators in this zone
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
                        split_entry.smplpan = map_range((-500.0, 500.0), (0.0, 127.0), (
                            *gen.amount.as_i16().unwrap() + if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::Pan).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 }
                        ) as f64).round() as i8;
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
                        if let Some(_) = additive  {
                            fill_env_from_additive_source(&mut attack, soundfont::data::GeneratorType::AttackVolEnv);
                            attack = Some(attack.unwrap_or(0) + *gen.amount.as_i16().unwrap());
                        } else {
                            attack = Some(*gen.amount.as_i16().unwrap());
                        }
                    },
                    soundfont::data::GeneratorType::HoldVolEnv => {
                        if let Some(_) = additive {
                            fill_env_from_additive_source(&mut hold, soundfont::data::GeneratorType::HoldVolEnv);
                            hold = Some(hold.unwrap_or(0) + *gen.amount.as_i16().unwrap());
                        } else {
                            hold = Some(*gen.amount.as_i16().unwrap());
                        }
                    },
                    soundfont::data::GeneratorType::DecayVolEnv => {
                        if let Some(_) = additive {
                            fill_env_from_additive_source(&mut decay, soundfont::data::GeneratorType::DecayVolEnv);
                            decay = Some(decay.unwrap_or(0) + *gen.amount.as_i16().unwrap());
                        } else {
                            decay = Some(*gen.amount.as_i16().unwrap());
                        }
                    },
                    soundfont::data::GeneratorType::SustainVolEnv => {
                        let decibels = -(gen.amount.as_i16().unwrap() + if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::SustainVolEnv).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 }) as f64 / 10.0_f64;
                        split_entry.volume_envelope.sustain = (gain(decibels) * 127.0).round() as i8;
                    },
                    soundfont::data::GeneratorType::ReleaseVolEnv => {
                        if let Some(_) = additive {
                            fill_env_from_additive_source(&mut release, soundfont::data::GeneratorType::ReleaseVolEnv);
                            release = Some(release.unwrap_or(0) + *gen.amount.as_i16().unwrap());
                        } else {
                            release = Some(*gen.amount.as_i16().unwrap());
                        }
                    },
                    soundfont::data::GeneratorType::KeynumToVolEnvHold => {  },
                    soundfont::data::GeneratorType::KeynumToVolEnvDecay => {  },
                    soundfont::data::GeneratorType::Instrument => {  },
                    soundfont::data::GeneratorType::Reserved1 => {  },
                    soundfont::data::GeneratorType::KeyRange => {
                        if let Some(additive_source_zones) = additive {
                            if let Some(base_key_range_gen) = find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::KeyRange) {
                                let base_key_range_value = base_key_range_gen.amount.as_range().unwrap();
                                let limiting_key_range_value = gen.amount.as_range().unwrap();
                                split_entry.lowkey = (base_key_range_value.low as i8).max(limiting_key_range_value.low as i8);
                                split_entry.hikey = (base_key_range_value.high as i8).min(limiting_key_range_value.high as i8);
                            } else {
                                let key_range_value = gen.amount.as_range().unwrap();
                                split_entry.lowkey = key_range_value.low as i8;
                                split_entry.hikey = key_range_value.high as i8;
                            }
                        } else {
                            let key_range_value = gen.amount.as_range().unwrap();
                            split_entry.lowkey = key_range_value.low as i8;
                            split_entry.hikey = key_range_value.high as i8;
                        }
                    },
                    soundfont::data::GeneratorType::VelRange => {
                        if let Some(additive_source_zones) = additive {
                            if let Some(base_vel_range_gen) = find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::VelRange) {
                                let base_vel_range_value = base_vel_range_gen.amount.as_range().unwrap();
                                let limiting_vel_range_value = gen.amount.as_range().unwrap();
                                split_entry.lovel = (base_vel_range_value.low as i8).max(limiting_vel_range_value.low as i8);
                                split_entry.hivel = (base_vel_range_value.high as i8).min(limiting_vel_range_value.high as i8);
                            } else {
                                let vel_range_value = gen.amount.as_range().unwrap();
                                split_entry.lovel = vel_range_value.low as i8;
                                split_entry.hivel = vel_range_value.high as i8;
                            }
                        } else {
                            let vel_range_value = gen.amount.as_range().unwrap();
                            split_entry.lovel = vel_range_value.low as i8;
                            split_entry.hivel = vel_range_value.high as i8;
                        }
                    },
                    soundfont::data::GeneratorType::StartloopAddrsCoarseOffset => {  },
                    soundfont::data::GeneratorType::Keynum => {  },
                    soundfont::data::GeneratorType::Velocity => {  },
                    soundfont::data::GeneratorType::InitialAttenuation => {
                        let mut decibels = -(gen.amount.as_i16().unwrap() + if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::InitialAttenuation).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 }) as f64 / 10.0_f64;
                        // Every 1dB of attenuation specified should attenuate by 0.4dB
                        // See https://www.polyphone-soundfonts.com/forum/soundfonts-help/29-understanding-attenuation for more information
                        decibels *= 0.4;
                        split_entry.smplvol = (gain(decibels) * 127.0).round() as i8;
                    },
                    soundfont::data::GeneratorType::Reserved2 => {  },
                    soundfont::data::GeneratorType::EndloopAddrsCoarseOffset => {  },
                    soundfont::data::GeneratorType::CoarseTune => {
                        let smpl = sample_infos.get(&map_samples(sample_i).unwrap()).ok_or(DSEError::_SampleInPresetMissing(map_samples(sample_i).unwrap())).unwrap();
                        let mut tuning = sample_rate_adjustment(smpl.smplrate as f64, sample_rate_adjustment_curve, pitch_adjust).unwrap();
                        tuning.add_semitones(*gen.amount.as_i16().unwrap() as i64);
                        tuning.add_semitones(if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::CoarseTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 } as i64);
                        tuning.add_cents(find_gen_in_zones(&[&zone], soundfont::data::GeneratorType::FineTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) as i64);
                        tuning.add_cents(if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::FineTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 } as i64);
                        split_entry.tuning = tuning;
                    },
                    soundfont::data::GeneratorType::FineTune => {
                        let smpl = sample_infos.get(&map_samples(sample_i).unwrap()).ok_or(DSEError::_SampleInPresetMissing(map_samples(sample_i).unwrap())).unwrap();
                        let mut tuning = sample_rate_adjustment(smpl.smplrate as f64, sample_rate_adjustment_curve, pitch_adjust).unwrap();
                        tuning.add_semitones(find_gen_in_zones(&[&zone], soundfont::data::GeneratorType::CoarseTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) as i64);
                        tuning.add_semitones(if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::CoarseTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 } as i64);
                        tuning.add_cents(*gen.amount.as_i16().unwrap() as i64);
                        tuning.add_cents(if let Some(additive_source_zones) = additive { find_gen_in_zones(additive_source_zones, soundfont::data::GeneratorType::FineTune).map(|g| *g.amount.as_i16().unwrap()).unwrap_or(0) } else { 0 } as i64);
                        split_entry.tuning = tuning;
                    },
                    soundfont::data::GeneratorType::SampleID => {
                        // Check if the zone specifies which sample we have to use!
                        split_entry.SmplID = map_samples(*gen.amount.as_u16().unwrap()).unwrap();
                    },
                    soundfont::data::GeneratorType::SampleModes => {
                        let smpl = sample_infos.get_mut(&map_samples(sample_i).unwrap()).ok_or(DSEError::_SampleInPresetMissing(map_samples(sample_i).unwrap())).unwrap();
                        let flags = u16::from_ne_bytes(gen.amount.as_i16().unwrap().to_ne_bytes());
                        smpl.smplloop = (flags & 0x3) % 2 == 1;
                    },
                    soundfont::data::GeneratorType::Reserved3 => {  },
                    soundfont::data::GeneratorType::ScaleTuning => {  },
                    soundfont::data::GeneratorType::ExclusiveClass => {  },
                    soundfont::data::GeneratorType::OverridingRootKey => {
                        let val = *gen.amount.as_i16().unwrap();
                        if val != -1 && additive.is_none() {
                            split_entry.rootkey = val as i8;
                        }
                    },
                    soundfont::data::GeneratorType::Unused5 => {  },
                    soundfont::data::GeneratorType::EndOper => {  },
                }
            }
            let max_envelope_value = [attack, hold, decay, release].iter().filter_map(|x| x.as_ref()).max().map(|x| timecents_to_index(*x));
            if let Some((envmult, _)) = max_envelope_value {
                split_entry.volume_envelope.envmult = envmult;
                if envmult == 0 { // Use i32 lookup
                    if let Some(attack) = attack {
                        split_entry.volume_envelope.attack = lookup_env_time_value_i32(timecents_to_milliseconds(attack));
                    }
                    if let Some(hold) = hold {
                        split_entry.volume_envelope.hold = lookup_env_time_value_i32(timecents_to_milliseconds(hold));
                    }
                    if let Some(decay) = decay {
                        split_entry.volume_envelope.decay = lookup_env_time_value_i32(timecents_to_milliseconds(decay));
                    }
                    if let Some(release) = release {
                        split_entry.volume_envelope.release = lookup_env_time_value_i32(timecents_to_milliseconds(release));
                    }
                } else { // Use i16 lookup
                    if let Some(attack) = attack {
                        split_entry.volume_envelope.attack = lookup_env_time_value_i16(timecents_to_milliseconds(attack) as i16);
                    }
                    if let Some(hold) = hold {
                        split_entry.volume_envelope.hold = lookup_env_time_value_i16(timecents_to_milliseconds(hold) as i16);
                    }
                    if let Some(decay) = decay {
                        split_entry.volume_envelope.decay = lookup_env_time_value_i16(timecents_to_milliseconds(decay) as i16);
                    }
                    if let Some(release) = release {
                        split_entry.volume_envelope.release = lookup_env_time_value_i16(timecents_to_milliseconds(release) as i16);
                    }
                }
            }
        }

        /// Function to create splits from zones
        fn create_splits_from_zones(global_preset_zone: Option<&Zone>, preset_zone: &Zone, instrument_zones: &Vec<Zone>, sample_infos: &mut BTreeMap<u16, SampleInfo>, mut map_samples: impl FnMut(u16) -> Option<u16>, sample_rate_adjustment_curve: usize, pitch_adjust: i64) -> Vec<SplitEntry> {
            let mut splits = Vec::with_capacity(instrument_zones.len());
            let mut global_instrument_zone: Option<&Zone> = None;
            for (i, instrument_zone) in instrument_zones.iter().enumerate() {
                let mut split = SplitEntry::default();
                let mut skip_this_split = false;
                split.lowkey = 0;
                split.hikey = 127;
                split.lovel = 0;
                split.hivel = 127;
                if let Some(&sample_i) = instrument_zone.sample() {
                    if let Some(mapping) = map_samples(sample_i) {
                        let smpl_ref = sample_infos.get(&mapping).ok_or(DSEError::_SampleInPresetMissing(mapping)).unwrap();
                        split.tuning = smpl_ref.tuning;
                        split.rootkey = smpl_ref.rootkey;
                        split.volume_envelope = smpl_ref.volume_envelope.clone();
                    } else {
                        println!("{}", format!("Sample associated with split unmapped! Skipping.").green());
                        continue;
                    }
                } else if i != 0 {
                    println!("{}Some instrument zones contain no samples!", "Warning: ".yellow());
                    continue;
                } else {
                    split.tuning = Tuning::new(0, 0);
                    split.rootkey = 60;
                    split.volume_envelope = ADSRVolumeEnvelope::default();
                    println!("{}", "Global instrument zone detected!".green());
                }
                split.smplvol = 127;
                split.smplpan = 64;
                split.kgrpid = 0;

                if let Some(&sample_i) = instrument_zone.sample() {
                    if let Some(global_instrument_zone) = global_instrument_zone {
                        apply_zone_data_to_split(&mut split, None, global_instrument_zone, sample_infos, sample_i, &mut map_samples, sample_rate_adjustment_curve, pitch_adjust);
                    }
                    apply_zone_data_to_split(&mut split, None, instrument_zone, sample_infos, sample_i, &mut map_samples, sample_rate_adjustment_curve, pitch_adjust);
                    if let Some(global_preset_zone) = global_preset_zone {
                        apply_zone_data_to_split(&mut split, Some(&(|| {
                            let mut additive_source_zones = vec![instrument_zone];
                            if let Some(global_instrument_zone) = global_instrument_zone {
                                additive_source_zones.push(global_instrument_zone);
                            }
                            additive_source_zones
                        })()), global_preset_zone, sample_infos, sample_i, &mut map_samples, sample_rate_adjustment_curve, pitch_adjust);
                    }
                    apply_zone_data_to_split(&mut split, Some(&(|| {
                        let mut additive_source_zones = vec![instrument_zone];
                        if let Some(global_instrument_zone) = global_instrument_zone {
                            additive_source_zones.push(global_instrument_zone);
                        }
                        additive_source_zones
                    })()), preset_zone, sample_infos, sample_i, &mut map_samples, sample_rate_adjustment_curve, pitch_adjust);
                } else if i == 0 {
                    global_instrument_zone = Some(instrument_zone);
                    skip_this_split = true;
                    println!("{}", "Global instrument zone detected!".green());
                } else {
                    skip_this_split = true;
                    println!("{}Some instrument zones contain no samples!", "Warning: ".yellow());
                }

                if !skip_this_split { // If this split represents a global instrument zone or is missing a corresponding sample it should not be included.
                    splits.push(split);
                }
            }
            splits
        }

        // Create splits
        let mut global_preset_zone: Option<&Zone> = None;
        let splits: Vec<SplitEntry> = preset.zones.iter().enumerate().map(|(preset_zone_i, preset_zone)| {
            if let Some(&instrument_i) = preset_zone.instrument() {
                let instrument = &sf2.instruments[instrument_i as usize];
                if filter_instruments(preset_i, &preset, global_preset_zone, preset_zone_i, preset_zone, instrument_i, instrument) {
                    create_splits_from_zones(global_preset_zone, preset_zone, &instrument.zones, sample_infos, &mut map_samples, sample_rate_adjustment_curve, pitch_adjust)
                } else {
                    Vec::new() // The instrument has been filtered out
                }
            } else if preset_zone_i == 0 {
                global_preset_zone = Some(preset_zone);
                println!("{}", "Global preset zone detected!".green());
                Vec::new() // The global preset zone should not be included.
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
        if let Some(mapping) = map_presets(preset_i, preset, &program_info) {
            program_info.header.id = mapping;
            prgi_pointer_table.objects.push(program_info);
        }
    }
}

pub fn find_preset_in_soundfont(soundfont: &SoundFont2, bank: u16, program: u16) -> Option<usize> {
    for (i, preset) in soundfont.presets.iter().enumerate() {
        if preset.header.bank == bank && preset.header.preset == program {
            return Some(i);
        }
    }
    if bank == 128 && program == 0 {
        // Handle case where the soundfont doesn't follow the soundfont2 specs and has the default drumset at 127:000 instead.
        if let Some(resolved_i) = find_preset_in_soundfont(soundfont, 127, 0) {
            return Some(resolved_i);
        }
        // Handle case where the soundfont doesn't follow the soundfont2 specs and has the default drumset at 126:000 instead.
        if let Some(resolved_i) = find_preset_in_soundfont(soundfont, 126, 0) {
            return Some(resolved_i);
        }
    }
    return None;
}
pub fn find_preset_in_soundfonts<'a>(soundfonts: &'a [&SoundFont2], bank: u16, program: u16) -> Option<(usize, usize)> {
    for (soundfont_i, soundfont) in soundfonts.iter().enumerate() {
        if let Some(preset_i) = find_preset_in_soundfont(soundfont, bank, program) {
            return Some((soundfont_i, preset_i));
        }
    }
    return None;
}

pub fn sample_rate_adjustment_in_cents(sample_rate: f64) -> f64 {
    ((sample_rate - 1115.9471180474397) / 31832.602532753794).ln() / 0.0005990154279493774
}

pub fn sample_rate_adjustment_ideal(sample_rate: f64) -> Tuning {
    Tuning::from_cents((1200.0 * (sample_rate / 32728.5).log2()).round() as i64)
}
pub fn sample_rate_adjustment_table(sample_rate: f64) -> Result<Tuning, DSEError> {
    let smplrate = sample_rate.round() as u32;
    if let Some(&cents) = BUILT_IN_SAMPLE_RATE_ADJUSTMENT_TABLE.get(&smplrate) {
        println!("{:?}", Tuning::from_cents(cents));
        Ok(Tuning::from_cents(cents))
    } else {
        Err(DSEError::SampleRateUnsupported(sample_rate))
    }
}
pub fn sample_rate_adjustment_fitted(sample_rate: f64) -> Tuning {
    Tuning::from_cents(sample_rate_adjustment_in_cents(sample_rate) as i64)
}
pub fn sample_rate_adjustment(sample_rate: f64, curve: usize, additional_adjust: i64) -> Result<Tuning, DSEError> {
    let mut val = match curve {
        1 => Ok(sample_rate_adjustment_ideal(sample_rate)),
        2 => sample_rate_adjustment_table(sample_rate),
        3 => Ok(sample_rate_adjustment_fitted(sample_rate)),
        _ => return Err(DSEError::Invalid("Invalid sample rate adjustment curve number!".to_string()))
    }?;
    val.add_cents(additional_adjust);
    Ok(val)
}

pub fn timecents_to_index(timecents: i16) -> (u8, i8) {
    let msec = timecents_to_milliseconds(timecents);
    if msec <= 0x7FFF {
        (1_u8, lookup_env_time_value_i16(msec as i16))
    } else {
        (0_u8, lookup_env_time_value_i32(msec))
    }
}

