use core::panic;
use std::io::{Read, Write, Seek, SeekFrom, Cursor};
use bevy_reflect::Reflect;
use byteorder::{ReadBytesExt, WriteBytesExt};
use serde::{Serialize, Deserialize};

use crate::swdl::DSEString;
use crate::peek_byte;
use crate::dtype::*;
use crate::deserialize_with;

pub mod midi;

/// By default, all unknown bytes that do not have a consistent pattern of values in the EoS roms are included in the XML.
/// However, a subset of these not 100% purpose-certain bytes is 80% or something of values that have "typical" values.
/// Setting this to true will strip all those somewhat certain bytes from the Serde serialization process, and replace them
/// with their typical values.
const fn serde_use_common_values_for_unknowns<T>(_: &T) -> bool {
    true
}

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields

#[derive(Debug, Reflect, Serialize, Deserialize)]
pub struct SMDLHeader {
    #[serde(default = "GenericDefaultU32::<0x6C646D73>::value")]
    #[serde(skip_serializing)]
    pub magicn: u32, //  The 4 characters "smdl" {0x73,0x6D,0x64,0x6C} 
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk7: u32, // Always zeroes
    #[serde(default)]
    #[serde(skip_serializing)]
    pub flen: u32,
    #[serde(default = "GenericDefaultU16::<0x415>::value")]
    #[serde(rename = "@version")]
    pub version: u16,
    pub unk1: u8, // There's also two consecutive bytes in the SWDL header with unknown purposes, could it be?? Could this be the link byte described by @nazberrypie in Trezer???!?
    pub unk2: u8,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk3: u32, // Always zeroes
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk4: u32, // Always zeroes

    #[serde(rename = "@year")]
    pub year: u16,
    #[serde(rename = "@month")]
    pub month: u8,
    #[serde(rename = "@day")]
    pub day: u8,
    #[serde(rename = "@hour")]
    pub hour: u8,
    #[serde(rename = "@minute")]
    pub minute: u8,
    #[serde(rename = "@second")]
    pub second: u8,
    #[serde(rename = "@centisecond")]
    pub centisecond: u8, // unsure

    #[serde(rename = "@fname")]
    /// Interestingly, while SWDL files all use 0xAA for the padding, after looking through all the SMDL files, they use 0xFF instead. Unsure of why that is, but maybe it's another way to distinguish swdls and smdls that later became unused because of the magicn? Or perhaps a easy to way recognize them in memory, idk.
    pub fname: DSEString<0xFF>,

    #[serde(default = "GenericDefaultU32::<0x1>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk5: u32, // Unknown, usually 0x1
    #[serde(default = "GenericDefaultU32::<0x1>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk6: u32, // Unknown, usually 0x1
    #[serde(default = "GenericDefaultU32::<0xFFFFFFFF>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk8: u32, // Unknown, usually 0xFFFFFFFF
    #[serde(default = "GenericDefaultU32::<0xFFFFFFFF>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk9: u32, // Unknown, usually 0xFFFFFFFF
}
impl Default for SMDLHeader {
    fn default() -> Self {
        SMDLHeader {
            magicn: 0x6C646D73,
            unk7: 0,
            flen: 0,
            version: 0x415,
            unk1: 0,
            unk2: 0,
            unk3: 0,
            unk4: 0,
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            centisecond: 0,
            fname: DSEString::<0xFF>::default(),
            unk5: 0x1,
            unk6: 0x1,
            unk8: 0xFFFFFFFF,
            unk9: 0xFFFFFFFF
        }
    }
}
impl AutoReadWrite for SMDLHeader {  }

