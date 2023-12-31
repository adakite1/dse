use std::{borrow::Cow, collections::{HashMap, BTreeSet, BTreeMap}, u8, hash::Hash, io::Write, rc::Rc, cell::RefCell};

use byteorder::{WriteBytesExt, LittleEndian, BigEndian, ByteOrder};
use colored::Colorize;
use midly::{Smf, TrackEvent, num::{u4, u28, u24}};

use crate::dtype::DSEError;

use super::{TrkChunk, events::{PlayNote, Other, FixedDurationPause}, DSEEvent};

// Open input MIDI file
pub fn open_midi<'a>(smf_source: &'a Vec<u8>) -> Result<Smf<'a>, DSEError> {
    Smf::parse(&smf_source).map_err(|x| DSEError::SmfParseError(x.to_string()))
}
pub fn get_midi_tpb(smf: &Smf) -> Result<u16, DSEError> {
    match smf.header.timing {
        midly::Timing::Metrical(tpb) => Ok(tpb.as_int()),
        _ => Err(DSEError::DSESmfUnsupportedTimingSpecifier())
    }
}

pub fn get_midi_messages_flattened<'a>(smf: &'a Smf) -> Result<Cow<'a, [TrackEvent<'a>]>, DSEError> {
    let midi_messages_combined: Vec<TrackEvent>;
    match smf.header.format {
        midly::Format::SingleTrack => { Ok(Cow::from(&smf.tracks[0])) },
        midly::Format::Parallel => {
            println!("{}SMF1-type MIDI file detected! All MIDI tracks contained within will be mapped to MIDI channels and converted to SMF0!", "Warning: ".yellow());
            println!("{}This converter assumes that the first MIDI track encountered is dedicated solely for Meta events to follow convention.", "Warning: ".yellow());
            let mut first_track_is_meta: bool = true;
            for midi_msg in &smf.tracks[0] {
                match midi_msg.kind {
                    midly::TrackEventKind::Midi { channel: _, message: _ } => {
                        // Track does not follow convention!
                        println!("{}SMF1 multi-track MIDI file contains note events in the first track! The first track is usually reserved only for meta events. It will be assumed that this MIDI file does not follow that convention.", "Warning: ".yellow());
                        first_track_is_meta = false;
                        break;
                    },
                    _ => {  }
                }
            }
            let mut midi_messages_tmp: Vec<(u128, TrackEvent)> = Vec::new();
            for (i, track) in smf.tracks.iter().enumerate() {
                let mut global_tick = 0;
                for midi_msg in track {
                    global_tick += midi_msg.delta.as_int() as u128;
                    // Overwrite MIDI message channel data to match track number!
                    let mut midi_msg_edited = midi_msg.clone();
                    if let midly::TrackEventKind::Midi { channel, message: _ } = &mut midi_msg_edited.kind {
                        let mapped_channel = if first_track_is_meta { i - 1 } else { i };
                        *channel = u4::try_from(u8::try_from(mapped_channel).map_err(|_| DSEError::DSESmf0TooManyTracks())?).ok_or(DSEError::DSESmf0TooManyTracks())?;
                    }
                    // Search to see where to insert the event
                    let insert_position = midi_messages_tmp.binary_search_by_key(&global_tick, |&(k, _)| k);
                    midi_messages_tmp.insert(match insert_position {
                        Ok(index) => index,
                        Err(index) => index
                    }, (global_tick, midi_msg_edited));
                }
            }
            for i in 0..midi_messages_tmp.len() {
                let mut new_delta = 0;
                if i != 0 {
                    let (previous_global_tick, _) = &midi_messages_tmp[i - 1];
                    let (current_global_tick, _) = &midi_messages_tmp[i];
                    new_delta = current_global_tick - previous_global_tick;
                }
                midi_messages_tmp[i].1.delta = u28::try_from(u32::try_from(new_delta).map_err(|_| DSEError::DSESmf0MessagesTooFarApart())?).ok_or(DSEError::DSESmf0MessagesTooFarApart())?;
            }
            midi_messages_combined = midi_messages_tmp.into_iter().map(|(_, evt)| evt).collect();
            Ok(Cow::from(midi_messages_combined))
        },
        _ => {
            return Err(DSEError::DSESequencialSmfUnsupported());
        },
    }
}

