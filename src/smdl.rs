use core::panic;
use std::io::{Read, Write, Seek, SeekFrom, Cursor};
use bevy_reflect::Reflect;
use byteorder::{ReadBytesExt, WriteBytesExt};
use serde::{Serialize, Deserialize};

use crate::swdl::DSEString;
use crate::peek_byte;
use crate::dtype::*;
use crate::deserialize_with;

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields

#[derive(Debug, Default, Reflect, Serialize, Deserialize)]
pub struct SMDLHeader {
    pub magicn: u32,
    pub unk7: u32, // Always zeroes
    pub flen: u32,
    pub version: u16,
    pub unk1: u8, // There's also two consecutive bytes in the SWDL header with unknown purposes, could it be?? Could this be the link byte described by @nazberrypie in Trezer???!?
    pub unk2: u8,
    pub unk3: u32, // Always zeroes
    pub unk4: u32, // Always zeroes
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub centisecond: u8, // unsure
    pub fname: DSEString,
    pub unk5: u32, // Unknown, usually 0x1
    pub unk6: u32, // Unknown, usually 0x1
    pub unk8: u32, // Unknown, usually 0xFFFFFFFF
    pub unk9: u32, // Unknown, usually 0xFFFFFFFF
}
impl AutoReadWrite for SMDLHeader {  }

#[derive(Debug, Default, Reflect, Serialize, Deserialize)]
pub struct SongChunk {
    pub label: u32, // Song chunk label "song" {0x73,0x6F,0x6E,0x67}
    pub unk1: u32, // usually 0x1
    pub unk2: u32, // usually 0xFF10
    pub unk3: u32, // usually 0xFFFFFFB0
    pub unk4: u16, // usually 0x1
    pub tpqn: u16, // ticks per quarter note (usually 48 which is consistent with the MIDI standard)
    pub unk5: u16, // usually 0xFF01
    pub nbtrks: u8, // number of track(trk) chunks
    pub nbchans: u8, // number of channels (unsure of how channels work in DSE)
    pub unk6: u32, // usually 0x0f000000
    pub unk7: u32, // usually 0xffffffff
    pub unk8: u32, // usually 0x40000000
    pub unk9: u32, // usually 0x00404000
    pub unk10: u16, // usually 0x0200
    pub unk11: u16, // usually 0x0800
    pub unk12: u32, // usually 0xffffff00
    pub unkpad: [u8; 16], // unknown sequence of 16 0xFF bytes
}
impl AutoReadWrite for SongChunk {  }

#[derive(Debug, Default, Reflect, Serialize, Deserialize)]
pub struct TrkChunkHeader {
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@label")]
    pub label: u32, // track chunk label "trk\0x20" {0x74,0x72,0x6B,0x20}
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@param1")]
    pub param1: u32, // usually 0x01000000
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@param2")]
    pub param2: u32, // usually 0x0000FF04
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@chunklen")]
    pub chunklen: u32, // length of the trk chunk. starting after this field to the first 0x98 event encountered in the track. length is in bytes like its swdl counterpart.
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
    #[serde(rename = "@unk1")]
    pub unk1: u8, // often 0
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@unk2")]
    pub unk2: u8, // often 0
}
impl AutoReadWrite for TrkChunkPreamble {  }

mod events {
    use byteorder::{ReadBytesExt, LittleEndian, WriteBytesExt};
    use serde::{Serialize, Deserialize};

    use crate::dtype::{ReadWrite, GenericError};

    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct PlayNote {
        velocity: u8,
        #[serde(default)]
        #[serde(skip_serializing)]
        _nbparambytes: u8,
        octavemod: u8,
        note: u8,
        keydownduration: u32
    }
    impl ReadWrite for PlayNote {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
            writer.write_u8(self.velocity)?;

            let mut keydownduration = [0_u8; 4];
            if self.keydownduration > 0xFFFFFF {
                return Err(Box::new(GenericError::new("Keydown duration needs to be within the range 0 to 0xFFFFFF")))?;
            }
            (&mut keydownduration[..]).write_u32::<LittleEndian>(self.keydownduration)?;
            let mut keydowndurationlen = 0_u8;
            for &b in keydownduration.iter() {
                if b != 0x00 {
                    keydowndurationlen += 1;
                } else {
                    break;
                }
            }