#[derive(Debug, Reflect, Serialize, Deserialize)]
pub struct SongChunk {
    #[serde(default = "GenericDefaultU32::<0x676E6F73>::value")]
    #[serde(skip_serializing)]
    pub label: u32, // Song chunk label "song" {0x73,0x6F,0x6E,0x67}
    #[serde(default = "GenericDefaultU32::<0x01000000>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk1: u32, // usually 0x1
    #[serde(default = "GenericDefaultU32::<0x0000FF10>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk2: u32, // usually 0xFF10
    #[serde(default = "GenericDefaultU32::<0xFFFFFFB0>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk3: u32, // usually 0xFFFFFFB0
    #[serde(default = "GenericDefaultU16::<0x1>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk4: u16, // usually 0x1

    pub tpqn: u16, // ticks per quarter note (usually 48 which is consistent with the MIDI standard)
    
    #[serde(default = "GenericDefaultU16::<0xFF01>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk5: u16, // usually 0xFF01

    #[serde(default)]
    #[serde(skip_serializing)]
    pub nbtrks: u8, // number of track(trk) chunks
    
    /// The Nintendo DS has 16 hardware audio channels. This could be referring to that as it's (I think) pretty much always a value <= 16. It's probably set to the max number of audio channels used by the tracks below. Based on this assumption, for now, this will be automatically be set to the max chanid used by the tracks below, plus 1. Will change if things turn out different.
    #[serde(default)]
    #[serde(skip_serializing)]
    pub nbchans: u8, // number of channels (unsure of how channels work in DSE)

    #[serde(default = "GenericDefaultU32::<0x0F000000>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk6: u32, // usually 0x0f000000
    #[serde(default = "GenericDefaultU32::<0xFFFFFFFF>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk7: u32, // usually 0xffffffff
    #[serde(default = "GenericDefaultU32::<0x40000000>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk8: u32, // usually 0x40000000
    #[serde(default = "GenericDefaultU32::<0x00404000>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk9: u32, // usually 0x00404000

    #[serde(default = "GenericDefaultU16::<0x0200>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk10: u16, // usually 0x0200
    #[serde(default = "GenericDefaultU16::<0x0800>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk11: u16, // usually 0x0800
    #[serde(default = "GenericDefaultU32::<0xFFFFFF00>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk12: u32, // usually 0xffffff00

    #[serde(default = "GenericDefaultByteArray::<0xFF, 16>::value")]
    #[serde(skip_serializing)]
    pub unkpad: [u8; 16], // unknown sequence of 16 0xFF bytes
}
impl Default for SongChunk {
    fn default() -> Self {
        SongChunk {
            label: 0x676E6F73,
            unk1: 0x01000000,
            unk2: 0x0000FF10,
            unk3: 0xFFFFFFB0,
            unk4: 0x1,
            tpqn: 0,
            unk5: 0xFF01,
            nbtrks: 0,
            nbchans: 0,
            unk6: 0x0F000000,
            unk7: 0xFFFFFFFF,
            unk8: 0x40000000,
            unk9: 0x00404000,
            unk10: 0x0200,
            unk11: 0x0800,
            unk12: 0xFFFFFF00,
            unkpad: [0xFF; 16]
        }
    }
}
impl AutoReadWrite for SongChunk {  }

#[derive(Debug, Reflect, Serialize, Deserialize)]
pub struct TrkChunkHeader {
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default = "GenericDefaultU32::<0x206B7274>::value")]
    #[serde(rename = "@label")]
    #[serde(skip_serializing)]
    pub label: u32, // track chunk label "trk\0x20" {0x74,0x72,0x6B,0x20}
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default = "GenericDefaultU32::<0x01000000>::value")]
    #[serde(rename = "@param1")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub param1: u32, // usually 0x01000000
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default = "GenericDefaultU32::<0x0000FF04>::value")]
    #[serde(rename = "@param2")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub param2: u32, // usually 0x0000FF04
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(rename = "@chunklen")]
    #[serde(skip_serializing)]
    pub chunklen: u32, // length of the trk chunk. starting after this field to the first 0x98 event encountered in the track. length is in bytes like its swdl counterpart.
}
impl Default for TrkChunkHeader {
    fn default() -> Self {
        TrkChunkHeader {
            label: 0x206B7274,
            param1: 0x01000000,
            param2: 0x0000FF04,
            chunklen: 0
        }
    }
}
impl AutoReadWrite for TrkChunkHeader {  }
#[derive(Debug, Default, Reflect, Serialize, Deserialize)]
pub struct TrkChunkPreamble {
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@trkid")]
    pub trkid: u8, // the track id of the track. a number between 0 and 0x11
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@chanid")]
    pub chanid: u8, // the channel id of the track. a number between 0 and 0x0F?
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(rename = "@unk1")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk1: u8, // often 0
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(rename = "@unk2")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk2: u8, // often 0
}
impl AutoReadWrite for TrkChunkPreamble {  }

