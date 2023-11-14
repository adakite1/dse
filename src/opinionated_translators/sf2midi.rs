use std::{ops::RangeInclusive, collections::{HashMap, HashSet, BTreeMap}, io::{Read, Seek, Write}, rc::Rc, cell::RefCell};

use colored::Colorize;
use indexmap::IndexMap;
use midly::Smf;
use soundfont::SoundFont2;

use crate::{smdl::{SMDL, midi::{get_midi_tpb, get_midi_messages_flattened, TrkChunkWriter, copy_midi_messages, ProgramUsed}, create_smdl_shell, DSEEvent}, dtype::{DSEError, DSELinkBytes, PointerTable}, swdl::{SWDL, sf2::{DSPOptions, find_preset_in_soundfonts, copy_presets, find_gen_in_zones, copy_raw_sample_data}, SampleInfo, PRGIChunk, KGRPChunk, Keygroup}};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SampleEntry {
    soundfont_name: String,
    sample_i: u16
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstrumentMappingEntry {
    soundfont_name: String,
    preset_i: usize,
    preset_zone_i: usize
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PresetEntry {
    soundfont_name: String,
    preset_i: usize
}

fn check_vcrange_valid(vcrange: &RangeInclusive<i8>) -> Result<(), DSEError> {
    if *vcrange.start() < 0 || *vcrange.start() > 15 {
        Err(DSEError::DSEUsedVoiceChannelsRangeOutOfBounds(vcrange.clone()))
    } else if *vcrange.end() != -1 && (*vcrange.end() < 0 || *vcrange.end() > 15) {
        Err(DSEError::DSEUsedVoiceChannelsRangeOutOfBounds(vcrange.clone()))
    } else if *vcrange.end() != -1 && (*vcrange.start() > *vcrange.end()) {
        Err(DSEError::DSEUsedVoiceChannelsRangeFlipped(vcrange.clone()))
    } else {
        Ok(())
    }
}
fn vclow(vcrange: &RangeInclusive<i8>) -> Result<i8, DSEError> {
    check_vcrange_valid(vcrange)?;
    Ok(*vcrange.start())
}
fn vchigh(vcrange: &RangeInclusive<i8>) -> Result<i8, DSEError> {
    check_vcrange_valid(vcrange)?;
    if *vcrange.end() == -1 { Ok(15) } else { Ok(*vcrange.end()) }
}
// fn vcchans(vcrange: &RangeInclusive<i8>) -> Result<u8, DSEError> {
//     Ok((vchigh(vcrange)? + 1 - vclow(vcrange)?) as u8)
// }

pub trait FromMIDIOnce {
    /// Creates an SMD file from MIDI data. The "once" in the name indicates that multiple MIDI's cannot be put into a single SMD file.
    /// 
    /// Returns the generated Bank/Program to DSE program id mappings, and then three optionally `None` sets `samples_used`, `instrument_mappings_used`, and `presets_used`, each containing identifiers for used samples, instrument mappings, and presets respectively.
    /// 
    /// # Arguments
    /// * `smf` - MIDI data in `midly::Smf` form.
    /// * `last_modified` - Last modified date.
    /// * `name` - Song name.
    /// * `link_bytes` - DSE Link bytes.
    /// * `vcrange` - Voice channels to use, range must not exceed `[0, 15]`, although the end parameter can be `-1`, which will be interpreted as the maximum, which is `15`.
    /// * `soundfonts` - `HashMap` of all available soundfonts.
    /// * `uses` - Soundfonts used by song.
    fn from_midi_once(&mut self, smf: &Smf, last_modified: (u16, u8, u8, u8, u8, u8, u8), name: &str, link_bytes: (u8, u8), vcrange: RangeInclusive<i8>, soundfonts: &HashMap<String, SoundFont2>, uses: &[String]) -> Result<(HashMap<(u8, u8), u8>, Option<HashSet<SampleEntry>>, Option<HashSet<InstrumentMappingEntry>>, Option<HashSet<PresetEntry>>), DSEError>;
}
pub trait TrimmedSampleDataCopy {
    /// Copies raw sample data from a soundfont into the SWD, but trim any samples not present in `samples_used`.
    /// 
    /// Returns a `HashMap` of sample id mappings, as well as another `HashMap` with the `SampleInfo` of every sample.
    /// 
    /// # Arguments
    /// * `sf2name` - Soundfont name.
    /// * `sf2file` - Soundfont file reader, seeked to zero (reset cursor position if passing unclean reader).
    /// * `sf2` - Soundfont data in `soundfont::SoundFont2` form.
    /// * `dsp_options` - Internal audio processing options.
    /// * `sample_rate_adjustment_curve` - Sample-rate adjustment curve.
    ///     1 - Ideal sample correction for fixed 32728.5Hz hardware output rate
    ///     2 - Discrete lookup table based on the original EoS main bank (all samples must either match the `sample_rate` parameter *or* be converted to that sample rate in this mode!)
    ///     3 - Fitted curve
    /// * `pitch_adjust` - Soft global pitch adjust (adjustments are made through `ftune` and `ctune` parameters within DSE instead of done directly on the samples).
    /// * `samples_used` - Samples to copy. If writing to the main bank, this should contain samples used across all songs. If writing to decoupled song banks, this should only contain samples used in that song. Since each entry contains an identifier to the origin Soundfont, it does not need to be trimmed to only contain samples within the Soundfont currently being processed.
    fn trimmed_raw_sample_copy<R: Read + Seek>(&mut self, sf2name: &str, sf2file: R, sf2: &SoundFont2, dsp_options: DSPOptions, sample_rate_adjustment_curve: usize, pitch_adjust: i64, samples_used: &HashSet<SampleEntry>) -> Result<(HashMap<u16, u16>, BTreeMap<u16, SampleInfo>), DSEError>;
}
pub trait FromSF2Once {
    /// Creates an SWD file from Soundfont presets **without copying any raw sample data.** The "once" in the name indicates that this should not be called a second time on the same SWD, as data written on the first run will be overwritten. This is meant specifically to create the song SWD files paired with each SMD.
    /// 
    /// Returns the generated `SWDL` structure.
    /// 
    /// # Arguments
    /// * `soundfonts` - `HashMap` of all available soundfonts.
    /// * `uses` - Soundfonts used by song.
    /// * `last_modified` - Last modified date.
    /// * `name` - Song name.
    /// * `link_bytes` - DSE Link bytes.
    /// * `vcrange` - Voice channels to use, range must not exceed `[0, 15]`, although the end parameter can be `-1`, which will be interpreted as the maximum, which is `15`.
    /// * `sample_rate_adjustment_curve` - Sample-rate adjustment curve.
    ///     1 - Ideal sample correction for fixed 32728.5Hz hardware output rate
    ///     2 - Discrete lookup table based on the original EoS main bank (all samples must either match the `sample_rate` parameter *or* be converted to that sample rate in this mode!)
    ///     3 - Fitted curve
    /// * `pitch_adjust` - Soft global pitch adjust (adjustments are made through `ftune` and `ctune` parameters within DSE instead of done directly on the samples).
    /// * `song_preset_map` - Bank/Program to DSE program id mappings. If `FromMIDIOnce::from_midi_once` was previously run to convert a MIDI, it would have created mappings based on all the presets used in the MIDI, which you should pass here so that the SWD file will have the corresponding Soundfont presets mapped to DSE in the same way.
    /// * `sample_mapping_information` - Soundfont Sample Indices to DSE sample id mappings for each soundfont. If `TrimmedSampleDataCopy::trimmed_raw_sample_copy` was previously run to copy samples from the same SF2's, it should have created custom mappings so as not to overwrite any existing sample data, which you should pass here so that the SWD file will reference the correct samples.
    /// * `instrument_mappings_used` - Instrument mappings to copy. This should only contain instrument mappings used in this song. Since each entry contains an identifier to the origin Soundfont, instrument mappings from various Soundfonts can be mixed in this list.
    /// * `samples_used` - Samples used for this song. Used for building the virtual `wavi` chunk present in all track SWD's pointing to samples in the main bank or the file itself if decoupled songs are being generated. It's different from the identically named parameter in `TrimmedSampleDataCopy::trimmed_raw_sample_copy` in that this should only contain samples used within this song, no matter what.
    fn from_sf2_once(&mut self, soundfonts: &HashMap<String, SoundFont2>, uses: &[String], last_modified: (u16, u8, u8, u8, u8, u8, u8), name: &str, link_bytes: (u8, u8), vcrange: RangeInclusive<i8>,
        sample_rate_adjustment_curve: usize, pitch_adjust: i64,
        song_preset_map: &HashMap<(u8, u8), u8>, sample_mapping_information: &HashMap<String, (HashMap<u16, u16>, BTreeMap<u16, SampleInfo>)>,
        instrument_mappings_used: &HashSet<InstrumentMappingEntry>, samples_used: &HashSet<SampleEntry>) -> Result<(), DSEError>;
}

impl FromSF2Once for SWDL {
    fn from_sf2_once(&mut self, soundfonts: &HashMap<String, SoundFont2>, uses: &[String], last_modified: (u16, u8, u8, u8, u8, u8, u8), name: &str, link_bytes: (u8, u8), vcrange: RangeInclusive<i8>,
            sample_rate_adjustment_curve: usize, pitch_adjust: i64,
            song_preset_map: &HashMap<(u8, u8), u8>, sample_mapping_information: &HashMap<String, (HashMap<u16, u16>, BTreeMap<u16, SampleInfo>)>,
            instrument_mappings_used: &HashSet<InstrumentMappingEntry>, samples_used: &HashSet<SampleEntry>) -> Result<(), DSEError> {
        // Set headers
        self.set_metadata(last_modified, format!("{}.SWD", name))?;
        self.set_link_bytes(link_bytes);

        // Get the soundfonts used by the track
        let track_soundfonts = uses.iter().map(|soundfont_name| soundfonts.get(soundfont_name).ok_or(DSEError::Invalid(format!("Soundfont with name '{}' not found!", soundfont_name)))).collect::<Result<Vec<&SoundFont2>, _>>()?;

        // Copy over the necessary presets from the used soundfonts
        let mut prgi = PRGIChunk::new(0);
        let mut sample_infos_merged = BTreeMap::new();
        for (soundfont_name, &sf2) in uses.iter().zip(track_soundfonts.iter()) {
            if let Some((sample_mappings, sample_infos)) = sample_mapping_information.get(soundfont_name) {
                let mut sample_infos = sample_infos.clone();
                copy_presets(
                    sf2,
                    &mut sample_infos,
                    &mut prgi.data,
                    |i| sample_mappings.get(&i).copied(),
                    sample_rate_adjustment_curve,
                    pitch_adjust,
                    |preset_i, _, _, preset_zone_i, _, _, _| instrument_mappings_used.get(&InstrumentMappingEntry { soundfont_name: soundfont_name.clone(), preset_i, preset_zone_i }).is_some(),
                    |_, preset, program_info| {
                        //TODO: An sf2 exported from VGMTrans had an extra empty preset after all the normal ones visible in Polyphone with a bank/preset number of 000:000, which broke the assertion that each id should correspond to one preset. The likely explanation is that empty presets are meant to be ignored, and so we do that here.
                        if program_info.splits_table.len() > 0 {
                            song_preset_map.get(&(preset.header.bank as u8, preset.header.preset as u8)).map(|x| *x as u16)   
                        } else {
                            None
                        }
                    });
                let sample_infos_trimmed: BTreeMap<u16, SampleInfo> = samples_used.iter().filter_map(|x| {
                    if let Some(mapping) = sample_mappings.get(&x.sample_i) {
                        Some((x.sample_i, sample_infos.get(mapping).ok_or(DSEError::_SampleInPresetMissing(*mapping)).unwrap().clone()))
                    } else {
                        // The ones that are filtered out are not in this specific soundfont
                        None
                    }
                }).collect::<BTreeMap<u16, SampleInfo>>();
                sample_infos_merged.extend(sample_infos_trimmed);
            } else {
                println!("{}Soundfont '{}' is never used! Writing will be skipped.", "Warning: ".yellow(), soundfont_name);
            }
        }
        self.prgi = Some(prgi);

        // Add the sample info objects last
        self.wavi.data.objects = sample_infos_merged.into_values().collect();
        // Fix the smplpos
        let mut pos_in_memory = 0;
        for obj in &mut self.wavi.data.objects {
            obj.smplpos = pos_in_memory;
            pos_in_memory += (obj.loopbeg + obj.looplen) * 4;
        }

        // Keygroups
        let mut kgrp = KGRPChunk::default();
        kgrp.data.objects = vec![
            Keygroup { id: 0, poly: -1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 1, poly: 2, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 2, poly: 1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 3, poly: 1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 4, poly: 1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 5, poly: 1, priority: 1, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 6, poly: 2, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 7, poly: 1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 8, poly: 2, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 9, poly: -1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 10, poly: -1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
            Keygroup { id: 11, poly: -1, priority: 8, vclow: vclow(&vcrange)?, vchigh: vchigh(&vcrange)?, unk50: 0, unk51: 0 },
        ]; // Just a quick template keygroup list. By default only the first kgrp is used!
        self.kgrp = Some(kgrp);

        Ok(())
    }
}

impl TrimmedSampleDataCopy for SWDL {
    fn trimmed_raw_sample_copy<R: Read + Seek>(&mut self, sf2name: &str, sf2file: R, sf2: &SoundFont2, dsp_options: DSPOptions, sample_rate_adjustment_curve: usize, pitch_adjust: i64, samples_used: &HashSet<SampleEntry>) -> Result<(HashMap<u16, u16>, BTreeMap<u16, SampleInfo>), DSEError> {
        Ok(copy_raw_sample_data(
            sf2file,
            sf2,
            self,
            dsp_options,
            sample_rate_adjustment_curve,
            pitch_adjust,
            |sample_i, _| samples_used.contains(&SampleEntry { soundfont_name: sf2name.to_string(), sample_i: sample_i as u16 }))?)
    }
}

impl FromMIDIOnce for SMDL {
    fn from_midi_once(&mut self, smf: &Smf, last_modified: (u16, u8, u8, u8, u8, u8, u8), name: &str, link_bytes: (u8, u8), vcrange: RangeInclusive<i8>, soundfonts: &HashMap<String, SoundFont2>, uses: &[String]) -> Result<(HashMap<(u8, u8), u8>, Option<HashSet<SampleEntry>>, Option<HashSet<InstrumentMappingEntry>>, Option<HashSet<PresetEntry>>), DSEError> {
        let tpb = get_midi_tpb(&smf)?;

        self.set_metadata(last_modified, format!("{}.SMD", name))?;
        self.set_link_bytes(link_bytes);
        self.song.tpqn = tpb;

        let midi_messages = get_midi_messages_flattened(&smf)?;

        let midi_channel_contains_midi_bank_select_or_program_changes = |target_channel: u8| midi_messages.iter().any(|x| match x.kind {
            midly::TrackEventKind::Midi { channel, message } => {
                if (channel.as_int() + 1) == target_channel {
                    match message {
                        midly::MidiMessage::Controller { controller, value: _ } => {
                            controller.as_int() == 00 // CC00 Bank Select MSB
                        },
                        midly::MidiMessage::ProgramChange { program: _ } => true,
                        _ => false,
                    }
                } else {
                    false
                }
            },
            _ => false
        });

        // Copy midi messages
        let mut programs_requiring_mapping: IndexMap<u8, Vec<(Rc<RefCell<DSEEvent>>, (u8, u8))>> = IndexMap::new();
        let mut map_program = |trkid, bank, program, same_tick, trk_chunk_writer: &mut TrkChunkWriter, newly_created_event: Rc<RefCell<DSEEvent>>| {
            if same_tick {
                // Discard last. If same_tick is true, then a previous preset change has already been recorded, so this is guaranteed to remove the correct entry.
                programs_requiring_mapping.entry(trkid).or_insert(Vec::new()).pop();
            }
            programs_requiring_mapping.entry(trkid).or_insert(Vec::new()).push((newly_created_event, (bank, program)));
            // Insert the event for now, fixing it later to be the correct value
            Some(0)
        };
        // Vec of TrkChunkWriter's
        let mut trks: Vec<TrkChunkWriter> = vec![
            // Meta events track
            TrkChunkWriter::create(0, 0, self.get_link_bytes()).unwrap()
        ];
        trks.extend((0..=15).enumerate().map(|(trkid, chanid)| {
            let mut trk = TrkChunkWriter::create(trkid as u8 + 1, chanid as u8, self.get_link_bytes()).unwrap();
            // Most soundfont players default to preset 000:000 if no MIDI Bank Select and Program Change messages are found. This matches that behavior.
            // There's also a special case for Channel 10, a channel reserved for drums in MIDI GM and thus has a default preset of 128:000.
            if (trkid+1) == 10 && !midi_channel_contains_midi_bank_select_or_program_changes(10) {
                let _ = trk.bank_select(128, true, &mut map_program); // The results can be ignored since the only failure condition is if the DSE opcode "SetProgram" could not be found, which would be very bad if that happened and this wouldn't be able to recover anyways.
                let _ = trk.program_change(0, true, &mut map_program);
            } else {
                let _ = trk.bank_select(0, true, &mut map_program); // The results can be ignored since the only failure condition is if the DSE opcode "SetProgram" could not be found, which would be very bad if that happened and this wouldn't be able to recover anyways.
                let _ = trk.program_change(0, true, &mut map_program);
            }
            trk
        }));
        let _ = copy_midi_messages(midi_messages, &mut trks, &mut map_program)?;
        let mut song_preset_map: HashMap<(u8, u8), u8> = HashMap::new();
        let mut current_id = 0_u8;
        for (trkid, programs_requiring_mapping) in programs_requiring_mapping.into_iter() {
            for (event, (bank, program)) in programs_requiring_mapping {
                println!("trk{:02} bank{} prgm{}", trkid, bank, program);
                let program_id;
                if let Some(&existing_program_id) = song_preset_map.get(&(bank, program)) {
                    program_id = existing_program_id;
                } else {
                    // Assign new
                    let assigned_id = current_id;
                    current_id += 1;
                    song_preset_map.insert((bank, program), assigned_id);
                    program_id = assigned_id;
                }
                if let DSEEvent::Other(evt) = &mut *event.borrow_mut() {
                    (&mut evt.parameters[..]).write_all(&[program_id])?;
                } else {
                    panic!("{}TrkChunkWriter has passed an invalid event as the program change event!", "Internal Error: ".red());
                }
            }
        }

        let mut samples_used: Option<HashSet<SampleEntry>> = None;
        let mut instrument_mappings_used: Option<HashSet<InstrumentMappingEntry>> = None;
        let mut presets_used: Option<HashSet<PresetEntry>> = None;

        // Fill the tracks into the smdl
        let track_soundfonts = uses.iter().map(|soundfont_name| soundfonts.get(soundfont_name).ok_or(DSEError::Invalid(format!("Soundfont with name '{}' not found!", soundfont_name)))).collect::<Result<Vec<&SoundFont2>, _>>()?;
        self.trks.objects = Vec::with_capacity(trks.len());
        for x in trks.into_iter() {
            for ProgramUsed { bank, program, notes, is_default } in x.programs_used() {
                let find_preset = find_preset_in_soundfonts(&track_soundfonts, *bank as u16, *program as u16);
                if find_preset.is_none() && *is_default {
                    println!("{}None of the following soundfonts {:?} used by a track contain a default 000:000 piano preset! Any MIDI tracks lacking MIDI Bank Select and Program Change messages will cause the tool to fail!", "Warning: ".yellow(), uses);
                    continue;
                }
                let (soundfont_i, preset_i) = find_preset.ok_or(DSEError::Invalid(format!("Preset {:03}:{:03} not found in any of the specified soundfonts for song '{}'!", bank, program, name)))?;
                let sf2 = soundfonts.get(&uses[soundfont_i]).ok_or(DSEError::Invalid(format!("Soundfont with name '{}' not found!", &uses[soundfont_i])))?;
                presets_used.get_or_insert(HashSet::new())
                    .insert(PresetEntry { soundfont_name: uses[soundfont_i].clone(), preset_i });

                let mut dummy_prgi = PointerTable::new(0, 0);
                copy_presets(sf2, &mut (0..sf2.sample_headers.len()).into_iter().map(|i| {
                    let mut dummy_smpl = SampleInfo::default();
                    dummy_smpl.smplrate = 44100;
                    (i as u16, dummy_smpl)
                }).collect::<BTreeMap<u16, SampleInfo>>(), &mut dummy_prgi, |x| Some(x), 1, 0, |preset_i, preset, global_preset_zone, preset_zone_i, preset_zone, _, _| {
                    // When this is called, the instrument is guaranteed to not be a global instrument
                    let mut preset_zones_to_search = vec![preset_zone];
                    if let Some(global_preset_zone) = global_preset_zone {
                        preset_zones_to_search.push(global_preset_zone);
                    }
                    // By default, keep the instrument
                    let mut keep = preset.header.bank == *bank as u16 && preset.header.preset == *program as u16;
                    let key_range;
                    let vel_range;
                    // Check the instrument's key range, if it is specified
                    if let Some(gen) = find_gen_in_zones(&preset_zones_to_search, soundfont::data::GeneratorType::KeyRange) {
                        let key_range_value = gen.amount.as_range().unwrap();
                        let lowkey = key_range_value.low as i8;
                        let hikey = key_range_value.high as i8;
                        key_range = Some(lowkey as u8..=hikey as u8);
                    } else {
                        key_range = None;
                    }
                    // Check the instrument's velocity range, if it is specified
                    if let Some(gen) = find_gen_in_zones(&preset_zones_to_search, soundfont::data::GeneratorType::VelRange) {
                        let vel_range_value = gen.amount.as_range().unwrap();
                        let lovel = vel_range_value.low as i8;
                        let hivel = vel_range_value.high as i8;
                        vel_range = Some(lovel as u8..=hivel as u8);
                    } else {
                        vel_range = None;
                    }
                    // Check for all possibilities of the two ranges existing
                    if let (Some(key_range), Some(vel_range)) = (&key_range, &vel_range) {
                        keep = keep && notes.iter().any(|(key, vels)| key_range.contains(key) && vels.iter().any(|vel| vel_range.contains(vel)));
                    } else if let Some(key_range) = &key_range {
                        keep = keep && notes.iter().any(|(key, _)| key_range.contains(key));
                    } else if let Some(vel_range) = &vel_range {
                        keep = keep && notes.iter().any(|(_, vels)| vels.iter().any(|vel| vel_range.contains(vel)));
                    }
                    // Make a record of if this instrument is used or not (only the index can be saved, and so a second step is necessary to actually turn these indices into references, which is done outside of this closure)
                    if keep {
                        instrument_mappings_used.get_or_insert(HashSet::new())
                            .insert(InstrumentMappingEntry { soundfont_name: uses[soundfont_i].clone(), preset_i, preset_zone_i });
                    }
                    keep
                }, |_, preset, _| {
                    if preset.header.bank == *bank as u16 && preset.header.preset == *program as u16 {
                        Some(0)
                    } else {
                        None
                    }
                });
                //TODO: An sf2 exported from VGMTrans had an extra empty preset after all the normal ones visible in Polyphone with a bank/preset number of 000:000, which broke the assertion that each id should correspond to one preset. The likely explanation is that empty presets are meant to be ignored, and so we do that here.
                dummy_prgi.objects.retain(|x| {
                    x.splits_table.len() > 0
                });
                assert!(dummy_prgi.objects.len() <= 1); //TODO: Low priority, but replace this with an actual error. This should never happen.
                for program in dummy_prgi.objects {
                    for split in program.splits_table.objects {
                        let key_range = split.lowkey as u8..=split.hikey as u8;
                        let vel_range = split.lovel as u8..=split.hivel as u8;
                        if notes.iter().any(|(key, vels)| key_range.contains(key) && vels.iter().any(|vel| vel_range.contains(vel))) {
                            samples_used.get_or_insert(HashSet::new())
                                .insert(SampleEntry { soundfont_name: uses[soundfont_i].clone(), sample_i: split.SmplID });
                        }
                    }
                }
            }
            self.trks.objects.push(x.close_track());
        }

        // Regenerate read markers for the SMDL
        self.regenerate_read_markers()?;

        Ok((song_preset_map, samples_used, instrument_mappings_used, presets_used))
    }
}

