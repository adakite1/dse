use core::panic;
use std::fmt::{Display, Debug};
use std::io::{Read, Write, Seek, SeekFrom, Cursor};
use std::fs::File;
use std::path::Path;
use bevy_reflect::Reflect;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use phf::phf_map;
use serde::{Serialize, Deserialize};

use crate::peek_magic;
use crate::dtype::{*};
use crate::deserialize_with;
use crate::fileutils::valid_file_of_type;

pub mod sf2;

use bitflags::bitflags;

/// By default, all unknown bytes that do not have a consistent pattern of values in the EoS roms are included in the XML.
/// However, a subset of these not 100% purpose-certain bytes is 80% or something of values that have "typical" values.
/// Setting this to true will strip all those somewhat certain bytes from the Serde serialization process, and replace them
/// with their typical values.
const fn serde_use_common_values_for_unknowns<T>(_: &T) -> bool {
    true
}

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields
//// NOTE ON STRUCT XML SERIALIZATION:
/// Fields with defined values are skipped, filled with default (magic, label, etc)
/// Fields known to be the same across all the files are skipped, filled with default
/// Fields that are automatically generated read markers (like flen, chunklen, and labels) are skipped, filled with zeroes
/// Other fields that are automatically generated (ktps, etc.)
/// Fields of unknown purpose are *intentionally left alone*
/// Any other fields are also left alone

#[derive(Debug, Clone, Default, Reflect)]
pub struct DSEString<const U: u8> {
    inner: [u8; 16]
}
impl<const U: u8> TryFrom<String> for DSEString<U> {
    type Error = DSEError;

    fn try_from(value: String) -> Result<DSEString<U>, Self::Error> {
        if !value.is_ascii() {
            return Err(DSEError::DSEStringConversionNonASCII(value));
        }
        if value.as_bytes().len() > 15 {
            return Err(DSEError::DSEStringConversionLengthError(value.clone(), value.as_bytes().len()));
        }
        let mut buf: [u8; 16] = [U; 16];
        for (i, &c) in value.as_bytes().iter().chain(std::iter::once(&0x00)).enumerate() {
            buf[i] = c;
        }
        Ok(DSEString { inner: buf })
    }
}
impl<const U: u8> Display for DSEString<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", std::str::from_utf8(
            &self.inner[..self.inner.as_ref().iter().position(|&x| x == 0).expect("Invalid DSE string! Null terminator not found!!")]
        ).expect("Invalid DSE string! Non-ASCII (actually, not even UTF-8) characters found!!"))
    }
}
impl<const U: u8> AutoReadWrite for DSEString<U> {  }
impl<const U: u8> Serialize for DSEString<U> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer {
        self.to_string().serialize(serializer)
    }
}
impl<'de, const U: u8> Deserialize<'de> for DSEString<U> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        Ok(DSEString::try_from(String::deserialize(deserializer)?).unwrap())
    }
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct SWDLHeader {
    /// Note: 4-bytes represented as one u32
    #[serde(default = "GenericDefaultU32::<0x6C647773>::value")]
    #[serde(skip_serializing)]
    pub magicn: u32,
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(rename = "@unk18")]
    pub unk18: u32, // Always zeroes (hijacked for flags)
    #[serde(default)]
    #[serde(skip_serializing)]
    pub flen: u32,
    #[serde(default = "GenericDefaultU16::<0x415>::value")]
    #[serde(rename = "@version")]
    pub version: u16,
    pub unk1: u8,
    pub unk2: u8,
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk3: u32, // Always zeroes
    /// Note: 4-bytes represented as one u32
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
    pub fname: DSEString<0xAA>,

    /// Note: 4-bytes represented as one u32
    #[serde(default = "GenericDefaultU32::<0xAAAAAA00>::value")]
    #[serde(skip_serializing)]
    pub unk10: u32, // Always 0x00AA AAAA (little endian)
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk11: u32, // Always zeroes
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk12: u32, // Always zeroes
    #[serde(default = "GenericDefaultU32::<0x10>::value")]
    #[serde(skip_serializing)]
    pub unk13: u32, // Always 0x10

    #[serde(default)]
    #[serde(skip_serializing)]
    pub pcmdlen: u32, //  Length of "pcmd" chunk if there is one. If not, is null! If set to 0xAAAA0000 (The 0000 may contains something else), the file refers to samples inside an external "pcmd" chunk, inside another SWDL ! 
    /// Note: 2-bytes represented as one u16
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk14: u16, // Always zeroes (The technical documentation on Project Pokemon describes this as 4 bytes, but in my testing for bgm0016.swd at least, it's 2 bytes. I've modified it here)
    #[serde(default)]
    #[serde(skip_serializing)]
    pub nbwavislots: u16,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub nbprgislots: u16,
    pub unk17: u16,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub wavilen: u32
}
impl Default for SWDLHeader {
    fn default() -> Self {
        SWDLHeader {
            magicn: 0x6C647773,
            unk18: 0,
            flen: 0,
            version: 0x415,
            unk1: 0x00, // Random value. These just need to match with the SWD file's corresponding SMD file.
            unk2: 0xFF, // Random value. These just need to match with the SWD file's corresponding SMD file.
            unk3: 0,
            unk4: 0,
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            centisecond: 0,
            fname: DSEString::<0xAA>::default(),
            unk10: 0xAAAAAA00,
            unk11: 0,
            unk12: 0,
            unk13: 0x10,
            pcmdlen: 0,
            unk14: 0,
            nbwavislots: 0,
            nbprgislots: 0,
            unk17: 524, // I'm not sure what this is so I'll just use the value from bgm0001
            wavilen: 0
        }
    }
}
impl AutoReadWrite for SWDLHeader {  }

