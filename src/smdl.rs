use core::panic;
use std::io::{Read, Write, Seek, SeekFrom};
use bevy_reflect::Reflect;
use byteorder::{ReadBytesExt, WriteBytesExt};

use crate::{peek_magic, peek_byte};
use crate::dtype::{*};

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields

#[derive(Debug, Default, Reflect)]
pub struct SMDLHeader {
    pub magicn: [u8; 4],
    pub unk7: [u8; 4], // Always zeroes
    pub flen: u32,
    pub version: u16,
    pub unk1: u8,
    pub unk2: u8,
    pub unk3: [u8; 4], // Always zeroes
    pub unk4: [u8; 4], // Always zeroes
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub centisecond: u8, // unsure
    pub fname: [u8; 16],
    pub unk5: u32, // Unknown, usually 0x1
    pub unk6: u32, // Unknown, usually 0x1
    pub unk8: u32, // Unknown, usually 0xFFFFFFFF
    pub unk9: u32, // Unknown, usually 0xFFFFFFFF
}
impl AutoReadWrite for SMDLHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct SongChunk {
    pub label: [u8; 4], // Song chunk label "song" {0x73,0x6F,0x6E,0x67}
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

#[derive(Debug, Default, Reflect)]
pub struct TrkChunkHeader {
    pub label: [u8; 4], // track chunk label "trk\0x20" {0x74,0x72,0x6B,0x20}
    pub param1: u32, // usually 0x01000000
    pub param2: u32, // usually 0x0000FF04
    pub chunklen: u32, // length of the trk chunk. starting after this field to the first 0x98 event encountered in the track. length is in bytes like its swdl counterpart.
}
impl AutoReadWrite for TrkChunkHeader {  }
#[derive(Debug, Default, Reflect)]
pub struct TrkChunkPreamble {
    pub trkid: u8, // the track id of the track. a number between 0 and 0x11
    pub chanid: u8, // the channel id of the track. a number between 0 and 0x0F?
    pub unk1: u8, // often 0
    pub unk2: u8, // often 0
}
impl AutoReadWrite for TrkChunkPreamble {  }

mod events {
    use byteorder::{ReadBytesExt, LittleEndian, WriteBytesExt};
    use ux::u24;

    use crate::dtype::ReadWrite;

    #[derive(Debug, Default)]
    pub struct PlayNote {
        velocity: u8,
        nbparambytes: u8,
        octavemod: u8,
        note: u8,
        keydownduration: u24
    }
    impl ReadWrite for PlayNote {
        fn write_to_file<W: std::io::Read + std::io::Write + std::io::Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
            writer.write_u8(self.velocity)?;
            let note_data = self.nbparambytes << 6 + self.octavemod << 4 + self.note;
            writer.write_u8(note_data)?;
            let mut keydownduration = [0_u8; 4];
            (&mut keydownduration[..]).write_u32::<LittleEndian>(self.keydownduration.into())?;
            writer.write_all(&keydownduration[..3])?;
            Ok(5)
        }
        fn read_from_file<R: std::io::Read + std::io::Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
            self.velocity = reader.read_u8()?;
            let note_data = reader.read_u8()?;
            self.nbparambytes = (note_data & 0b11000000) >> 6;
            self.octavemod = (note_data & 0b00110000) >> 4;
            self.note = note_data & 0b00001111;
            let mut keydownduration = [0_u8; 4];
            for i in 0..self.nbparambytes as usize {
                keydownduration[i] = reader.read_u8()?;
            }
            self.keydownduration = (&keydownduration[..]).read_u32::<LittleEndian>()?.try_into().unwrap();
            Ok(())
        }
    }

    #[derive(Debug, Default)]
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
}

#[derive(Debug)]
pub enum DSEEvent {
    PlayNote(events::PlayNote)
}
impl ReadWrite for DSEEvent {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        match self {
            DSEEvent::PlayNote(event) => event.write_to_file(writer)
        }
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        match peek_byte!(reader)? {
            0x0..=0x7F => {
                let mut event = events::PlayNote::default();
                event.read_from_file(reader)?;
                *self = DSEEvent::PlayNote(event);
            },
            0x80..=0x8F => todo!(),
            _ => todo!()
        }
        Ok(())
    }
}








#[derive(Debug)]
pub struct SMDL {

}