pub fn copy_midi_messages<'a, MapProgram>(midi_messages: Cow<'a, [TrackEvent<'a>]>, trks: &mut [TrkChunkWriter], mut map_program: MapProgram) -> Result<u128, DSEError>
where
    MapProgram: FnMut(u8, u8, u8, bool, &mut TrkChunkWriter, Rc<RefCell<DSEEvent>>) -> Option<u8> {
    // Loop through all the events
    let mut global_tick = 0;
    for midi_msg in midi_messages.as_ref() {
        let delta = midi_msg.delta.as_int() as u128;
        global_tick += delta;

        match midi_msg.kind {
            midly::TrackEventKind::Midi { channel, message } => {
                let channel_i = channel.as_int() as usize + 1;

                match message {
                    midly::MidiMessage::NoteOn { key, vel } => {
                        trks[channel_i].fix_current_global_tick(global_tick)?;
                        if vel == 0 {
                            trks[channel_i].note_off(key.as_int())?
                        } else {
                            trks[channel_i].note_on(key.as_int(), vel.as_int())?
                        }
                    },
                    midly::MidiMessage::NoteOff { key, vel: _ } => {
                        trks[channel_i].fix_current_global_tick(global_tick)?;
                        trks[channel_i].note_off(key.as_int())?
                    },
                    midly::MidiMessage::Aftertouch { key, vel } => { /* Ignore aftertouch events */ },
                    midly::MidiMessage::Controller { controller, value } => {
                        trks[channel_i].fix_current_global_tick(global_tick)?;
                        match controller.as_int() {
                            00 => { // CC00 Bank Select MSB
                                println!("{}", "Processing bank select message.".green());
                                trks[channel_i].bank_select(value.as_int(), false, &mut map_program)?;
                            },
                            07 => { // CC07 Volume MSB
                                trks[channel_i].add_other_with_params_u8("SetTrackVolume", value.as_int())?;
                            },
                            10 => { // CC10 Pan Position MSB
                                trks[channel_i].add_other_with_params_u8("SetTrackPan", value.as_int())?;
                            },
                            11 => { // CC11 Expression MSB
                                trks[channel_i].add_other_with_params_u8("SetTrackExpression", value.as_int())?;
                            },
                            _ => { /* Ignore the other controllers for now */ }
                        }
                    },
                    midly::MidiMessage::ProgramChange { program } => {
                        trks[channel_i].fix_current_global_tick(global_tick)?;
                        println!("{}", "Processing program change message.".green());
                        trks[channel_i].program_change(program.as_int(), false, &mut map_program)?;
                    },
                    midly::MidiMessage::ChannelAftertouch { vel } => { /* Ignore channel aftertouch events */ },
                    midly::MidiMessage::PitchBend { bend } => {
                        trks[channel_i].fix_current_global_tick(global_tick)?;
                        trks[channel_i].add_other_with_params_i16::<BigEndian>("PitchBend", bend.as_int())?;
                    },
                }
            },
            midly::TrackEventKind::SysEx(_) => { /* Ignore sysex events */ },
            midly::TrackEventKind::Escape(_) => { /* Ignore escape events */ },
            midly::TrackEventKind::Meta(meta) => {
                match meta {
                    midly::MetaMessage::TrackNumber(_) => { /* Ignore */ },
                    midly::MetaMessage::Text(_) => { /* Ignore */ },
                    midly::MetaMessage::Copyright(_) => { /* Ignore */ },
                    midly::MetaMessage::TrackName(_) => { /* Ignore */ },
                    midly::MetaMessage::InstrumentName(_) => { /* Ignore */ },
                    midly::MetaMessage::Lyric(_) => { /* Ignore */ },
                    midly::MetaMessage::Marker(marker) => {
                        if let Ok(marker) = String::from_utf8(marker.into()) {
                            if marker.trim().to_lowercase() == "loopstart" {
                                for trk in trks.iter_mut() {
                                    trk.fix_current_global_tick(global_tick)?;
                                    trk.add_other_no_params("LoopPoint")?;
                                }
                            } else if marker.trim().to_lowercase() == "loopend" {
                                for trk in trks.iter_mut() {
                                    trk.fix_current_global_tick(global_tick)?;
                                    for val in 0..u8::MAX { // Reset synth
                                        trk.note_off(val)?;
                                    }
                                }
                                break;
                            } else if marker.trim().to_lowercase() == "loopendnoreset" {
                                break;
                            } else if marker.trim().to_lowercase().starts_with("signal") {
                                let cmd = marker.trim().to_lowercase();
                                let signal_val: u8 = cmd[6..].replace("(", "").replace(")", "").trim().parse::<u8>().map_err(|_| DSEError::Invalid("MIDI Marker 'Signal(n)' must have a uint8 as its parameter!".to_string()))?;
                                trks[0].fix_current_global_tick(global_tick)?;
                                trks[0].add_other_with_params_u8("Signal", signal_val)?;
                            } else if marker.trim().starts_with("dsec") {
                                let mut track_i = 0;
                                for cmd in marker.trim()[4..].trim_start().split(";") {
                                    let cmd = cmd.trim();

                                    println!("{}", cmd.green());

                                    if cmd.starts_with("trk") {
                                        let new_track_n = cmd[3..].trim_start().parse::<usize>()
                                            .map_err(|_| DSEError::InvalidDSECommandFailedToParseTrkChange(cmd.to_string()))?;
                                        track_i = new_track_n;
                                        continue;
                                    } else if cmd.starts_with("evttrk") {
                                        track_i = 0;
                                        continue;
                                    }

                                    let name;
                                    let mut arguments_bytes: Vec<u8> = Vec::new();

                                    if let Some(left_paren_index) = cmd.chars().position(|c| c == '(') {
                                        name = cmd[..left_paren_index].trim_end();

                                        // Parse arguments
                                        let mut arguments_str = cmd[(left_paren_index+1)..].trim_start();
                                        if arguments_str.len() == 0 {
                                            return Err(DSEError::InvalidDSECommand(cmd.to_string(), "Opening parentheses must be closed!!".to_string()));
                                        } else {
                                            if arguments_str.chars().last().unwrap() == ')' {
                                                arguments_str = arguments_str[..(arguments_str.len()-1)].trim_end();
                                            } else {
                                                return Err(DSEError::InvalidDSECommand(cmd.to_string(), "Opening parentheses must be closed!!".to_string()));
                                            }
                                        }
                                        for arg in arguments_str.split(",").map(|x| x.trim().to_lowercase()) {
                                            let mut added_argument_bytes: Vec<u8> = Vec::new();

                                            let typed: Vec<&str> = arg.split("_").map(|x| x.trim()).collect();

                                            if arg == "" {
                                                // Skip
                                            }

                                            else if typed.len() == 2 {
                                                // Typed
                                                match typed[1] {
                                                    "i8" => added_argument_bytes.write_i8(
                                                        typed[0].parse::<i8>()
                                                            .map_or_else(|_| i8::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u8" => added_argument_bytes.write_u8(
                                                        typed[0].parse::<u8>()
                                                            .map_or_else(|_| u8::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),

                                                    "i16le" => added_argument_bytes.write_i16::<LittleEndian>(
                                                        typed[0].parse::<i16>()
                                                            .map_or_else(|_| i16::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u16le" => added_argument_bytes.write_u16::<LittleEndian>(
                                                        typed[0].parse::<u16>()
                                                            .map_or_else(|_| u16::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i32le" => added_argument_bytes.write_i32::<LittleEndian>(
                                                        typed[0].parse::<i32>()
                                                            .map_or_else(|_| i32::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u32le" => added_argument_bytes.write_u32::<LittleEndian>(
                                                        typed[0].parse::<u32>()
                                                            .map_or_else(|_| u32::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i64le" => added_argument_bytes.write_i64::<LittleEndian>(
                                                        typed[0].parse::<i64>()
                                                            .map_or_else(|_| i64::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u64le" => added_argument_bytes.write_u64::<LittleEndian>(
                                                        typed[0].parse::<u64>()
                                                            .map_or_else(|_| u64::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i128le" => added_argument_bytes.write_i128::<LittleEndian>(
                                                        typed[0].parse::<i128>()
                                                            .map_or_else(|_| i128::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u128le" => added_argument_bytes.write_u128::<LittleEndian>(
                                                        typed[0].parse::<u128>()
                                                            .map_or_else(|_| u128::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),

                                                    "i16be" => added_argument_bytes.write_i16::<BigEndian>(
                                                        typed[0].parse::<i16>()
                                                            .map_or_else(|_| i16::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u16be" => added_argument_bytes.write_u16::<BigEndian>(
                                                        typed[0].parse::<u16>()
                                                            .map_or_else(|_| u16::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i32be" => added_argument_bytes.write_i32::<BigEndian>(
                                                        typed[0].parse::<i32>()
                                                            .map_or_else(|_| i32::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u32be" => added_argument_bytes.write_u32::<BigEndian>(
                                                        typed[0].parse::<u32>()
                                                            .map_or_else(|_| u32::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i64be" => added_argument_bytes.write_i64::<BigEndian>(
                                                        typed[0].parse::<i64>()
                                                            .map_or_else(|_| i64::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u64be" => added_argument_bytes.write_u64::<BigEndian>(
                                                        typed[0].parse::<u64>()
                                                            .map_or_else(|_| u64::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "i128be" => added_argument_bytes.write_i128::<BigEndian>(
                                                        typed[0].parse::<i128>()
                                                            .map_or_else(|_| i128::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),
                                                    "u128be" => added_argument_bytes.write_u128::<BigEndian>(
                                                        typed[0].parse::<u128>()
                                                            .map_or_else(|_| u128::from_str_radix(&typed[0].trim_start_matches("0x"), 16), |x| Ok(x))
                                                            .map_err(|_| DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))?
                                                    ),

                                                    _ => {
                                                        return Err(DSEError::InvalidDSECommandTypedArgument(cmd.to_string(), arg.to_string(), typed[1].to_string()))
                                                    }
                                                }?;
                                            }
                                            
                                            else if let Ok(val) = arg.parse::<i8>() {
                                                added_argument_bytes.write_i8(val)?;
                                            } else if let Ok(val) = i8::from_str_radix(&arg.trim_start_matches("0x"), 16) {
                                                added_argument_bytes.write_i8(val)?;
                                            }
                                            
                                            else if let Ok(val) = arg.parse::<u8>() {
                                                added_argument_bytes.write_u8(val)?;
                                            } else if let Ok(val) = u8::from_str_radix(&arg.trim_start_matches("0x"), 16) {
                                                added_argument_bytes.write_u8(val)?;
                                            }

                                            else {
                                                return Err(DSEError::InvalidDSECommand(cmd.to_string(), format!("Value '{}' could not be parsed!", arg)));
                                            }

                                            arguments_bytes.extend(added_argument_bytes);
                                        }
                                    } else {
                                        name = cmd;
                                    }

                                    let mut evt = Other::default();
                                    evt.code = Other::name_to_code(name)?;

                                    // Check if the appropriate number of arguments were passed
                                    let (canonical_name, (_, _, num_bytes_taken)) = Other::lookup(evt.code)?;
                                    if arguments_bytes.len() != *num_bytes_taken as usize {
                                        return Err(DSEError::InvalidDSECommandArguments(cmd.to_string(), arguments_bytes.len(), canonical_name.to_string(), *num_bytes_taken as usize))
                                    }

                                    (&mut evt.parameters[..]).write_all(&arguments_bytes)?;
                                    trks[track_i].fix_current_global_tick(global_tick)?;
                                    trks[track_i].add_other_event(evt);
                                }
                            }
                        }
                    },
                    midly::MetaMessage::CuePoint(_) => { /* Ignore */ },
                    midly::MetaMessage::ProgramName(_) => { /* Ignore */ },
                    midly::MetaMessage::DeviceName(_) => { /* Ignore */ },
                    midly::MetaMessage::MidiChannel(_) => { /* Ignore */ },
                    midly::MetaMessage::MidiPort(_) => { /* Ignore */ },
                    midly::MetaMessage::EndOfTrack => { /* Ignore */ },
                    midly::MetaMessage::Tempo(microspb) => {
                        trks[0].fix_current_global_tick(global_tick)?;
                        trks[0].add_other_with_params_u8("SetTempo", (6e7 / microspb.as_int() as f64).round() as u8)?;
                    },
                    midly::MetaMessage::SmpteOffset(_) => { /* Ignore */ },
                    midly::MetaMessage::TimeSignature(_, _, _, _) => { /* Ignore */ },
                    midly::MetaMessage::KeySignature(_, _) => { /* Ignore */ },
                    midly::MetaMessage::SequencerSpecific(_) => { /* Ignore */ },
                    midly::MetaMessage::Unknown(_, _) => { /* Ignore */ },
                }
            },
        }
    }
    for trk in trks {
        trk.fix_current_global_tick(global_tick)?;
    }
    Ok(global_tick)
}

#[derive(Debug)]
pub struct ProgramUsed {
    pub bank: u8,
    pub program: u8,
    pub is_default: bool,
    pub notes: BTreeMap<u8, BTreeSet<u8>>
}
impl ProgramUsed {
    pub fn new(bank: u8, program: u8, is_default: bool) -> ProgramUsed {
        ProgramUsed { bank, program, is_default, notes: BTreeMap::new() }
    }
    pub fn from_dse(id: u8, is_default: bool) -> ProgramUsed {
        ProgramUsed::new(id / 128, id % 128, is_default)
    }
    pub fn to_dse(&self) -> u8 {
        self.bank * 128 + self.program
    }
    pub fn is_default(&self) -> bool {
        self.is_default
    }
}
impl PartialEq for ProgramUsed {
    fn eq(&self, other: &Self) -> bool {
        self.bank == other.bank && self.program == other.program
    }
}
impl Eq for ProgramUsed {  }
impl Hash for ProgramUsed {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bank.hash(state);
        self.program.hash(state);
    }
}
pub struct TrkChunkWriter {
    trkid: u8,
    chanid: u8,
    current_global_tick: u128,
    trk_events: Vec<Rc<RefCell<DSEEvent>>>,
    notes_held: HashMap<u8, (Rc<RefCell<DSEEvent>>, u128)>,
    bank: u8,
    program: u8,
    programs_used: Vec<ProgramUsed>,
    last_program_change_global_tick: Option<u128>,
    last_program_change_event_index: Option<usize>
}
impl TrkChunkWriter {
    pub fn create(trkid: u8, chanid: u8, link_bytes: (u8, u8)) -> Result<TrkChunkWriter, DSEError> {
        let mut trk_chunk_writer = TrkChunkWriter { trkid, chanid, current_global_tick: 0, trk_events: Vec::new(), notes_held: HashMap::new(), bank: 0, program: 0, programs_used: Vec::new(), last_program_change_global_tick: None, last_program_change_event_index: None };

        // Fill in some standard events
        trk_chunk_writer.add_other_with_params_u8("SetTrackExpression", 100)?; // Random value for now
        if !(trkid == 0 /* && chanid == 0 */) {
            trk_chunk_writer.add_swdl(link_bytes.1)?;
            trk_chunk_writer.add_bank(link_bytes.0)?;
        }

        Ok(trk_chunk_writer)
    }
    pub fn programs_used(&self) -> &Vec<ProgramUsed> {
        &self.programs_used
    }
    pub fn bank_select<MapProgram>(&mut self, bank: u8, is_default: bool, mut map_program: MapProgram) -> Result<Option<(Rc<RefCell<DSEEvent>>, usize)>, DSEError>
    where
        MapProgram: FnMut(u8, u8, u8, bool, &mut TrkChunkWriter, Rc<RefCell<DSEEvent>>) -> Option<u8> {
        self.bank = bank;
        let mut same_tick = false;
        if let &Some(last_program_change_global_tick) = &self.last_program_change_global_tick {
            if self.current_global_tick - last_program_change_global_tick == 0 {
                self.programs_used.pop();
                same_tick = true;
                if let Some(last_program_change_event_index) = self.last_program_change_event_index {
                    self.trk_events.remove(last_program_change_event_index);
                }
            }
        }
        self.programs_used.push(ProgramUsed::new(self.bank, self.program, is_default));
        self.last_program_change_global_tick = Some(self.current_global_tick);
        let (last_program_change_event, last_program_change_event_index) = self.add_other_with_params_u8("SetProgram", 0)?; // Set to zero for now, change later
        if let Some(program_id) = map_program(self.trkid, self.bank, self.program, same_tick, self, last_program_change_event.clone()) {
            Self::set_other_with_params_u8(&mut *last_program_change_event.borrow_mut(), program_id)?; // Set program to mapped
            self.last_program_change_event_index = Some(last_program_change_event_index);
            Ok(Some((last_program_change_event, last_program_change_event_index)))
        } else {
            self.last_program_change_event_index = None;
            Ok(None)
        }
    }
    pub fn program_change<MapProgram>(&mut self, prgm: u8, is_default: bool, mut map_program: MapProgram) -> Result<Option<(Rc<RefCell<DSEEvent>>, usize)>, DSEError>
    where
        MapProgram: FnMut(u8, u8, u8, bool, &mut TrkChunkWriter, Rc<RefCell<DSEEvent>>) -> Option<u8> {
        self.program = prgm;
        let mut same_tick = false;
        if let &Some(last_program_change_global_tick) = &self.last_program_change_global_tick {
            if self.current_global_tick - last_program_change_global_tick == 0 {
                self.programs_used.pop();
                same_tick = true;
                if let Some(last_program_change_event_index) = self.last_program_change_event_index {
                    self.trk_events.remove(last_program_change_event_index);
                }
            }
        }
        self.programs_used.push(ProgramUsed::new(self.bank, self.program, is_default));
        self.last_program_change_global_tick = Some(self.current_global_tick);
        let (last_program_change_event, last_program_change_event_index) = self.add_other_with_params_u8("SetProgram", 0)?; // Set to zero for now, change later
        if let Some(program_id) = map_program(self.trkid, self.bank, self.program, same_tick, self, last_program_change_event.clone()) {
            Self::set_other_with_params_u8(&mut *last_program_change_event.borrow_mut(), program_id)?; // Set program to mapped
            self.last_program_change_event_index = Some(last_program_change_event_index);
            Ok(Some((last_program_change_event, last_program_change_event_index)))
        } else {
            self.last_program_change_event_index = None;
            Ok(None)
        }
    }
    pub fn note_on(&mut self, key: u8, vel: u8) -> Result<(), DSEError> {
        if self.notes_held.contains_key(&key) {
            println!("{}Overlapping notes detected! By default when there's note overlap a noteoff is sent immediately to avoid them.", "Warning: ".yellow());
            self.note_off(key)?;
        }
        self.add_other_with_params_u8("SetTrackOctave", key / 12)?; // AN EXTRA OCTAVE IS NOT LONGER ADDED BY DEFAULT SO THAT CUSTOM SOUND BANKS WORK CORRECTLY
        let mut evt = PlayNote::default();
        evt.velocity = vel;
        evt.octavemod = 2;
        evt.note = key % 12;
        let (note_on_evt_clone, _) = self.add(DSEEvent::PlayNote(evt));
        self.notes_held.insert(key, (note_on_evt_clone, self.current_global_tick));
        if let Some(program_used) = self.programs_used.last_mut() {
            program_used.notes.entry(key).or_insert(BTreeSet::new()).insert(vel);
        }
        Ok(())
    }
    pub fn note_off(&mut self, key: u8) -> Result<(), DSEError> {
        if !self.notes_held.contains_key(&key) {
            return Ok(());
        }
        let (note_on_event, past_global_tick) = self.notes_held.remove(&key).ok_or(DSEError::_ValidHashMapKeyRemovalFailed())?;
        if let Ok(delta) = u32::try_from(self.current_global_tick - past_global_tick) {
            if let Some(delta) = u24::try_from(delta) {
                if let DSEEvent::PlayNote(evt) = &mut *note_on_event.borrow_mut() {
                    evt.keydownduration = delta.as_int();
                }
            } else {
                return Err(DSEError::DSESmfNotesTooLong());
            }
        } else {
            return Err(DSEError::DSESmfNotesTooLong());
        }
        Ok(())
    }
    pub fn add_other_no_params(&mut self, name: &str) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        Ok(self.add_other_event(evt))
    }
    pub fn set_other_with_params_u8(event: &mut DSEEvent, val: u8) -> Result<(), DSEError> {
        if let DSEEvent::Other(event) = event {
            (&mut event.parameters[..]).write_u8(val)?;
            Ok(())
        } else {
            Err(DSEError::_InvalidEventTypePassedToSetOtherWithParamsU8())
        }
    }
    pub fn add_other_with_params_u8(&mut self, name: &str, val: u8) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        (&mut evt.parameters[..]).write_u8(val)?;
        Ok(self.add_other_event(evt))
    }
    pub fn add_other_with_params_i16<E: ByteOrder>(&mut self, name: &str, val: i16) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        (&mut evt.parameters[..]).write_i16::<E>(val)?;
        Ok(self.add_other_event(evt))
    }
    pub fn add_other_with_params_u16<E: ByteOrder>(&mut self, name: &str, val: u16) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        (&mut evt.parameters[..]).write_u16::<E>(val)?;
        Ok(self.add_other_event(evt))
    }
    pub fn add_swdl(&mut self, unk2: u8) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        self.add_other_with_params_u8("SetSwdl", unk2)
    }
    pub fn add_bank(&mut self, unk1: u8) -> Result<(Rc<RefCell<DSEEvent>>, usize), DSEError> {
        self.add_other_with_params_u8("SetBank", unk1)
    }
    // pub fn next_event_index(&self) -> usize {
    //     self.trk_events.len()
    // }
    pub fn add(&mut self, event: DSEEvent) -> (Rc<RefCell<DSEEvent>>, usize) {
        let new_event_index = self.trk_events.len();
        let new_event = Rc::new(RefCell::new(event));
        self.trk_events.push(new_event.clone());
        (new_event, new_event_index)
    }
    // pub fn get_event_by_index(&mut self, index: usize) -> Option<&Rc<RefCell<DSEEvent>>> {
    //     self.trk_events.get(index)
    // }
    // pub fn get_event_by_index_mut(&mut self, index: usize) -> Option<&mut Rc<RefCell<DSEEvent>>> {
    //     self.trk_events.get_mut(index)
    // }
    pub fn add_playnote_event(&mut self, playnote: PlayNote) -> (Rc<RefCell<DSEEvent>>, usize) {
        self.add(DSEEvent::PlayNote(playnote))
    }
    pub fn add_fixeddurationpause_event(&mut self, fixeddurationpause: FixedDurationPause) -> (Rc<RefCell<DSEEvent>>, usize) {
        self.add(DSEEvent::FixedDurationPause(fixeddurationpause))
    }
    pub fn add_other_event(&mut self, other: Other) -> (Rc<RefCell<DSEEvent>>, usize) {
        self.add(DSEEvent::Other(other))
    }
    /// Fix the current global tick to match the entire song by adding new pause events
    pub fn fix_current_global_tick(&mut self, new_global_tick: u128) -> Result<(), DSEError> {
        let delta = new_global_tick - self.current_global_tick;

        if delta == 0 {
            return Ok(());
        } else if let Ok(delta) = u8::try_from(delta) {
            self.current_global_tick += delta as u128;
            let mut pause_event = Other::default();
            pause_event.code = Other::name_to_code("Pause8Bits")?;
            (&mut pause_event.parameters[..]).write_u8(delta)?;
            self.add_other_event(pause_event);
            return Ok(());
        } else if let Ok(delta) = u16::try_from(delta) {
            self.current_global_tick += delta as u128;
            let mut pause_event = Other::default();
            pause_event.code = Other::name_to_code("Pause16Bits")?;
            (&mut pause_event.parameters[..]).write_u16::<LittleEndian>(delta)?;
            self.add_other_event(pause_event);
            return Ok(());
        } else if let Ok(delta) = u32::try_from(delta) {
            if let Some(delta) = u24::try_from(delta) {
                self.current_global_tick += delta.as_int() as u128;
                let mut pause_event = Other::default();
                pause_event.code = Other::name_to_code("Pause24Bits")?;
                (&mut pause_event.parameters[..]).write_u32::<LittleEndian>(delta.as_int())?;
                self.add_other_event(pause_event);
                return Ok(());
            }
        }
        let delta = u24::max_value().as_int();
        self.current_global_tick += delta as u128;
        let mut pause_event = Other::default();
        pause_event.code = Other::name_to_code("Pause24Bits")?;
        (&mut pause_event.parameters[..]).write_u32::<LittleEndian>(delta)?;
        self.add_other_event(pause_event);

        self.fix_current_global_tick(new_global_tick)
    }
    /// Close the track by adding the end of track event
    pub fn close_track(mut self) -> TrkChunk {
        std::mem::take(&mut self.notes_held); // Dispose of notes_held to free up the Rc's of the track events.

        let mut eot_event = Other::default();
        eot_event.code = Other::name_to_code("EndOfTrack").unwrap();
        self.add_other_event(eot_event);
        
        let mut trk = TrkChunk::default();
        trk.preamble.trkid = self.trkid;
        trk.preamble.chanid = self.chanid;
        trk.events.events = self.trk_events.into_iter().map(|v| Rc::try_unwrap(v).unwrap().into_inner()).collect(); //TODO: Error handling
        trk
    }
}