pub mod events {
    use byteorder::{ReadBytesExt, LittleEndian, WriteBytesExt, BigEndian};
    use phf::phf_ordered_map;
    use serde::{Serialize, Deserialize};

    use crate::dtype::{ReadWrite, DSEError};

    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct PlayNote {
        pub velocity: u8,
        #[serde(default)]
        #[serde(skip_serializing)]
        _nbparambytes: u8,
        pub octavemod: u8,
        pub note: u8,
        pub keydownduration: u32
    }
    impl ReadWrite for PlayNote {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
            writer.write_u8(self.velocity)?;

            let mut keydownduration = [0_u8; 4];
            (&mut keydownduration[..]).write_u32::<BigEndian>(self.keydownduration)?;

            let keydowndurationlen = match self.keydownduration {
                0x0 => 0,
                0x1..=0xFF => 1,
                0x100..=0xFFFF => 2,
                0x10000..=0xFFFFFF => 3,
                _ => {
                    return Err(DSEError::Invalid("Keydown duration needs to be within the range 0 to 0xFFFFFF".to_string()));
                }
            };

            let note_data = (keydowndurationlen << 6) + (self.octavemod << 4) + self.note;
            writer.write_u8(note_data)?;
            writer.write_all(&keydownduration[(4-keydowndurationlen as usize)..])?;
            Ok(2 + keydowndurationlen as usize)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
            self.velocity = reader.read_u8()?;
            let note_data = reader.read_u8()?;
            self._nbparambytes = (note_data & 0b11000000) >> 6;
            self.octavemod = (note_data & 0b00110000) >> 4;
            self.note = note_data & 0b00001111;

            // print!("{}", ['t', 'T', 'y', 'Y', 'u', 'i', 'I', 'o', 'O', 'p', 'P', 'a', ' '][self.note as usize]);