            let note_data = (keydowndurationlen << 6) + (self.octavemod << 4) + self.note;
            writer.write_u8(note_data)?;
            writer.write_all(&keydownduration[..keydowndurationlen as usize])?;
            Ok(2 + keydowndurationlen as usize)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
            self.velocity = reader.read_u8()?;
            let note_data = reader.read_u8()?;
            self._nbparambytes = (note_data & 0b11000000) >> 6;
            self.octavemod = (note_data & 0b00110000) >> 4;
            self.note = note_data & 0b00001111;
            let mut keydownduration = [0_u8; 4];
            for i in 0..self._nbparambytes as usize {
                keydownduration[i] = reader.read_u8()?;
            }
            self.keydownduration = (&keydownduration[..]).read_u32::<LittleEndian>()?;
            Ok(())
        }
    }

    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct FixedDurationPause {
        duration: u8,
    }
    impl ReadWrite for FixedDurationPause {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
            writer.write_u8(self.duration)?;
            Ok(1)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
            self.duration = reader.read_u8()?;
            Ok(())
        }
    }

    const PARAMETERS_COUNT: [u8; 112] = [0, 1, 1, 2, 3, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 1, 1, 0, 1, 0, 0, 3, 0, 1, 1, 1, 2, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 3, 2, 2, 2, 2, 0, 0, 1, 5, 4, 0, 1, 1, 1, 3, 1, 5, 4, 0, 1, 1, 1, 3, 0, 5, 4, 0, 1, 5, 4, 2, 3, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    #[derive(Debug, Default, Serialize, Deserialize)]
    pub struct Other {
        code: u8,
        parameters: [u8; 3]
    }
    impl Other {
        pub fn is_eot_event(&self) -> bool {
            self.code == 0x98
        }
    }
    impl ReadWrite for Other {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
            let nbparams = PARAMETERS_COUNT[self.code as usize - 0x90];
            writer.write_u8(self.code)?;
            writer.write_all(&self.parameters[..nbparams as usize])?;
            Ok(1 + nbparams as usize)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
            self.code = reader.read_u8()?;
            let nbparams = PARAMETERS_COUNT[self.code as usize - 0x90];
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        match self {
            DSEEvent::PlayNote(event) => event.write_to_file(writer),
            DSEEvent::FixedDurationPause(event) => event.write_to_file(writer),
            DSEEvent::Other(event) => event.write_to_file(writer)
        }
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrkEvents {
    #[serde(rename = "$value")]
    pub events: Vec<DSEEvent>,
}
impl ReadWrite for TrkEvents {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = 0;
        for event in &self.events {
            bytes_written += event.write_to_file(writer)?;
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        let mut read_event = || -> Result<DSEEvent, Box<dyn std::error::Error>> {
            let mut event = DSEEvent::default();
            event.read_from_file(reader)?;
            Ok(event)
        };
        self.events.push(read_event()?);
        while !self.events.last().unwrap().is_eot_event() {
            self.events.push(read_event()?);
        }
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrkChunk {
    #[serde(flatten)]
    pub header: TrkChunkHeader,
    #[serde(flatten)]
    pub preamble: TrkChunkPreamble,
    pub events: TrkEvents,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub _padding: Vec<u8>
}
impl ReadWrite for TrkChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
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
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.preamble.read_from_file(reader)?;
        self.events.read_from_file(reader)?;
        while peek_byte!(reader)? == 0x98 {
            self._padding.push(reader.read_u8()?);
        }
        Ok(())
    }
}
impl IsSelfIndexed for TrkChunk {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.preamble.trkid as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.preamble.trkid = new_index.try_into()?;
        Ok(())
    }
}

#[derive(Debug, Reflect, Serialize, Deserialize)]
pub struct EOCChunk {
    pub label: u32, // the ChunkID -  The chunk ID "eoc\0x20" {0x65, 0x6F, 0x63, 0x20} 
    pub param1: u32, //  Unknown meaning, is often 0x00000001. 
    pub param2: u32, //  Unknown meaning, is often 0x04FF0000. 
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
    pub _eoc: EOCChunk
}
impl SMDL {
    pub fn regenerate_read_markers(&mut self) -> Result<(), Box<dyn std::error::Error>> { //TODO: make more efficient
        self.header.flen = self.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
        self.song.nbtrks = self.trks.len() as u8;
        for trk in self.trks.objects.iter_mut() {
            trk.header.chunklen = trk.preamble.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32 + trk.events.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32 - trk._padding.len() as u32;
        }
        Ok(())
    }
}
impl ReadWrite for SMDL {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.song.write_to_file(writer)?;
        bytes_written += self.trks.write_to_file(writer)?;
        bytes_written += EOCChunk::default().write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.song.read_from_file(reader)?;
        self.trks.set_read_params(self.song.nbtrks as usize);
        self.trks.read_from_file(reader)?;
        self._eoc.read_from_file(reader)?;
        Ok(())
    }
}