bitflags! {
    /// Although mostly unused within this crate, these bitflags are provided as a standard way to utilize the `unk18` value within the SWDL header.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub struct SongBuilderFlags: u32 {
        /// The WAVI chunk's pointers are extended to use 32-bit unsigned integers.
        const WAVI_POINTER_EXTENSION = 0b00000001;
        ///UNUSED!!!
        const PRGI_POINTER_EXTENSION = 0b00000010;
        ///UNUSED!!!
        const FULL_POINTER_EXTENSION = Self::WAVI_POINTER_EXTENSION.bits() | Self::PRGI_POINTER_EXTENSION.bits();
    }
}
//UNUSED BUT KEPT (Since these flags are not part of DSE itself, but an addition)
// impl ReadWrite for SongBuilderFlags {
//     fn write_to_file<W: Read + std::io::Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
//         writer.write_u32::<LittleEndian>(self.bits())?;
//         Ok(4)
//     }
//     fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
//         *self = Self::from_bits_retain(reader.read_u32::<LittleEndian>()?);
//         Ok(())
//     }
// }
impl SongBuilderFlags {
    pub fn parse_from_swdl_file<R: Read + Seek>(reader: &mut R) -> Result<SongBuilderFlags, DSEError> {
        let previous_seek_pos = reader.seek(SeekFrom::Current(0))?;
        
        let mut swdl_header = SWDLHeader::default();
        swdl_header.read_from_file(reader)?;

        reader.seek(SeekFrom::Start(previous_seek_pos))?;
        Ok(Self::from_bits_retain(swdl_header.unk18))
    }
    pub fn parse_from_swdl(swdl: &SWDL) -> SongBuilderFlags {
        Self::from_bits_retain(swdl.header.unk18)
    }
}
pub trait SetSongBuilderFlags {
    fn get_song_builder_flags(&self) -> SongBuilderFlags;
    fn set_song_builder_flags(&mut self, flags: SongBuilderFlags);
}
impl SetSongBuilderFlags for SWDL {
    fn get_song_builder_flags(&self) -> SongBuilderFlags {
        SongBuilderFlags::from_bits_retain(self.header.unk18)
    }
    fn set_song_builder_flags(&mut self, flags: SongBuilderFlags) {
        self.header.unk18 = flags.bits();
    }
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct ChunkHeader {
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing)]
    pub label: u32, // Always "wavi"  {0x77, 0x61, 0x76, 0x69} 
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk1: u16, // Always 0.
    #[serde(default = "GenericDefaultU16::<0x415>::value")]
    #[serde(skip_serializing)]
    pub unk2: u16, // Always 0x1504
    #[serde(default = "GenericDefaultU32::<0x10>::value")]
    #[serde(skip_serializing)]
    pub chunkbeg: u32, //  Seems to always be 0x10, possibly the start of the chunk data.
    #[serde(default)]
    #[serde(skip_serializing)]
    pub chunklen: u32, //  Length of the chunk data.
}
impl Default for ChunkHeader {
    fn default() -> ChunkHeader {
        ChunkHeader {
            label: 0,
            unk1: 0,
            unk2: 0x415,
            chunkbeg: 0x10,
            chunklen: 0
        }
    }
}
impl AutoReadWrite for ChunkHeader {  }

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct ADSRVolumeEnvelope {
    #[serde(rename = "@envon")]
    pub envon: bool, // Volume envelope on
    #[serde(rename = "@envmult")]
    pub envmult: u8, //  If not == 0, is used as multiplier for envelope paramters, and the 16bits lookup table is used for parameter durations. If 0, the 32bits duration lookup table is used instead. This value has no effects on volume parameters, like sustain, and atkvol. 
    
    #[serde(default = "GenericDefaultU8::<0x1>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk19: u8, // Usually 0x1
    #[serde(default = "GenericDefaultU8::<0x3>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk20: u8, // Usually 0x3
    #[serde(default = "GenericDefaultU16::<0xFF03>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk21: u16, // Usually 0x03FF (little endian -253)
    #[serde(default = "GenericDefaultU16::<0xFFFF>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk22: u16, // Usually 0xFFFF
    
    #[serde(rename = "@atkvol")]
    pub atkvol: i8, // Sample volume envelope attack volume (0-127) Higher values towards 0x7F means the volume at which the attack phase begins at is louder. Doesn't shorten the attack time. 
    #[serde(rename = "@attack")]
    pub attack: i8, // Sample volume envelope attack (0-127) 126 is ~10 secs
    #[serde(rename = "@decay")]
    pub decay: i8, // Sample volume envelope decay (0-127) Time it takes for note to fall in volume to sustain volume after hitting attack stage
    #[serde(rename = "@sustain")]
    pub sustain: i8, // Sample volume envelope sustain (0-127) Note stays at this until noteoff
    #[serde(rename = "@hold")]
    pub hold: i8, // Sample volume envelope hold (0-127) After attack, do not immediately start decaying towards the sustain level. Keep the full volume for some time based on the hold value here.
    #[serde(rename = "@decay2")]
    pub decay2: i8, // Sample volume envelope decay 2 (0-127) Time it takes for note to fade after hitting sustain volume.
    #[serde(rename = "@release")]
    pub release: i8, // Kinda similar to decay2, but I'd hazard a guess that this controls release *after* note off while `decay2` is release while the note is still pressed.
    
    #[serde(default = "GenericDefaultI8::<-1>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk57: i8 // Usually 0xFF
}
impl Default for ADSRVolumeEnvelope {
    fn default() -> Self {
        ADSRVolumeEnvelope {
            envon: false,
            envmult: 0,
            unk19: 0x1,
            unk20: 0x3,
            unk21: 0xFF03,
            unk22: 0xFFFF,
            atkvol: 0,
            attack: 0,
            decay: 0,
            sustain: 0,
            hold: 0,
            decay2: 0,
            release: 0,
            unk57: -1
        }
    }
}
impl ADSRVolumeEnvelope {
    /// Returns an alternative "default value" of `ADSRVolumeEnvelope` based on observations of common values inside the game's swdl soundtrack.
    pub fn default2() -> Self {
        let mut default = Self::default();

        // These params are the default for all samples in the WAVI section as seen from the bgm0001.swd and bgm.swd files. 
        default.envon = true;
        default.envmult = 1;
        default.atkvol = 0;
        default.attack = 0;
        default.decay = 0;
        default.sustain = 127;
        default.hold = 0;
        default.decay2 = 127;
        default.release = 40;
        
        default
    }
}
impl AutoReadWrite for ADSRVolumeEnvelope {  }

