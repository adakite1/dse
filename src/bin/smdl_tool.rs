use std::collections::HashMap;
/// Example: .\smdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.smd -o unpack
/// Example: .\smdl_tool.exe from-xml .\unpack\*.smd.xml -o .\NDS_UNPACK\data\SOUND\BGM\

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use byteorder::{WriteBytesExt, LittleEndian};
use chrono::{DateTime, Local, Datelike, Timelike};
use clap::{Parser, command, Subcommand};
use colored::Colorize;
use dse::smdl::{TrkChunk, DSEEvent};
use dse::smdl::events::{Other, PlayNote, FixedDurationPause};
use dse::swdl::DSEString;
use dse::{smdl::SMDL, swdl::SWDL};
use dse::dtype::ReadWrite;

#[path = "../binutils.rs"]
mod binutils;
use binutils::VERSION;
use midly::{Smf, TrackEvent};
use midly::num::{u24, u4, u28};
use crate::binutils::{get_final_output_folder, get_input_output_pairs, open_file_overwrite_rw, valid_file_of_type, get_file_last_modified_date_with_default};

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

        /// Sets the SWDL file or SWD.XML to pair the MIDI files with
        #[arg(value_name = "SWDL")]
        swdl: PathBuf,

        /// Sets the folder to output the encoded files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,

        /// Map Program Change and CC0 Bank Select events to DSE SWDL program id's
        #[arg(short = 'M', long, action)]
        midi_prgch: bool,
    }
}