            let mut keydownduration = [0_u8; 4];
            for i in (4-self._nbparambytes as usize)..4 {
                keydownduration[i] = reader.read_u8()?;
            }
            self.keydownduration = (&keydownduration[..]).read_u32::<BigEndian>()?;
            Ok(())
        }
    }

    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct FixedDurationPause {
        duration: u8,
    }
    impl ReadWrite for FixedDurationPause {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
            writer.write_u8(self.duration)?;
            Ok(1)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
            self.duration = reader.read_u8()?;
            Ok(())
        }
    }

    static CODE_TRANSLATIONS: phf::OrderedMap<&'static str, (bool, u8, u8)> = phf_ordered_map! {
        "RepeatLastPause" => (false, 0x90, 0),
        "AddToLastPause" => (false, 0x91, 1),
        "Pause8Bits" => (false, 0x92, 1),
        "Pause16Bits" => (false, 0x93, 2),
        "Pause24Bits" => (false, 0x94, 3),
        "PauseUntilRelease" => (false, 0x95, 1),
        "0x96" => (true, 0x96, 0),
        "0x97" => (true, 0x97, 0),
        "EndOfTrack" => (false, 0x98, 0),
        "LoopPoint" => (false, 0x99, 0),
        "0x9A" => (true, 0x9A, 0),
        "0x9B" => (true, 0x9B, 0),
        "RepeatFrom" => (false, 0x9C, 1), // Mark location subsequent RepeatSegment events should repeat from, and also indicates the amount of times to repeat. (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "RepeatSegment" => (false, 0x9D, 0), // Repeat the segment starting at the last RepeatFrom event (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "AfterRepeat" => (false, 0x9E, 0), // After the last "RepeatSegment" event has finished its repeats, playback will jump here. (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "0x9F" => (true, 0x9F, 0),
        "SetTrackOctave" => (false, 0xA0, 1),
        "AddToTrackOctave" => (false, 0xA1, 1),
        "0xA2" => (true, 0xA2, 0),
        "0xA3" => (true, 0xA3, 0),
        "SetTempo" => (false, 0xA4, 1),
        "SetTempo2" => (false, 0xA5, 1), // Duplicate
        "0xA6" => (true, 0xA6, 0),
        "0xA7" => (true, 0xA7, 0),
        "SetSwdlAndBank" => (false, 0xA8, 2), // Set both the swdl id and the bank id. First param is swdl, second is bank. (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "SetSwdl" => (false, 0xA9, 1), // unk2 from the track header is mapped to the 0xA9 event here. (Set that first unknown value from the track's header.) (confirmed by dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "SetBank" => (false, 0xAA, 1), // unk1 from the track header is mapped to the 0xAA event here. (Set that second unknown value from the track's header.) (confirmed by dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "SkipNextByte" => (false, 0xAB, 1), // While this isn't supposed to have any parameters, setting the parameters to 1 is an easy way to implement this without changing things too much.
        "SetProgram" => (false, 0xAC, 1),
        "0xAD" => (true, 0xAD, 0),
        "0xAE" => (true, 0xAE, 0),
        "FadeSongVolume" => (false, 0xAF, 3), // Sweep the song's volume. First arg is the rate, second is the target volume (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "DisableEnvelope" => (false, 0xB0, 0), // Disable envelope  (from dse_sequence.hpp, ppmdu_2, updated information by Psy)
        "SetEnvAtkLvl" => (false, 0xB1, 1), // Envelope attack  ('')
        "SetEnvAtkTime" => (false, 0xB2, 1), // Envelope attack time   ('')
        "SetEnvHold" => (false, 0xB3, 1), // Envelope hold  ('')
        "SetEnvDecSus" => (false, 0xB4, 2), // Envelope decay and sustain  ('')
        "SetEnvFade" => (false, 0xB5, 1), // Envelope fade  ('')
        "SetEnvRelease" => (false, 0xB6, 1), // Envelope release  ('')
        "0xB7" => (true, 0xB7, 0),
        "0xB8" => (true, 0xB8, 0),
        "0xB9" => (true, 0xB9, 0),
        "0xBA" => (true, 0xBA, 0),
        "0xBB" => (true, 0xBB, 0),
        "SetNoteVolume" => (false, 0xBC, 1), // SetNoteVolume (?)  ('')
        "0xBD" => (true, 0xBD, 0),
        "SetChanPan" => (false, 0xBE, 1), // Sets the current channel's pan ('')
        "0xBF" => (false, 0xBF, 1),
        "0xC0" => (false, 0xC0, 0), // Still unknown, but originally 1 in tech docs, zero in dse_sequence.hpp, ppmdu_2
        "0xC1" => (true, 0xC1, 0),
        "0xC2" => (true, 0xC2, 0),
        "SetChanVolume" => (false, 0xC3, 1), // Sets the current channel's volume ('')
        "0xC4" => (true, 0xC4, 0),
        "0xC5" => (true, 0xC5, 0),
        "0xC6" => (true, 0xC6, 0),
        "0xC7" => (true, 0xC7, 0),
        "0xC8" => (true, 0xC8, 0),
        "0xC9" => (true, 0xC9, 0),
        "0xCA" => (true, 0xCA, 0),
        "SkipNext2Bytes" => (false, 0xCB, 2), // While this isn't supposed to have any parameters, setting the parameters to 2 is an easy way to implement this without changing things too much.
        "0xCC" => (true, 0xCC, 0),
        "0xCD" => (true, 0xCD, 0),
        "0xCE" => (true, 0xCE, 0),
        "0xCF" => (true, 0xCF, 0),
        "SetFTune" => (false, 0xD0, 1), // Set fine tune  ('')
        "AddFTune" => (false, 0xD1, 1), // Add to fine tune  ('')
        "SetCTune" => (false, 0xD2, 1), // Set coarse tune  ('')
        "AddCTune" => (false, 0xD3, 2), // Add to coarse tune  ('')
        "SweepTune" => (false, 0xD4, 3), // Interpolate between tune values ('')
        "SetRndNoteRng" => (false, 0xD5, 2), // Sets random notes range (random notes? how much horse power did they pack in an NDS music player?? I mean it makes me more excited to use it, but still) ('')
        "SetDetuneRng" => (false, 0xD6, 2), // Sets detune range ('')
        "PitchBend" => (false, 0xD7, 2),
        "0xD8" => (false, 0xD8, 2),
        "0xD9" => (true, 0xD9, 0),
        "0xDA" => (true, 0xDA, 0),
        "SetPitchBendRng" => (false, 0xDB, 1), // Bend range for pitch bending  ('')
        "SetLFO1" => (false, 0xDC, 5), // LFO rate, depth, and waveform  ('')
        "SetLFO1DelayFade" => (false, 0xDD, 4), // LFO delay and fade out  ('')
        "0xDE" => (true, 0xDE, 0),
        "RouteLFO1ToPitch" => (false, 0xDF, 1), // Route the LFO1 output to note pitch if set to > 0 ('')
        "SetTrackVolume" => (false, 0xE0, 1),
        "AddTrkVol" => (false, 0xE1, 1), // Add to track volume ('')
        "SweepTrackVol" => (false, 0xE2, 3), // Interpolate track volume to specified value at rate ('')
        "SetTrackExpression" => (false, 0xE3, 1),
        "SetLFO2" => (false, 0xE4, 5),
        "SetLFO2DelFade" => (false, 0xE5, 4),
        "0xE6" => (true, 0xE6, 0),
        "RouteLFO2ToVol" => (false, 0xE7, 1), // Route the LFO2 output to volume if set to > 0 ('')
        "SetTrackPan" => (false, 0xE8, 1),
        "AddTrkPan" => (false, 0xE9, 1), // Sets the panning of the track. ('')
        "SweepTrkPan" => (false, 0xEA, 3), // Interpolate the track's panning value to the specified value at the specified rate ('')
        "0xEB" => (true, 0xEB, 0),
        "SetLFO3" => (false, 0xEC, 5), // Sets LFO rate, depth, and waveform. ('')
        "SetLFO3DelFade" => (false, 0xED, 4), // Sets the LFO effect delay, and fade out ('')
        "0xEE" => (true, 0xEE, 0),
        "RouteLFO3ToPan" => (false, 0xEF, 1), // Routes the LFO3 output to the track panning value if > 0 ('')
        "SetLFO" => (false, 0xF0, 5), // Sets LFO rate, depth, and waveform ('')
        "SetLFODelFade" => (false, 0xF1, 4), // Sets the LFO effect delay, and fade out ('')
        "SetLFOParam" => (false, 0xF2, 2), // Sets the LFO's parameter and its value
        "SetLFORoute" => (false, 0xF3, 3), // Set what LFO is routed to what, and whether its enabled
        "0xF4" => (true, 0xF4, 0),
        "0xF5" => (true, 0xF5, 0),
        "0xF6" => (false, 0xF6, 1),
        "0xF7" => (true, 0xF7, 0),
        "SkipNext2Bytes2" => (false, 0xF8, 2), // While this isn't supposed to have any parameters, setting the parameters to 2 is an easy way to implement this without changing things too much.
        "0xF9" => (true, 0xF9, 0),
        "0xFA" => (true, 0xFA, 0),
        "0xFB" => (true, 0xFB, 0),
        "0xFC" => (true, 0xFC, 0),
        "0xFD" => (true, 0xFD, 0),
        "0xFE" => (true, 0xFE, 0),
        "0xFF" => (true, 0xFF, 0),
    };
    mod named {
        use serde::{Serializer, Deserializer, Serialize, Deserialize};

        use super::Other;

        pub fn serialize<S: Serializer>(v: &u8, s: S) -> Result<S::Ok, S::Error> {
            let (name, &(_, _, _)) = Other::lookup(*v).map_err(serde::ser::Error::custom)?;
            name.to_string().serialize(s)
        }
        
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
            Other::name_to_code(&String::deserialize(d)?).map_err(serde::de::Error::custom)
        }
    }
    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct Other {
        #[serde(rename = "@code")]
        #[serde(with = "named")]
        pub code: u8,
        pub parameters: [u8; 5]
    }
    impl Other {
        pub fn lookup(v: u8) -> Result<(&'static &'static str, &'static (bool, u8, u8)), DSEError> {
            CODE_TRANSLATIONS.index(v as usize - 0x90).ok_or(DSEError::DSEEventLookupError(v))
        }
        pub fn name_to_code(name: &str) -> Result<u8, DSEError> {
            if let Some(&(_, code, _)) = CODE_TRANSLATIONS.get(name) {
                Ok(code)
            } else if let Ok(code_u8) = name.parse::<u8>() {
                Ok(code_u8)
            } else if let Ok(code_u8) = u8::from_str_radix(name.trim_start_matches("0x"), 16) {
                Ok(code_u8)
            } else {
                Err(DSEError::DSEEventNameLookupError(name.to_string()))
            }
        }
        pub fn is_eot_event(&self) -> bool {
            self.code == 0x98
        }
    }
    impl ReadWrite for Other {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
            let (_, &(_, _, nbparams)) = CODE_TRANSLATIONS.index(self.code as usize - 0x90).ok_or(DSEError::DSEEventLookupError(self.code))?;
            writer.write_u8(self.code)?;
            writer.write_all(&self.parameters[..nbparams as usize])?;
            Ok(1 + nbparams as usize)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
            self.code = reader.read_u8()?;
            let (_, &(_, _, nbparams)) = CODE_TRANSLATIONS.index(self.code as usize - 0x90).ok_or(DSEError::DSEEventLookupError(self.code))?;
            for i in 0..nbparams as usize {
                self.parameters[i] = reader.read_u8()?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DSEEvent {
    PlayNote(events::PlayNote),
    FixedDurationPause(events::FixedDurationPause),
    Other(events::Other)
}
impl Default for DSEEvent {
    fn default() -> Self {
        DSEEvent::Other(events::Other::default())
    }
}
impl DSEEvent {
    pub fn is_eot_event(&self) -> bool {
        match self {
            DSEEvent::Other(other) => other.is_eot_event(),
            _ => false
        }
    }
}
impl ReadWrite for DSEEvent {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        match self {
            DSEEvent::PlayNote(event) => event.write_to_file(writer),
            DSEEvent::FixedDurationPause(event) => event.write_to_file(writer),
            DSEEvent::Other(event) => event.write_to_file(writer)
        }
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        match peek_byte!(reader)? {
            0x0..=0x7F => {
                let mut event = events::PlayNote::default();
                event.read_from_file(reader)?;
                *self = DSEEvent::PlayNote(event);
            },
            0x80..=0x8F => {
                let mut event = events::FixedDurationPause::default();
                event.read_from_file(reader)?;
                *self = DSEEvent::FixedDurationPause(event);
            },
            0x90..=0xFF => {
                let mut event = events::Other::default();
                event.read_from_file(reader)?;
                *self = DSEEvent::Other(event);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrkEvents {
    /// Fail-safe mechanism for dangling 0x98 bytes like the one in track 1 of bgm0016.smd
    #[serde(default)]
    #[serde(skip_serializing)]
    _read_n: u64,
    #[serde(rename = "$value")]
    pub events: Vec<DSEEvent>,
}
impl TrkEvents {
    pub fn new(chunklen: u64) -> TrkEvents {
        TrkEvents {
            _read_n: chunklen,
            events: Vec::new()
        }
    }
    pub fn set_read_params(&mut self, chunklen: u64) {
        self._read_n = chunklen;
    }
}
impl ReadWrite for TrkEvents {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = 0;
        for event in &self.events {
            bytes_written += event.write_to_file(writer)?;
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        let _trk_events_len = self._read_n - 4; // Subtract the preamble's length!
        let start_cursor_pos = reader.seek(SeekFrom::Current(0))?; // Failsafe
        let mut current_cursor_pos;
        let mut evt;
        let mut read_event = || -> Result<(DSEEvent, u64), DSEError> {
            let mut event = DSEEvent::default();
            event.read_from_file(reader)?;
            Ok((event, reader.seek(SeekFrom::Current(0))?))
        };
        (evt, current_cursor_pos) = read_event()?;
        self.events.push(evt);
        while current_cursor_pos < start_cursor_pos + _trk_events_len {
            (evt, current_cursor_pos) = read_event()?;
            self.events.push(evt);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrkChunk {
    #[serde(default)]
    #[serde(flatten)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub header: TrkChunkHeader,
    #[serde(flatten)]
    pub preamble: TrkChunkPreamble,
    pub events: TrkEvents,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub _padding: Vec<u8>
}
impl Default for TrkChunk {
    fn default() -> Self {
        TrkChunk {
            header: TrkChunkHeader::default(),
            preamble: TrkChunkPreamble::default(),
            events: TrkEvents::new(0),
            _padding: Vec::new()
        }
    }
}
impl ReadWrite for TrkChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.preamble.write_to_file(writer)?;
        bytes_written += self.events.write_to_file(writer)?;
        let bytes_written_aligned = ((bytes_written - 1) | 3) + 1;
        let pad_len = bytes_written_aligned - bytes_written;
        for _ in 0..pad_len {
            writer.write_u8(0x98)?;
        }
        Ok(bytes_written_aligned)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.preamble.read_from_file(reader)?;
        self.events.set_read_params(self.header.chunklen as u64);
        self.events.read_from_file(reader)?;
        while peek_byte!(reader)? == 0x98 {
            self._padding.push(reader.read_u8()?);
        }
        Ok(())
    }
}
/// Note: BGM0016 is a counter example to all the indices having to be in perfect order
impl IsSelfIndexed for TrkChunk {
    fn is_self_indexed(&self) -> Option<usize> {
        // Some(self.preamble.trkid as usize)
        None
    }
    fn change_self_index(&mut self, _: usize) -> Result<(), DSEError> {
        // self.preamble.trkid = new_index.try_into()?;
        // Ok(())
        Err(DSEError::Invalid("Track chunks do not have indices!!".to_string()))
    }
}

#[derive(Debug, Reflect, Serialize, Deserialize)]
pub struct EOCChunk {
    #[serde(default = "GenericDefaultU32::<0x20636F65>::value")]
    #[serde(skip_serializing)]
    pub label: u32, // the ChunkID -  The chunk ID "eoc\0x20" {0x65, 0x6F, 0x63, 0x20} 
    #[serde(default = "GenericDefaultU32::<0x01000000>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub param1: u32, //  Unknown meaning, is often 0x00000001. 
    #[serde(default = "GenericDefaultU32::<0x0000FF04>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub param2: u32, //  Unknown meaning, is often 0x04FF0000. 
    #[serde(default)]
    #[serde(skip_serializing)]
    pub chunklen: u32, //  Always 0, for end of content chunks. 
}
impl Default for EOCChunk {
    fn default() -> Self {
        EOCChunk {
            label: 0x20636F65, //  The chunk ID "eoc\0x20" {0x65, 0x6F, 0x63, 0x20} 
            param1: 0x01000000, //  Unknown meaning, is often 0x00000001. 
            param2: 0x0000FF04, //  Unknown meaning, is often 0x04FF0000. 
            chunklen: 0
        }
    }
}
impl AutoReadWrite for EOCChunk {  }

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SMDL {
    pub header: SMDLHeader,
    pub song: SongChunk,
    pub trks: Table<TrkChunk>,
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub eoc: EOCChunk
}
impl DSELinkBytes for SMDL {
    fn get_link_bytes(&self) -> (u8, u8) {
        (self.header.unk1, self.header.unk2)
    }
    fn set_link_bytes(&mut self, link_bytes: (u8, u8)) {
        (self.header.unk1, self.header.unk2) = link_bytes;
    }
    fn set_unk1(&mut self, unk1: u8) {
        self.header.unk1 = unk1;
    }
    fn set_unk2(&mut self, unk2: u8) {
        self.header.unk2 = unk2;
    }
}
impl SMDL {
    pub fn regenerate_read_markers(&mut self) -> Result<(), DSEError> { //TODO: make more efficient
        // ======== NUMERICAL VALUES (LENGTHS, SLOTS, etc) ========
        self.header.flen = self.write_to_file(&mut Cursor::new(&mut Vec::new()))?.try_into().map_err(|_| DSEError::BinaryFileTooLarge(DSEFileType::SMDL))?;
        self.song.nbtrks = self.trks.len() as u8;
        self.song.nbchans = self.trks.objects.iter().map(|x| x.preamble.chanid).max().ok_or(DSEError::Invalid("SMDL file contains zero tracks! Unable to automatically determine number of channels used!!".to_string()))? + 1;
        for trk in self.trks.objects.iter_mut() {
            trk.header.chunklen = trk.preamble.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32 + trk.events.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
        }
        // ======== CHUNK LABELS ========
        self.header.magicn = 0x6C646D73;  //  The 4 characters "smdl" {0x73,0x6D,0x64,0x6C} 
        self.song.label = 0x676E6F73; // Song chunk label "song" {0x73,0x6F,0x6E,0x67}
        for obj in self.trks.objects.iter_mut() {
            obj.header.label = 0x206B7274; // track chunk label "trk\0x20" {0x74,0x72,0x6B,0x20}
        }
        self.eoc.label = 0x20636F65; // the ChunkID -  The chunk ID "eoc\0x20" {0x65, 0x6F, 0x63, 0x20} 
        Ok(())
    }
}
impl ReadWrite for SMDL {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.song.write_to_file(writer)?;
        bytes_written += self.trks.write_to_file(writer)?;
        bytes_written += self.eoc.write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.song.read_from_file(reader)?;
        self.trks.set_read_params(self.song.nbtrks as usize);
        self.trks.read_from_file(reader)?;
        self.eoc.read_from_file(reader)?;
        Ok(())
    }
}

// Setup empty smdl object
pub fn create_smdl_shell(last_modified: (u16, u8, u8, u8, u8, u8, u8), mut fname: String) -> Result<SMDL, DSEError> {
    let mut smdl = SMDL::default();
    let (year, month, day, hour, minute, second, centisecond) = last_modified;

    smdl.header.version = 0x415;
    smdl.header.year = year;
    smdl.header.month = month;
    smdl.header.day = day;
    smdl.header.hour = hour;
    smdl.header.minute = minute;
    smdl.header.second = second;
    smdl.header.centisecond = centisecond;

    if !fname.is_ascii() {
        return Err(DSEError::DSEFileNameConversionNonASCII("MIDI".to_string(), fname));
    }
    fname.truncate(15);
    smdl.header.fname = DSEString::<0xFF>::try_from(fname)?;

    Ok(smdl)
}