#[derive(Debug, Default, Copy, Clone, Reflect, Serialize, Deserialize)]
pub struct Tuning {
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(rename = "@ftune")]
    ftune: u8, // Pitch fine tuning, ranging from 0 to 255 with 255 representing +100 cents and 0 representing no change.
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(rename = "@ctune")]
    ctune: i8 // Coarse tuning, possibly in semitones(?). Default is -7
}
impl Tuning {
    pub fn new(ftune: u8, ctune: i8) -> Tuning {
        Tuning { ftune, ctune }
    }
    pub fn from_cents(mut cents: i64) -> Tuning {
        let mut sign = 1;
        if cents == 0 {
            return Tuning::new(0, 0);
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

        ctune = sign * ctune;
        ftune = sign * ftune;
        if ftune < 0 {
            ftune += 100;
            ctune -= 1;
        }

        let ftune = ((ftune as f64 / 100.0) * 255.0).round() as u8;

        Tuning::new(ftune, ctune as i8)
    }
    pub fn ftune(&self) -> u8 {
        self.ftune
    }
    pub fn ctune(&self) -> i8 {
        self.ctune
    }
    pub fn to_cents(&self) -> i64 {
        self.ctune as i64 * 100 + ((self.ftune as f64 / 255.0) * 100.0).round() as i64
    }
    pub fn add_semitones(&mut self, semitones: i64) {
        self.add_cents(semitones * 100);
    }
    pub fn add_cents(&mut self, cents: i64) {
        *self = Self::from_cents(self.to_cents() + cents);
    }
}
impl AutoReadWrite for Tuning {  }
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct SampleInfo {
    #[serde(default = "GenericDefaultU16::<0xAA01>::value")]
    #[serde(skip_serializing)]
    pub unk1: u16, // Entry marker? Always 0x01AA

    #[serde(rename = "@id")]
    pub id: u16,
    #[serde(flatten)]
    #[serde(rename = "@tuning")]
    pub tuning: Tuning,
    #[serde(rename = "@rootkey")]
    pub rootkey: i8, // MIDI note

    #[serde(default)]
    #[serde(skip_serializing)]
    pub ktps: i8, // Key transpose. Diff between rootkey and 60.

    #[serde(rename = "@volume")]
    pub volume: i8, // Volume of the sample.
    #[serde(rename = "@pan")]
    pub pan: i8, // Pan of the sample.

    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk5: u8, // Possibly Keygroup parameter for the sample. Always 0x00.
    #[serde(default = "GenericDefaultU8::<0x02>::value")]
    #[serde(skip_serializing)]
    pub unk58: u8, // Always 0x02
    #[serde(default = "GenericDefaultU16::<0x0000>::value")]
    #[serde(skip_serializing)]
    pub unk6: u16, // Always 0x0000
    /// Note: 2-bytes represented as one u16
    #[serde(default = "GenericDefaultU16::<0xAAAA>::value")]
    #[serde(skip_serializing)]
    pub unk7: u16, // 0xAA padding.
    #[serde(default = "GenericDefaultU16::<0x415>::value")]
    #[serde(skip_serializing)]
    pub unk59: u16, // Always 0x1504.

    #[serde(rename = "@smplfmt")]
    pub smplfmt: u16, // Sample format. 0x0000: 8-bit PCM, 0x0100: 16-bits PCM, 0x0200: 4-bits ADPCM, 0x0300: Possibly PSG
    
    #[serde(default = "GenericDefaultU8::<0x09>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk9: u8, // Often 0x09

    #[serde(rename = "@smplloop")]
    pub smplloop: bool, // true = looped, false = not looped

    #[serde(default = "GenericDefaultU16::<0x0801>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk10: u16, // Often 0x0108
    #[serde(default = "GenericDefaultU16::<0x0400>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk11: u16, // Often 0004 (Possible typo, 0x0400)
    #[serde(default = "GenericDefaultU16::<0x0101>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk12: u16, // Often 0x0101
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk13: u32, // Often 0x0000 0000

    #[serde(rename = "@smplrate")]
    pub smplrate: u32, // Sample rate in hertz
    #[serde(rename = "@smplpos")]
    pub smplpos: u32, // Offset of the sound sample in the "pcmd" chunk when there is one. Otherwise, possibly offset of the exact sample among all the sample data loaded in memory? (The value usually doesn't match the main bank's)
    #[serde(rename = "@loopbeg")]
    pub loopbeg: u32, //  The position in bytes divided by 4, the loop begins at, from smplpos. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! (For ADPCM samples, the 4 bytes preamble is counted in the loopbeg!) (P.s. the division by 4 might be because in a Stereo 16-bit PCM signal, 4 bytes is one sample (16-bit l, then 16-bit r))
    #[serde(rename = "@looplen")]
    pub looplen: u32, //  The length of the loop in bytes, divided by 4. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! 
    
    pub volume_envelope: ADSRVolumeEnvelope
}
impl Default for SampleInfo {
    fn default() -> Self {
        SampleInfo {
            unk1: 0xAA01,
            id: 0,
            tuning: Tuning::new(0, 0),
            rootkey: 0,
            ktps: 0,
            volume: 0,
            pan: 0,
            unk5: 0x00,
            unk58: 0x02,
            unk6: 0x0000,
            unk7: 0xAAAA,
            unk59: 0x415,
            smplfmt: 0x0100,
            unk9: 0x09,
            smplloop: false,
            unk10: 0x0801,
            unk11: 0x0400,
            unk12: 0x0101,
            unk13: 0,
            smplrate: 0,
            smplpos: 0,
            loopbeg: 0,
            looplen: 0,
            volume_envelope: ADSRVolumeEnvelope::default()
        }
    }
}
impl IsSelfIndexed for SampleInfo {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError> {
        self.id = new_index.try_into().map_err(|_| DSEError::Placeholder())?;
        Ok(())
    }
}
impl AutoReadWrite for SampleInfo {  }

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct ProgramInfoHeader {
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@id")]
    pub id: u16, // Index of the pointer in the pointer table. Also corresponding to the program ID in the corresponding SMDL file!
    
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing)]
    pub nbsplits: u16, // Nb of samples mapped to this preset in the split table.

    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@prgvol")]
    pub prgvol: i8, // Volume of the entire program.
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@prgpan")]
    pub prgpan: i8, // Pan of the entire program (0-127, 64 mid, 127 right, 0 left)
    
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk3: u8, // Most of the time 0x00
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default = "GenericDefaultU8::<0x0F>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub thatFbyte: u8, // Most of the time 0x0F
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default = "GenericDefaultU16::<0x200>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk4: u16, // Most of the time 0x200
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk5: u8, // Most of the time is 0x00

    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing)]
    pub nblfos: u8, // Nb of entries in the LFO table.

    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(rename = "@PadByte")]
    pub PadByte: u8, // Most of the time is 0xAA, or 0x00. Value here used as the delimiter and padding later between the LFOTable and the SplitEntryTable (and more)
    
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk7: u8, // Most of the time is 0x0
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk8: u8, // Most of the time is 0x0
    #[serde(deserialize_with = "deserialize_with::flattened_xml_attr")]
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk9: u8, // Most of the time is 0x0
}
impl Default for ProgramInfoHeader {
    fn default() -> Self {
        ProgramInfoHeader {
            id: 0,
            nbsplits: 0,
            prgvol: 0,
            prgpan: 0,
            unk3: 0,
            thatFbyte: 0x0F,
            unk4: 0x200,
            unk5: 0,
            nblfos: 0,
            PadByte: 0xAA,
            unk7: 0,
            unk8: 0,
            unk9: 0
        }
    }
}
impl IsSelfIndexed for ProgramInfoHeader {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError> {
        self.id = new_index.try_into().map_err(|_| DSEError::Placeholder())?;
        Ok(())
    }
}
impl AutoReadWrite for ProgramInfoHeader {  }

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct LFOEntry {
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk34: u8, // Unknown, usually 0x00. Does seem to have an effect with a certain combination of other values in the other parameters.
    #[serde(default)]
    #[serde(rename = "@unk52_lfo_on")]
    pub unk52: u8, // Unknown, usually 0x00. Most of the time, value is 1 when the LFO is in use.
    
    #[serde(rename = "@dest")]
    pub dest: u8, // 0x0: disabled, 0x1: pitch, 0x2: volume, 0x3: pan, 0x4: lowpass/cutoff filter?
    #[serde(rename = "@wshape")]
    pub wshape: u8, // Shape/function of the waveform. When the LFO is disabled, its always 1.
    #[serde(rename = "@rate")]
    pub rate: u16, // Rate at which the LFO "oscillate". May or may not be in Hertz.
    
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk29: u16, // uint16? Changing the value seems to induce feedback or resonance. (Corrupting engine?)
    
    #[serde(rename = "@depth")]
    pub depth: u16, // The depth parameter of the LFO.
    #[serde(rename = "@delay")]
    pub delay: u16, // Delay in ms before the LFO's effect is applied after the sample begins playing. (Per-note LFOs! So fancy!)
    
    #[serde(default)]
    #[serde(rename = "@unk32_fadeout")]
    pub unk32: u16, // Unknown, usually 0x0000. Possibly fade-out in ms.
    #[serde(default)]
    #[serde(rename = "@unk33_lowpassfreq")]
    pub unk33: u16, // Unknown, usually 0x0000. Possibly an extra parameter? Or a cutoff/lowpass filter's frequency cutoff?
}
impl Default for LFOEntry {
    fn default() -> Self {
        LFOEntry {
            unk34: 0,
            unk52: 0,
            dest: 0,
            wshape: 1,
            rate: 0,
            unk29: 0,
            depth: 0,
            delay: 0,
            unk32: 0,
            unk33: 0
        }
    }
}
impl IsSelfIndexed for LFOEntry {
    fn is_self_indexed(&self) -> Option<usize> {
        None
    }
    fn change_self_index(&mut self, _: usize) -> Result<(), DSEError> {
        Err(DSEError::Invalid("LFO entries do not have indices!!".to_string()))
    }
}
impl AutoReadWrite for LFOEntry {  }

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct SplitEntry {
    #[serde(default)]
    #[serde(skip_serializing)]
    pub unk10: u8, // A leading 0.

    #[serde(rename = "@id")]
    pub id: u8, //  The Index of the sample in the SplitsTbl! (So, a simple array with elements that reference the index of itself)
    
    #[serde(default = "GenericDefaultU8::<0x02>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk11: u8, // Unknown. Is always the same value as offset 0x1A below! (It doesn't seem to match kgrpid, so I'm wondering which byte this might be referring to:::: It refers to unk22, the one after kgrpid) (Possibly "bend range" according to assumptions made from teh DSE screenshots) (Could it maybe affect how some tracks sound if it is ever defined and we discards it?)
    #[serde(default = "GenericDefaultU8::<0x01>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk25: u8, // Unknown. Possibly a boolean.

    #[serde(rename = "@lowkey")]
    pub lowkey: i8, // Usually 0x00. Lowest MIDI key this sample can play on.
    #[serde(rename = "@hikey")]
    pub hikey: i8, // Usually 0x7F. Highest MIDI key this sample can play on.
    #[serde(default)]
    #[serde(skip_serializing)]
    pub lowkey2: i8, // A copy of lowkey, for unknown purpose.
    #[serde(default)]
    #[serde(skip_serializing)]
    pub hikey2: i8, // A copy of hikey, for unknown purpose.

    #[serde(rename = "@lovel")]
    pub lovel: i8, // Lowest note velocity the sample is played on. (0-127) (DSE has velocity layers!)
    #[serde(rename = "@hivel")]
    pub hivel: i8, // Highest note velocity the sample is played on. (0-127)
    #[serde(default)]
    #[serde(skip_serializing)]
    pub lovel2: i8, // A copy of lovel, for unknown purpose. Usually 0x00. 
    #[serde(default)]
    #[serde(skip_serializing)]
    pub hivel2: i8, // A copy of hivel, for unknown purpose. Usually 0x7F.
    
    /// Note: 4-bytes represented as one u32
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    /// 
    /// Addendum 8/1/2023: PadByte doesn't seem to always match what's in here. For example in track 43, while the padding *should* be 0x00's at 0x740, it is instead 0xAA's.
    pub unk16: u32, // Usually the same value as "PadByte", or 0. Possibly padding.
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    /// Note: 2-bytes represented as one u16
    /// 
    /// Addendum 8/1/2023: PadByte doesn't seem to always match what's in here. For example in track 43, while the padding *should* be 0x00's at 0x740, it is instead 0xAA's.
    pub unk17: u16, // Usually the same value as "PadByte", or 0. Possibly padding.
    
    #[serde(rename = "@SmplID")]
    pub SmplID: u16, // The ID/index of sample in the "wavi" chunk's lookup table.
    
    #[serde(flatten)]
    #[serde(rename = "@tuning")]
    pub tuning: Tuning,
    #[serde(rename = "@rootkey")]
    pub rootkey: i8, // Note at which the sample is sampled at!
    #[serde(default)]
    #[serde(skip_serializing)]
    pub ktps: i8, // Key transpose. Diff between rootkey and 60.

    #[serde(rename = "@smplvol")]
    pub smplvol: i8, // Volume of the sample
    #[serde(rename = "@smplpan")]
    pub smplpan: i8, // Pan of the sample
    #[serde(rename = "@kgrpid")]
    pub kgrpid: u8, // Keygroup ID of the keygroup this split belongs to!
    
    #[serde(default = "GenericDefaultU8::<0x02>::value")]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk22: u8, // Unknown, possibly a flag. Usually 0x02. Matches unk11
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk23: u16, // Unknown, usually 0000.
    /// Note: 2-bytes represented as one u16
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    /// 
    /// Addendum 8/1/2023: PadByte doesn't seem to always match what's in here. For example in track 43, while the padding *should* be 0x00's at 0x740, it is instead 0xAA's.
    pub unk24: u16, // Usually the same value as "PadByte", or 0. Possibly padding?
    // After here, the last 16 bytes are for the volume enveloped. They override the sample's original volume envelope!
    pub volume_envelope: ADSRVolumeEnvelope
}
impl Default for SplitEntry {
    fn default() -> Self {
        SplitEntry {
            unk10: 0,
            id: 0,
            unk11: 0x02,
            unk25: 0x01,
            lowkey: 0,
            hikey: 0,
            lowkey2: 0,
            hikey2: 0,
            lovel: 0,
            hivel: 0,
            lovel2: 0,
            hivel2: 0,
            unk16: 0,
            unk17: 0,
            SmplID: 0,
            tuning: Tuning::new(0, 0),
            rootkey: 0,
            ktps: 0,
            smplvol: 0,
            smplpan: 0,
            kgrpid: 0x0,
            unk22: 0x02,
            unk23: 0,
            unk24: 0,
            volume_envelope: ADSRVolumeEnvelope::default()
        }
    }
}
impl IsSelfIndexed for SplitEntry {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError> {
        self.id = new_index.try_into().map_err(|_| DSEError::Placeholder())?;
        Ok(())
    }
}
impl AutoReadWrite for SplitEntry {  }