/// Error to represent a variety of errors emitted by smdl_tool
#[derive(Debug, Clone)]
pub struct SMDLToolError(String);
impl SMDLToolError {
    pub fn new(message: &str) -> SMDLToolError {
        SMDLToolError(String::from(message))
    }
}
impl std::fmt::Display for SMDLToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}
impl std::error::Error for SMDLToolError {  }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::FromXML { input_glob, output_folder } | Commands::ToXML { input_glob, output_folder } => {
            let (source_file_format, change_ext) = match &cli.command {
                Commands::FromXML { input_glob: _, output_folder: _ } => ("xml", ""),
                Commands::ToXML { input_glob: _, output_folder: _ } => ("smd", "smd.xml"),
                _ => panic!("Unreachable")
            };
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext);

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
        Commands::FromMIDI { input_glob, swdl: swdl_path, output_folder, midi_prgch } => {
            let (source_file_format, change_ext) = ("mid", "smd");
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext);

            let mut swdl;
            if valid_file_of_type(swdl_path, "swd") {
                swdl = SWDL::default();
                swdl.read_from_file(&mut File::open(swdl_path)?)?;
            } else if valid_file_of_type(swdl_path, "xml") {
                let st = std::fs::read_to_string(swdl_path)?;
                swdl = quick_xml::de::from_str::<SWDL>(&st)?;
                swdl.regenerate_read_markers()?;
                swdl.regenerate_automatic_parameters()?;
            } else {
                return Err(Box::new(SMDLToolError::new("Provided SWD file is not an SWD file!")));
            }

            for (input_file_path, output_file_path) in input_file_paths {
                print!("Converting {}... ", input_file_path.display());

                // Open input MIDI file
                let (year, month, day, hour, minute, second, centisecond) = get_file_last_modified_date_with_default(&input_file_path)?;
                let smf_source = std::fs::read(&input_file_path)?;
                let smf = Smf::parse(&smf_source)?;
                let tpb = match smf.header.timing {
                    midly::Timing::Metrical(tpb) => tpb.as_int(),
                    _ => {
                        panic!("Only ticks/beat is supported currently as a timing specifier!");
                    }
                };
                let mut fname = input_file_path.file_name().ok_or(SMDLToolError::new(&format!("Couldn't obtain filename of MIDI file with path {}!", input_file_path.display())))?
                    .to_str().ok_or(SMDLToolError::new(&format!("Couldn't convert filename for MIDI file with path {} into a UTF-8 Rust String. Filenames should be pure-ASCII only!", input_file_path.display())))?
                    .to_string();
                if !fname.is_ascii() {
                    panic!("Filenames must be ASCII-only!");
                }
                fname.truncate(15);
                let fname = DSEString::<0xFF>::try_from(fname)?;

                // Setup empty smdl object
                let mut smdl = SMDL::default();
                // Fill in header and song information
                smdl.header.version = 0x415;
                smdl.header.year = year;
                smdl.header.month = month;
                smdl.header.day = day;
                smdl.header.hour = hour;
                smdl.header.minute = minute;
                smdl.header.second = second;
                smdl.header.centisecond = centisecond;
                smdl.header.fname = fname;

                smdl.header.unk1 = swdl.header.unk1;
                smdl.header.unk2 = swdl.header.unk2;

                smdl.song.tpqn = tpb;

                // Fill in tracks
                let midi_messages_combined: Vec<TrackEvent>;
                let midi_messages = match smf.header.format {
                    midly::Format::SingleTrack => { &smf.tracks[0] },
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
                                    *channel = u4::try_from(u8::try_from(mapped_channel)?).ok_or(SMDLToolError::new("MIDI track number out of acceptable range for conversion from Smf1 to Smf0!"))?;
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
                            midi_messages_tmp[i].1.delta = u28::try_from(u32::try_from(new_delta)?).ok_or(SMDLToolError::new("Some notes are too far apart!"))?;
                        }
                        midi_messages_combined = midi_messages_tmp.into_iter().map(|(_, evt)| evt).collect();
                        &midi_messages_combined
                    },
                    _ => {
                        panic!("Only single track MIDI files (with 16 channels or less) are currently supported!");
                    },
                };
                // Vec of TrkChunk's
                let prgi_objects = &swdl.prgi.as_ref().expect("SWDL must contain a prgi chunk!").data.objects;
                let mut trks: [TrkChunkWriter; 17] = std::array::from_fn(|i| TrkChunkWriter::new(i as u8, i as u8, swdl.header.unk1, swdl.header.unk2, prgi_objects[(i + prgi_objects.len() - 1) % prgi_objects.len()].header.id as u8).unwrap());
                // Loop through all the events
                let mut global_tick = 0;
                for midi_msg in midi_messages {
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
                                            if *midi_prgch {
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
                                    if *midi_prgch {
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
                                    let marker = String::from_utf8(marker.into())?;
                                    if marker.trim() == "LoopStart" {
                                        for trk in trks.iter_mut() {
                                            trk.fix_current_global_tick(global_tick)?;
                                            trk.add_other_no_params("LoopPoint")?;
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

                // Fill the tracks into the smdl
                smdl.trks.objects = trks.into_iter().map(|mut x| {
                    x.fix_current_global_tick(global_tick).unwrap();
                    x.close_track()
                }).collect();

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

pub struct TrkChunkWriter {
    current_global_tick: u128,
    trk: TrkChunk,
    notes_held: HashMap<u8, (usize, u128)>,
    bank: u8,
    program: u8
}
impl TrkChunkWriter {
    pub fn new(trkid: u8, chanid: u8, unk1: u8, unk2: u8, default_program: u8) -> Result<TrkChunkWriter, Box<dyn std::error::Error>> {
        let mut trk = TrkChunk::default();
        trk.preamble.trkid = trkid;
        trk.preamble.chanid = chanid;
        let mut trk_chunk_writer = TrkChunkWriter { current_global_tick: 0, trk, notes_held: HashMap::new(), bank: 0, program: 0 };

        // Fill in some standard events
        trk_chunk_writer.add_other_with_params_u8("SetTrackExpression", 100)?; // Random value for now
        if !(trkid == 0 && chanid == 0) {
            trk_chunk_writer.add_swdl(unk2)?;
            trk_chunk_writer.add_bank(unk1)?;
            trk_chunk_writer.add_other_with_params_u8("SetProgram", default_program)?;
        }

        Ok(trk_chunk_writer)
    }
    pub fn bank_select(&mut self, bank: u8) -> Result<(), Box<dyn std::error::Error>> {
        self.bank = bank;
        self.add_other_with_params_u8("SetProgram", self.bank * 128 + self.program)
    }
    pub fn program_change(&mut self, prgm: u8) -> Result<(), Box<dyn std::error::Error>> {
        self.program = prgm;
        self.add_other_with_params_u8("SetProgram", self.bank * 128 + self.program)
    }
    pub fn note_on(&mut self, key: u8, vel: u8) -> Result<(), Box<dyn std::error::Error>> {
        if self.notes_held.contains_key(&key) {
            println!("{}Overlapping notes detected! By default when there's note overlap a noteoff is sent immediately to avoid them.", "Warning: ".yellow());
            self.note_off(key)?;
        }
        self.add_other_with_params_u8("SetTrackOctave", (key - 24) / 12 + 2)?; // An extra octave is added since by default pretty much every patch in the game default to -7 ctune
        let mut evt = PlayNote::default();
        evt.velocity = vel;
        evt.octavemod = 2;
        evt.note = (key - 24) % 12;
        self.add(DSEEvent::PlayNote(evt));
        self.notes_held.insert(key, (self.trk.events.events.len() - 1, self.current_global_tick));
        Ok(())
    }
    pub fn note_off(&mut self, key: u8) -> Result<(), Box<dyn std::error::Error>> {
        if !self.notes_held.contains_key(&key) {
            return Ok(());
        }
        let (index, past_global_tick) = self.notes_held.remove(&key).ok_or(SMDLToolError::new("Internal error"))?;
        if let Ok(delta) = u32::try_from(self.current_global_tick - past_global_tick) {
            if let Some(delta) = u24::try_from(delta) {
                if let DSEEvent::PlayNote(evt) = &mut self.trk.events.events[index] {
                    evt.keydownduration = delta.as_int();
                } else {
                    panic!("Internal error");
                }
            } else {
                panic!("Some notes are too long!");
            }
        } else {
            panic!("Some notes are too long!");
        }
        Ok(())
    }
    pub fn add_other_no_params(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name).unwrap();
        self.add_other_event(evt);
        Ok(())
    }
    pub fn add_other_with_params_u8(&mut self, name: &str, val: u8) -> Result<(), Box<dyn std::error::Error>> {
        let mut evt = Other::default();
        evt.code = Other::name_to_code(name).unwrap();
        (&mut evt.parameters[..]).write_u8(val)?;
        self.add_other_event(evt);
        Ok(())
    }
    pub fn add_swdl(&mut self, unk2: u8) -> Result<(), Box<dyn std::error::Error>> {
        self.add_other_with_params_u8("SetSwdl", unk2)
    }
    pub fn add_bank(&mut self, unk1: u8) -> Result<(), Box<dyn std::error::Error>> {
        self.add_other_with_params_u8("SetBank", unk1)
    }
    pub fn add(&mut self, event: DSEEvent) {
        self.trk.events.events.push(event)
    }
    pub fn add_playnote_event(&mut self, playnote: PlayNote) {
        self.trk.events.events.push(dse::smdl::DSEEvent::PlayNote(playnote))
    }
    pub fn add_fixeddurationpause_event(&mut self, fixeddurationpause: FixedDurationPause) {
        self.trk.events.events.push(dse::smdl::DSEEvent::FixedDurationPause(fixeddurationpause))
    }
    pub fn add_other_event(&mut self, other: Other) {
        self.trk.events.events.push(dse::smdl::DSEEvent::Other(other))
    }
    /// Fix the current global tick to match the entire song by adding new pause events
    pub fn fix_current_global_tick(&mut self, new_global_tick: u128) -> Result<(), Box<dyn std::error::Error>> {
        let delta = new_global_tick - self.current_global_tick;

        if delta == 0 {
            return Ok(());
        } else if let Ok(delta) = u8::try_from(delta) {
            self.current_global_tick += delta as u128;
            let mut pause_event = Other::default();
            pause_event.code = Other::name_to_code("Pause8Bits").unwrap();
            (&mut pause_event.parameters[..]).write_u8(delta)?;
            self.add_other_event(pause_event);
            return Ok(());
        } else if let Ok(delta) = u16::try_from(delta) {
            self.current_global_tick += delta as u128;
            let mut pause_event = Other::default();
            pause_event.code = Other::name_to_code("Pause16Bits").unwrap();
            (&mut pause_event.parameters[..]).write_u16::<LittleEndian>(delta)?;
            self.add_other_event(pause_event);
            return Ok(());
        } else if let Ok(delta) = u32::try_from(delta) {
            if let Some(delta) = u24::try_from(delta) {
                self.current_global_tick += delta.as_int() as u128;
                let mut pause_event = Other::default();
                pause_event.code = Other::name_to_code("Pause24Bits").unwrap();
                (&mut pause_event.parameters[..]).write_u32::<LittleEndian>(delta.as_int())?;
                self.add_other_event(pause_event);
                return Ok(());
            }
        }
        let delta = u24::max_value().as_int();
        self.current_global_tick += delta as u128;
        let mut pause_event = Other::default();
        pause_event.code = Other::name_to_code("Pause24Bits").unwrap();
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

