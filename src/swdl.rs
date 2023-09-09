use core::panic;
use std::fmt::Display;
use std::io::{Read, Write, Seek, SeekFrom, Cursor};
use bevy_reflect::Reflect;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use serde::{Serialize, Deserialize};

use crate::peek_magic;
use crate::dtype::{*};
use crate::deserialize_with;

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
    #[serde(skip_serializing)]
    pub unk18: u32, // Always zeroes
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
        default.atkvol = 127; // Modified slightly to match SF2 defaults
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
impl ReadWrite for WAVIChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file(reader)?;
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
impl ReadWrite for PRGIChunk {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file(reader)?;
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
    /// Regenerate length, slots, and nb parameters. To keep this working, `write_to_file` should never attempt to read or seek beyond alotted frame, which is initial cursor position and beyond.
    pub fn regenerate_read_markers(&mut self) -> Result<(), DSEError> { //TODO: make more efficient
        // ======== NUMERICAL VALUES (LENGTHS, SLOTS, etc) ========
        self.header.flen = self.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
        println!("flen {}", self.header.flen);
        if self.header.pcmdlen & 0xFFFF0000 == 0xAAAA0000 && self.pcmd.is_none() {
            // Expected case of separation with main bank. Noop
        } else if let Some(pcmd) = &mut self.pcmd {
            self.header.pcmdlen = pcmd.data.len() as u32;
            pcmd.header.chunklen = pcmd.data.len() as u32;
        } else {
            // By default, assume that if the file does not contain a bank of its own, that the samples it refers to are in the main bank
            self.header.pcmdlen = 0xAAAA0000;
        }
        self.header.nbwavislots = self.wavi.data.slots() as u16;
        self.header.nbprgislots = self.prgi.as_ref().map(|prgi| prgi.data.slots() as u16).unwrap_or(128); // In the main bank, this is set to 128 even though there is no prgi chunk
        self.header.wavilen = self.wavi.data.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
        self.wavi.header.chunklen = self.header.wavilen;
        if let Some(prgi) = &mut self.prgi {
            prgi.header.chunklen = prgi.data.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
            for obj in prgi.data.objects.iter_mut() {
                obj.header.nbsplits = obj.splits_table.len() as u16;
                obj.header.nblfos = obj.lfo_table.len() as u8;
            }
        }
        if let Some(kgrp) = &mut self.kgrp {
            kgrp.header.chunklen = kgrp.data.write_to_file(&mut Cursor::new(&mut Vec::new()))? as u32;
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
impl ReadWrite for SWDL {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.wavi.write_to_file(writer)?;
        bytes_written += if let Some(prgi) = &self.prgi { prgi.write_to_file(writer)? } else { 0 };
        bytes_written += if let Some(kgrp) = &self.kgrp { kgrp.write_to_file(writer)? } else { 0 };
        bytes_written += if let Some(pcmd) = &self.pcmd { pcmd.write_to_file(writer)? } else { 0 };
        bytes_written += SWDL::generate_eod_chunk_header().write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        self.header.read_from_file(reader)?;
        // WAVI
        self.wavi.set_read_params(self.header.nbwavislots as usize);
        self.wavi.read_from_file(reader)?;
        // PRGI {0x70, 0x72, 0x67, 0x69}
        if peek_magic!(reader)? == [0x70, 0x72, 0x67, 0x69] {
            let mut tmp = PRGIChunk::new(self.header.nbprgislots as usize);
            tmp.read_from_file(reader)?;
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