#[derive(Debug, Clone, Default, Reflect, Serialize, Deserialize)]
pub struct _ProgramInfoDelimiter {
    pub delimiter: [u8; 16],
}
impl AutoReadWrite for _ProgramInfoDelimiter {  }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramInfo {
    #[serde(flatten)]
    pub header: ProgramInfoHeader,
    #[serde(default)]
    #[serde(skip_serializing_if = "Table::table_is_empty")]
    pub lfo_table: Table<LFOEntry>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub _delimiter: _ProgramInfoDelimiter,
    #[serde(default)]
    #[serde(skip_serializing_if = "Table::table_is_empty")]
    pub splits_table: Table<SplitEntry>
}
impl IsSelfIndexed for ProgramInfo {
    fn is_self_indexed(&self) -> Option<usize> {
        self.header.is_self_indexed()
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError> {
        self.header.change_self_index(new_index)
    }
}
impl Default for ProgramInfo {
    fn default() -> ProgramInfo {
        ProgramInfo {
            header: ProgramInfoHeader::default(),
            lfo_table: Table::new(4), // Rough estimate
            _delimiter: _ProgramInfoDelimiter::default(),
            splits_table: Table::new(8) // Rough estimate
        }
    }
}
impl ReadWrite for ProgramInfo {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.lfo_table.write_to_file(writer)?;
        // bytes_written += self._delimiter.write_to_file(writer)?;
        bytes_written += vec![self.header.PadByte; 16].write_to_file(writer)?;
        if self.splits_table.objects.len() == 256 {
            return Err(DSEError::Invalid("A preset has more than 255 sample mappings (in fact it has exactly 256, one more than the maximum)! If the tool works, the final file will still play silence! Reduce the number of samples used by editing the MIDI to solve this.".to_string()));
        }
        bytes_written += self.splits_table.write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.lfo_table.set_read_params(self.header.nblfos as usize);
        self.lfo_table.read_from_file(reader)?;
        self._delimiter.read_from_file(reader)?;
        self.splits_table.set_read_params(self.header.nbsplits as usize);
        self.splits_table.read_from_file(reader)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Reflect, Serialize, Deserialize)]
