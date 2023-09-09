use std::{borrow::Cow, collections::HashMap};

use byteorder::{WriteBytesExt, LittleEndian};
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

pub fn copy_midi_messages<'a>(midi_messages: Cow<'a, [TrackEvent<'a>]>, trks: &mut [TrkChunkWriter], use_midi_prgch: bool) -> Result<u128, DSEError> {
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
                                if use_midi_prgch {
                                    println!("{}", "Found --midi-prgch flag! Processing bank select message.".green());
                                    trks[channel_i].bank_select(value.as_int())?;
                                }
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
                        if use_midi_prgch {
                            println!("{}", "Found --midi-prgch flag! Processing program change message.".green());
                            trks[channel_i].program_change(program.as_int())?;
                        }
                    },
                    midly::MidiMessage::ChannelAftertouch { vel } => { /* Ignore channel aftertouch events */ },
                    midly::MidiMessage::PitchBend { bend } => { /* Ignore pitchbend events */ },
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
                            if marker.trim() == "LoopStart" {
                                for trk in trks.iter_mut() {
                                    trk.fix_current_global_tick(global_tick)?;
                                    trk.add_other_no_params("LoopPoint")?;
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

pub struct TrkChunkWriter {
    current_global_tick: u128,
    trk: TrkChunk,
    notes_held: HashMap<u8, (usize, u128)>,
    bank: u8,
    program: u8,
    programs_used: Vec<u8>,
    last_program_change_global_tick: u128
}
impl TrkChunkWriter {
    pub fn create(trkid: u8, chanid: u8, link_bytes: (u8, u8), default_program: u8) -> Result<TrkChunkWriter, DSEError> {
        let mut trk = TrkChunk::default();
        trk.preamble.trkid = trkid;
        trk.preamble.chanid = chanid;
        let mut trk_chunk_writer = TrkChunkWriter { current_global_tick: 0, trk, notes_held: HashMap::new(), bank: 0, program: 0, programs_used: Vec::new(), last_program_change_global_tick: 0 };

        // Fill in some standard events
        trk_chunk_writer.add_other_with_params_u8("SetTrackExpression", 100)?; // Random value for now
        if !(trkid == 0 && chanid == 0) {
            trk_chunk_writer.add_swdl(link_bytes.1)?;
            trk_chunk_writer.add_bank(link_bytes.0)?;
            trk_chunk_writer.add_other_with_params_u8("SetProgram", default_program)?;
            trk_chunk_writer.programs_used.push(default_program);
        }

        Ok(trk_chunk_writer)
    }
    pub fn programs_used(&self) -> &Vec<u8> {
        &self.programs_used
    }
    pub fn bank_select(&mut self, bank: u8) -> Result<(), DSEError> {
        self.bank = bank;
        if self.current_global_tick - self.last_program_change_global_tick == 0 { self.programs_used.pop(); }
        self.programs_used.push(self.bank * 128 + self.program);
        self.last_program_change_global_tick = self.current_global_tick;
        self.add_other_with_params_u8("SetProgram", self.bank * 128 + self.program)
    }
    pub fn program_change(&mut self, prgm: u8) -> Result<(), DSEError> {
        self.program = prgm;
        if self.current_global_tick - self.last_program_change_global_tick == 0 { self.programs_used.pop(); }
        self.programs_used.push(self.bank * 128 + self.program);
        self.last_program_change_global_tick = self.current_global_tick;
        self.add_other_with_params_u8("SetProgram", self.bank * 128 + self.program)
    }
    pub fn note_on(&mut self, key: u8, vel: u8) -> Result<(), DSEError> {
        if self.notes_held.contains_key(&key) {
            println!("{}Overlapping notes detected! By default when there's note overlap a noteoff is sent immediately to avoid them.", "Warning: ".yellow());
            self.note_off(key)?;
        }
        self.add_other_with_params_u8("SetTrackOctave", (key - 24) / 12 + 2)?; // AN EXTRA OCTAVE IS NOT LONGER ADDED BY DEFAULT SO THAT CUSTOM SOUND BANKS WORK CORRECTLY
        let mut evt = PlayNote::default();
        evt.velocity = vel;
        evt.octavemod = 2;
        evt.note = (key - 24) % 12;
        self.add(DSEEvent::PlayNote(evt));
        self.notes_held.insert(key, (self.trk.events.events.len() - 1, self.current_global_tick));
        Ok(())
    }
    pub fn note_off(&mut self, key: u8) -> Result<(), DSEError> {
        if !self.notes_held.contains_key(&key) {
            return Ok(());
        }
        let (index, past_global_tick) = self.notes_held.remove(&key).ok_or(DSEError::_ValidHashMapKeyRemovalFailed())?;
        if let Ok(delta) = u32::try_from(self.current_global_tick - past_global_tick) {
            if let Some(delta) = u24::try_from(delta) {
                if let DSEEvent::PlayNote(evt) = &mut self.trk.events.events[index] {
                    evt.keydownduration = delta.as_int();
                } else {
                    return Err(DSEError::_CorrespondingNoteOnNotFound())
                }
            } else {
                return Err(DSEError::DSESmfNotesTooLong());
            }
        } else {
            return Err(DSEError::DSESmfNotesTooLong());
        }
        Ok(())
    }
    pub fn add_other_no_params(&mut self, name: &str) -> Result<(), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        self.add_other_event(evt);
        Ok(())
    }
    pub fn add_other_with_params_u8(&mut self, name: &str, val: u8) -> Result<(), DSEError> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name)?;
        (&mut evt.parameters[..]).write_u8(val)?;
        self.add_other_event(evt);
        Ok(())
    }
    pub fn add_swdl(&mut self, unk2: u8) -> Result<(), DSEError> {
        self.add_other_with_params_u8("SetSwdl", unk2)
    }
    pub fn add_bank(&mut self, unk1: u8) -> Result<(), DSEError> {
        self.add_other_with_params_u8("SetBank", unk1)
    }
    pub fn add(&mut self, event: DSEEvent) {
        self.trk.events.events.push(event)
    }
    pub fn add_playnote_event(&mut self, playnote: PlayNote) {
        self.trk.events.events.push(DSEEvent::PlayNote(playnote))
    }
    pub fn add_fixeddurationpause_event(&mut self, fixeddurationpause: FixedDurationPause) {
        self.trk.events.events.push(DSEEvent::FixedDurationPause(fixeddurationpause))
    }
    pub fn add_other_event(&mut self, other: Other) {
        self.trk.events.events.push(DSEEvent::Other(other))
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
        let mut eot_event = Other::default();
        eot_event.code = Other::name_to_code("EndOfTrack").unwrap();
        self.add_other_event(eot_event);
        self.trk
    }
}