pub struct Keygroup {
    #[serde(rename = "@id")]
    pub id: u16, // Index/ID of the keygroup
    #[serde(rename = "@poly")]
    pub poly: i8, // Polyphony. Max number of simultaneous notes played. 0 to 15. -1 means disabled. (Technical documentation describes this field as unsigned, but I've switched it to signed since -1 is off instead of 255 being off)
    #[serde(rename = "@priority")]
    pub priority: u8, // Priority over the assignment of voice channels for members of this group. 0-possibly 99, default is 8. Higher is higeher priority.
    #[serde(rename = "@vclow")]
    pub vclow: i8, // Lowest voice channel the group may use. Usually between 0 and 15
    #[serde(rename = "@vchigh")]
    pub vchigh: i8, // Highest voice channel this group may use. 0-15 (While not explicitly stated in the documentation, this value being i8 makes sense as the first keygroup typically has this set to 255 which makes more sense interpreted as -1 disabled)
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk50: u8, // Unown
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_use_common_values_for_unknowns")]
    pub unk51: u8, // Unknown
}
impl IsSelfIndexed for Keygroup {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError> {
        self.id = new_index.try_into().map_err(|_| DSEError::Placeholder())?;
        Ok(())
    }
}
impl AutoReadWrite for Keygroup {  }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WAVIChunk {
    #[serde(default)]
    #[serde(skip_serializing)]
    _read_n: usize,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub header: ChunkHeader,
    pub data: PointerTable<SampleInfo>
}
impl WAVIChunk {
    pub fn new(nbwavislots: usize) -> WAVIChunk {
        WAVIChunk {
            _read_n: nbwavislots,
            header: ChunkHeader::default(),
            data: PointerTable::new(nbwavislots, 0) // Temporarily 0
        }
    }
    pub fn set_read_params(&mut self, nbwavislots: usize) {
        self._read_n = nbwavislots;
    }
}
impl WAVIChunk {
    pub fn write_to_file<P: Pointer<LittleEndian>, W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file::<P, _>(writer).map_err(|e| match e {
            DSEError::Placeholder() => DSEError::PointerTableTooLarge(DSEBlockType::SwdlWavi),
            _ => e
        })?)
    }
    pub fn read_from_file<P: Pointer<LittleEndian>, R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file::<P, _>(reader)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRGIChunk {
    #[serde(default)]
    #[serde(skip_serializing)]
    _read_n: usize,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub header: ChunkHeader,
    pub data: PointerTable<ProgramInfo>
}
impl PRGIChunk {
    pub fn new(nbprgislots: usize) -> PRGIChunk {
        PRGIChunk {
            _read_n: nbprgislots,
            header: ChunkHeader::default(),
            data: PointerTable::new(nbprgislots, 0) // Temporarily 0
        }
    }
    pub fn set_read_params(&mut self, nbprgislots: usize) {
        self._read_n = nbprgislots;
    }
}
impl PRGIChunk {
    pub fn write_to_file<P: Pointer<LittleEndian>, W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file::<P, _>(writer).map_err(|e| match e {
            DSEError::Placeholder() => DSEError::PointerTableTooLarge(DSEBlockType::SwdlPrgi),
            _ => e
        })?)
    }
    pub fn read_from_file<P: Pointer<LittleEndian>, R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file::<P, _>(reader)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Reflect, Serialize, Deserialize)]
pub struct _KeygroupsSampleDataDelimiter {
    pub delimiter: [u8; 8],
}
impl AutoReadWrite for _KeygroupsSampleDataDelimiter {  }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KGRPChunk {
    #[serde(default)]
    #[serde(skip_serializing)]
    pub header: ChunkHeader,
    pub data: Table<Keygroup>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub _padding: Option<_KeygroupsSampleDataDelimiter>
}
impl Default for KGRPChunk {
    fn default() -> KGRPChunk {
        KGRPChunk {
            header: ChunkHeader::default(),
            data: Table::new(0),
            _padding: None
        }
    }
}
impl ReadWrite for KGRPChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)? + if self.data.objects.len() % 2 == 1 { vec![0x67, 0xC0, 0x40, 0x00, 0x88, 0x00, 0xFF, 0x04].write_to_file(writer)? } else { 0 })
        // Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)? + if let Some(pad) = &self._padding { pad.write_to_file(writer)? } else { 0 })
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self.header.chunklen as usize / 8);
        self.data.read_from_file(reader)?;
        self._padding = Some(_KeygroupsSampleDataDelimiter::default());
        self._padding.as_mut().unwrap().read_from_file(reader)?;
        // "pcmd" {0x70, 0x63, 0x6D, 0x64}
        // "eod\20" {0x65, 0x6F, 0x64, 0x20}
        if &self._padding.as_ref().unwrap().delimiter[..4] == &[0x70, 0x63, 0x6D, 0x64] ||
            &self._padding.as_ref().unwrap().delimiter[..4] == &[0x65, 0x6F, 0x64, 0x20] {
            self._padding = None;
            reader.seek(SeekFrom::Current(-8))?;
        }
        Ok(())
    }
}

mod base64 {
    use serde::{Serialize, Deserialize};
    use serde::{Deserializer, Serializer};
    use base64::{Engine as _, engine::general_purpose};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = general_purpose::STANDARD.encode(v);
        String::serialize(&base64, s)
    }
    
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        general_purpose::STANDARD.decode(base64)
            .map_err(|e| serde::de::Error::custom(e))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PCMDChunk {
    #[serde(default)]
    #[serde(skip_serializing)]
    pub header: ChunkHeader,
    #[serde(with = "base64")]
    pub data: Vec<u8>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub _padding: Vec<u8>
}
impl Default for PCMDChunk {
    fn default() -> Self {
        PCMDChunk {
            header: ChunkHeader::default(),
            data: Vec::new(),
            _padding: Vec::new()
        }
    }
}
impl ReadWrite for PCMDChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let len = self.header.write_to_file(writer)? + self.data.write_to_file(writer)?;
        let len_aligned = ((len - 1) | 15) + 1; // Round the length of the pcmd chunk in bytes to the next multiple of 16
        let padding_zero = len_aligned - len;
        for _ in 0..padding_zero {
            writer.write_u8(0)?;
        }
        Ok(len_aligned)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data = vec![0; self.header.chunklen as usize];
        self.data.read_from_file(reader)?;
        // EOD\20 {0x65, 0x6F, 0x64, 0x20}
        while peek_magic!(reader)? != [0x65, 0x6F, 0x64, 0x20] {
            self._padding.push(reader.read_u8()?);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SWDL {
    pub header: SWDLHeader,
    pub wavi: WAVIChunk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prgi: Option<PRGIChunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kgrp: Option<KGRPChunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pcmd: Option<PCMDChunk>,
    #[serde(default = "SWDL::generate_eod_chunk_header")]
    #[serde(skip_serializing)]
    pub _eod: ChunkHeader
}
impl DSELinkBytes for SWDL {
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
impl SWDL {
    pub fn generate_eod_chunk_header() -> ChunkHeader {
        let mut eod = ChunkHeader::default();
        eod.label = 0x20646F65; //  "eod\20" {0x65, 0x6F, 0x64, 0x20} 
        eod
    }
    pub fn set_metadata(&mut self, last_modified: (u16, u8, u8, u8, u8, u8, u8), mut fname: String) -> Result<(), DSEError> {
        let (year, month, day, hour, minute, second, centisecond) = last_modified;
        
        self.header.version = 0x415;
        self.header.year = year;
        self.header.month = month;
        self.header.day = day;
        self.header.hour = hour;
        self.header.minute = minute;
        self.header.second = second;
        self.header.centisecond = centisecond;

        if !fname.is_ascii() {
            return Err(DSEError::DSEFileNameConversionNonASCII("SWD".to_string(), fname));
        }
        fname.truncate(15);
        self.header.fname = DSEString::<0xAA>::try_from(fname)?;

        Ok(())
    }
    /// Regenerate length, slots, and nb parameters. To keep this working, `write_to_file` should never attempt to read or seek beyond alotted frame, which is initial cursor position and beyond.
    pub fn regenerate_read_markers<PWavi: Pointer<LittleEndian>, PPrgi: Pointer<LittleEndian>>(&mut self) -> Result<(), DSEError> { //TODO: make more efficient
        // ======== NUMERICAL VALUES (LENGTHS, SLOTS, etc) ========
        self.header.flen = self.write_to_file::<PWavi, PPrgi, _>(&mut Cursor::new(&mut Vec::new()))?.try_into().map_err(|_| DSEError::BinaryFileTooLarge(DSEFileType::SWDL))?;
        println!("flen {}", self.header.flen);
        if self.header.pcmdlen & 0xFFFF0000 == 0xAAAA0000 && self.pcmd.is_none() {
            // Expected case of separation with main bank. Noop
        } else if let Some(pcmd) = &mut self.pcmd {
            self.header.pcmdlen = pcmd.data.len().try_into().map_err(|_| DSEError::BinaryBlockTooLarge(DSEFileType::SWDL, DSEBlockType::SwdlPcmd))?;
            pcmd.header.chunklen = self.header.pcmdlen;
        } else {
            // By default, assume that if the file does not contain a bank of its own, that the samples it refers to are in the main bank
            self.header.pcmdlen = 0xAAAA0000;
        }
        self.header.nbwavislots = self.wavi.data.slots().try_into().map_err(|_| DSEError::PointerTableTooLong(DSEBlockType::SwdlWavi))?;
        self.header.nbprgislots = self.prgi.as_ref().map(|prgi| prgi.data.slots().try_into().map_err(|_| DSEError::PointerTableTooLong(DSEBlockType::SwdlPrgi))).unwrap_or(Ok(128))?; // In the main bank, this is set to 128 even though there is no prgi chunk
        self.header.wavilen = self.wavi.data.write_to_file::<PWavi, _>(&mut Cursor::new(&mut Vec::new())).map_err(|e| match e {
            DSEError::Placeholder() => DSEError::PointerTableTooLarge(DSEBlockType::SwdlWavi),
            _ => e
        })?.try_into().map_err(|_| DSEError::BinaryBlockTooLarge(DSEFileType::SWDL, DSEBlockType::SwdlWavi))?;
        self.wavi.header.chunklen = self.header.wavilen;
        if let Some(prgi) = &mut self.prgi {
            prgi.header.chunklen = prgi.data.write_to_file::<PPrgi, _>(&mut Cursor::new(&mut Vec::new())).map_err(|e| match e {
                DSEError::Placeholder() => DSEError::PointerTableTooLarge(DSEBlockType::SwdlPrgi),
                _ => e
            })?.try_into().map_err(|_| DSEError::BinaryBlockTooLarge(DSEFileType::SWDL, DSEBlockType::SwdlPrgi))?;
            for (i, obj) in prgi.data.objects.iter_mut().enumerate() {
                obj.header.nbsplits = obj.splits_table.len().try_into().map_err(|_| DSEError::TableTooLong(DSEBlockType::SwdlPrgiProgramInfoSplits(i)))?;
                obj.header.nblfos = obj.lfo_table.len().try_into().map_err(|_| DSEError::TableTooLong(DSEBlockType::SwdlPrgiProgramInfoLfos(i)))?;
            }
        }
        if let Some(kgrp) = &mut self.kgrp {
            kgrp.header.chunklen = kgrp.data.write_to_file(&mut Cursor::new(&mut Vec::new()))?.try_into().map_err(|_| DSEError::BinaryBlockTooLarge(DSEFileType::SWDL, DSEBlockType::SwdlKgrp))?;
        }
        // ======== CHUNK LABELS ========
        self.header.magicn = 0x6C647773;
        self.wavi.header.label = 0x69766177; // "wavi"  {0x77, 0x61, 0x76, 0x69}
        if let Some(prgi) = &mut self.prgi {
            prgi.header.label = 0x69677270; //  "prgi" {0x70, 0x72, 0x67, 0x69} 
        }
        if let Some(kgrp) = &mut self.kgrp {
            kgrp.header.label = 0x7072676B; //  "kgrp" {0x6B, 0x67, 0x72, 0x70} 
        }
        if let Some(pcmd) = &mut self.pcmd {
            pcmd.header.label = 0x646D6370; //  "pcmd" {0x70, 0x63, 0x6D, 0x64} 
        }
        // self._eod.label = 0x20646F65; //  "eod\20" {0x65, 0x6F, 0x64, 0x20} 
        Ok(())
    }
    /// Regenerate automatic parameters.
    pub fn regenerate_automatic_parameters(&mut self) -> Result<(), DSEError> {
        // ======== SAMPLEINFO ========
        for sample_info in self.wavi.data.objects.iter_mut() {
            sample_info.ktps = 60 - sample_info.rootkey; // Note: what does DSE need ktps for though?
        }
        // ======== SPLITS ========
        if let Some(prgi) = &mut self.prgi {
            for program_info in prgi.data.objects.iter_mut() {
                for split_entry in program_info.splits_table.objects.iter_mut() {
                    split_entry.lowkey2 = split_entry.lowkey;
                    split_entry.hikey2 = split_entry.hikey;
                    split_entry.lovel2 = split_entry.lovel;
                    split_entry.hivel2 = split_entry.hivel;
                    if serde_use_common_values_for_unknowns(&()) {
                        split_entry.unk16 = (&[program_info.header.PadByte; 4][..]).read_u32::<LittleEndian>()?;
                        split_entry.unk17 = (&[program_info.header.PadByte; 2][..]).read_u16::<LittleEndian>()?;
                        split_entry.unk24 = (&[program_info.header.PadByte; 2][..]).read_u16::<LittleEndian>()?;
                    }
                    split_entry.ktps = 60 - split_entry.rootkey;
                }
            }
        }
        Ok(())
    }
}
impl Default for SWDL {
    fn default() -> SWDL {
        SWDL {
            header: SWDLHeader::default(),
            wavi: WAVIChunk::new(0),
            prgi: None,
            kgrp: None,
            pcmd: None,
            _eod: ChunkHeader::default()
        }
    }
}
impl SWDL {
    pub fn write_to_file<PWavi: Pointer<LittleEndian>, PPrgi: Pointer<LittleEndian>, W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.wavi.write_to_file::<PWavi, _>(writer)?;
        bytes_written += if let Some(prgi) = &self.prgi { prgi.write_to_file::<PPrgi, _>(writer)? } else { 0 };
        bytes_written += if let Some(kgrp) = &self.kgrp { kgrp.write_to_file(writer)? } else { 0 };
        bytes_written += if let Some(pcmd) = &self.pcmd { pcmd.write_to_file(writer)? } else { 0 };
        bytes_written += SWDL::generate_eod_chunk_header().write_to_file(writer)?;
        Ok(bytes_written)
    }
    pub fn read_from_file<PWavi: Pointer<LittleEndian>, PPrgi: Pointer<LittleEndian>, R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        // WAVI
        self.wavi.set_read_params(self.header.nbwavislots as usize);
        self.wavi.read_from_file::<PWavi, _>(reader)?;
        // PRGI {0x70, 0x72, 0x67, 0x69}
        if peek_magic!(reader)? == [0x70, 0x72, 0x67, 0x69] {
            let mut tmp = PRGIChunk::new(self.header.nbprgislots as usize);
            tmp.read_from_file::<PPrgi, _>(reader)?;
            self.prgi = Some(tmp);
        }
        // KGRP {0x6B, 0x67, 0x72, 0x70}
        if peek_magic!(reader)? == [0x6B, 0x67, 0x72, 0x70] {
            let mut tmp = KGRPChunk::default();
            tmp.read_from_file(reader)?;
            self.kgrp = Some(tmp);
        }
        // PCMD {0x70, 0x63, 0x6D, 0x64}
        if peek_magic!(reader)? == [0x70, 0x63, 0x6D, 0x64] {
            let mut tmp = PCMDChunk::default();
            tmp.read_from_file(reader)?;
            self.pcmd = Some(tmp);
        }
        // EOD\20 {0x65, 0x6F, 0x64, 0x20}
        self._eod.read_from_file(reader)?;
        Ok(())
    }
}
impl SWDL {
    pub fn load<R: Read + Seek>(file: &mut R) -> Result<SWDL, DSEError> {
        let flags = SongBuilderFlags::parse_from_swdl_file(file)?;

        let mut swdl = SWDL::default();
        if flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
            swdl.read_from_file::<u32, u32, _>(file)?;
        } else if flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
            swdl.read_from_file::<u32, u16, _>(file)?;
        } else if flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
            swdl.read_from_file::<u16, u32, _>(file)?;
        } else {
            swdl.read_from_file::<u16, u16, _>(file)?;
        }

        Ok(swdl)
    }
    pub fn load_xml<R: Read + Seek>(file: &mut R) -> Result<SWDL, DSEError> {
        let mut st = String::new();
        file.read_to_string(&mut st)?;
        let mut swdl = quick_xml::de::from_str::<SWDL>(&st)?;

        let flags = SongBuilderFlags::parse_from_swdl(&swdl);

        if flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
            swdl.regenerate_read_markers::<u32, u32>()?;
        } else if flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
            swdl.regenerate_read_markers::<u32, u16>()?;
        } else if flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
            swdl.regenerate_read_markers::<u16, u32>()?;
        } else {
            swdl.regenerate_read_markers::<u16, u16>()?;
        }

        swdl.regenerate_automatic_parameters()?;

        Ok(swdl)
    }
    pub fn load_path<P: AsRef<Path> + Debug>(path: P) -> Result<SWDL, DSEError> {
        let swdl;
        if valid_file_of_type(&path, "swd") {
            println!("[*] Opening bank {:?}", &path);
            swdl = SWDL::load(&mut File::open(path)?)?;
        } else if valid_file_of_type(&path, "xml") {
            println!("[*] Opening bank {:?} (xml)", &path);
            swdl = SWDL::load_xml(&mut File::open(path)?)?;
        } else {
            return Err(DSEError::Invalid(format!("File '{:?}' is not an SWD file!", path)));
        }
        Ok(swdl)
    }
    pub fn save<W: Read + Write + Seek>(&mut self, file: &mut W, flags: SongBuilderFlags) -> Result<(), DSEError> {
        self.set_song_builder_flags(flags);
        if flags.contains(SongBuilderFlags::FULL_POINTER_EXTENSION) {
            self.regenerate_read_markers::<u32, u32>()?;
            self.regenerate_automatic_parameters()?;
            self.write_to_file::<u32, u32, _>(file)?;
        } else if flags.contains(SongBuilderFlags::WAVI_POINTER_EXTENSION) {
            self.regenerate_read_markers::<u32, u16>()?;
            self.regenerate_automatic_parameters()?;
            self.write_to_file::<u32, u16, _>(file)?;
        } else if flags.contains(SongBuilderFlags::PRGI_POINTER_EXTENSION) {
            self.regenerate_read_markers::<u16, u32>()?;
            self.regenerate_automatic_parameters()?;
            self.write_to_file::<u16, u32, _>(file)?;
        } else {
            self.regenerate_read_markers::<u16, u16>()?;
            self.regenerate_automatic_parameters()?;
            self.write_to_file::<u16, u16, _>(file)?;
        }
        Ok(())
    }
}

pub static BUILT_IN_SAMPLE_RATE_ADJUSTMENT_TABLE: phf::Map<u32, i64> = phf_map! {
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

// https://projectpokemon.org/docs/mystery-dungeon-nds/dse-swdl-format-r14/#SWDL_Header
pub const LOOKUP_TABLE_20_B0_F50: [i16; 128] = [
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
pub fn lookup_env_time_value_i16(msec: i16) -> i8 {
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
pub const LOOKUP_TABLE_20_B1050: [i32; 128] = [
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
pub fn lookup_env_time_value_i32(msec: i32) -> i8 {
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

pub fn create_swdl_shell(last_modified: (u16, u8, u8, u8, u8, u8, u8), fname: String) -> Result<SWDL, DSEError> {
    let mut track_swdl = SWDL::default();
    track_swdl.set_metadata(last_modified, fname)?;
    Ok(track_swdl)
}

