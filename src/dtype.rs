use core::panic;
use std::{io::{Read, Write, Seek, SeekFrom, Cursor}, fmt::{Display, Debug}, vec, ops::RangeInclusive};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian, ByteOrder};
use num_traits::{Zero, AsPrimitive};
use serde::{Serialize, Deserialize};

use crate::{swdl::{ADSRVolumeEnvelope, DSEString, Tuning, SWDLHeader, SWDL}, smdl::{SMDLHeader, SMDL}};

use thiserror::Error;

use strum::Display;

use bitflags::bitflags;

bitflags! {
    /// Although mostly unused within this crate, these bitflags are provided as a standard way to utilize the `unk18` value within the SWDL header.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub struct SongBuilderFlags: u32 {
        /// The WAVI chunk's pointers are extended to use 32-bit unsigned integers.
        const WAVI_POINTER_EXTENSION = 0b00000001;
        /// The PRGI chunk's pointers are extended to use 32-bit unsigned integers.
        const PRGI_POINTER_EXTENSION = 0b00000010;
        /// A combination of `WAVI_POINTER_EXTENSION` and `PRGI_POINTER_EXTENSION`.
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

    pub fn parse_from_smdl_file<R: Read + Seek>(reader: &mut R) -> Result<SongBuilderFlags, DSEError> {
        let previous_seek_pos = reader.seek(SeekFrom::Current(0))?;
        
        let mut smdl_header = SMDLHeader::default();
        smdl_header.read_from_file(reader)?;

        reader.seek(SeekFrom::Start(previous_seek_pos))?;
        Ok(Self::from_bits_retain(smdl_header.unk7))
    }
    pub fn parse_from_smdl(smdl: &SMDL) -> SongBuilderFlags {
        Self::from_bits_retain(smdl.header.unk7)
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
impl SetSongBuilderFlags for SMDL {
    fn get_song_builder_flags(&self) -> SongBuilderFlags {
        SongBuilderFlags::from_bits_retain(self.header.unk7)
    }
    fn set_song_builder_flags(&mut self, flags: SongBuilderFlags) {
        self.header.unk7 = flags.bits();
    }
}

#[derive(Debug, Display)]
pub enum DSEFileType {
    SWDL,
    SMDL
}
#[derive(Debug, Display)]
pub enum DSEBlockType {
    Header,
    SwdlWavi,
    SwdlPrgi,
    SwdlPrgiProgramInfoSplits(usize),
    SwdlPrgiProgramInfoLfos(usize),
    SwdlKgrp,
    SwdlPcmd,
    SwdlEoD,
    SmdlTrkEvents(usize),
}

pub trait DSEWrappableError: std::error::Error + Display + Debug {  }
impl<E> DSEWrappableError for E
where
    E: std::error::Error + Display + Debug {  }
#[derive(Error, Debug)]
pub enum DSEError {
    // #[error("data store disconnected")]
    // Disconnect(#[from] io::Error),
    // #[error("the data for key `{0}` is not available")]
    // Redaction(String),
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader {
    //     expected: String,
    //     found: String,
    // },
    // #[error("unknown data store error")]
    // Unknown,

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Deserialize Error: {0}")]
    DeserializeError(#[from] quick_xml::DeError),
    #[error("SoundFont Parse Error: {0}")]
    SoundFontParseError(String),
    #[error("MIDI Parse Error: {0}")]
    SmfParseError(String),
    #[error("Glob Pattern Error: {0}")]
    GlobPatternError(#[from] glob::PatternError),
    // Intended for outside callers
    #[error("Wrapped Error: {0}")]
    Wrapper(Box<dyn DSEWrappableError>),
    // Intended for outside callers
    #[error("Wrapped Error: {0}")]
    WrapperString(String),

    #[error("Failed to find sample {0} at {1}.")]
    SampleFindError(String, u64),
    #[error("Failed to read sample {0} at {1}, expected sample length is {2} bytes.")]
    SampleReadError(String, u64, usize),
    #[error("Target sample rate {0} unsupported by the lookup table! Cannot determine its adjustment value!!")]
    SampleRateUnsupported(f64),

    #[error("{0}")]
    Invalid(String),
    #[error("DSE command '{0}' is invalid! {1}")]
    InvalidDSECommand(String, String),
    #[error("Failed to parse switch track command '{0}'!")]
    InvalidDSECommandFailedToParseTrkChange(String),
    #[error("DSE command '{0}' specifies {1} bytes of arguments, but DSE event {2} takes {3} bytes of arguments!")]
    InvalidDSECommandArguments(String, usize, String, usize),
    #[error("DSE command '{0}' specifies a typed argument '{1}', but it could not be parsed as that type! ({2})")]
    InvalidDSECommandTypedArgument(String, String, String),
    #[error("DSE command '{0}' specifies a typed argument '{1}' with an unknown type '{2}'!")]
    InvalidDSECommandUnknownType(String, String, String),
    #[error("Couldn't convert filename for {0} file with path '{1}' into a UTF-8 Rust String. Filenames should be pure-ASCII only!")]
    DSEFileNameConversionNonUTF8(String, String),
    #[error("Couldn't convert song name '{1}' into an ASCII string for setting {0} metadata! Song names should be pure-ASCII only!")]
    DSEFileNameConversionNonASCII(String, String),
    #[error("Cannot create `DSEString` from the provided value '{0}'! String contains non-ASCII characters!")]
    DSEStringConversionNonASCII(String),
    #[error("Cannot create `DSEString` from the provided value '{0}'! String contains more than 15 characters! ({1} characters)")]
    DSEStringConversionLengthError(String, usize),
    #[error("Invalid other event code '{0}'! It's not within acceptable range!")]
    DSEEventLookupError(u8),
    #[error("Invalid other event name '{0}'!!")]
    DSEEventNameLookupError(String),
    #[error("Only ticks/beat is supported currently as a timing specifier!")]
    DSESmfUnsupportedTimingSpecifier(),
    #[error("Sequencial MIDI files are not supported!")]
    DSESequencialSmfUnsupported(),
    #[error("MIDI contains too many tracks to be converted to the Smf0 format!")]
    DSESmf0TooManyTracks(),
    #[error("Invalid used voice channels range {0:?}! Range must be bounded inside [0, 15], with the vchigh optionally being -1, interpreted as the max 15.")]
    DSEUsedVoiceChannelsRangeOutOfBounds(RangeInclusive<i8>),
    #[error("Invalid used voice channels range {0:?}! The range end must be greater than or equal to the range start!")]
    DSEUsedVoiceChannelsRangeFlipped(RangeInclusive<i8>),
    #[error("Table<T> write_to_file: Self-index of object {0} is {0}. The self-index of an object in a table must match its actual index in the table!!")]
    TableNonMatchingSelfIndex(usize, usize),
    #[error("PointerTable<T> write_to_file: The self-index of an object in a pointer table must be unique!!")]
    PointerTableDuplicateSelfIndex(),
    #[error("SWDL must contain a prgi chunk!")]
    DSESmdConverterSwdEmpty(),

    #[error("Couldn't export as a binary {0} file! The final file was too large!!")]
    BinaryFileTooLarge(DSEFileType),
    #[error("Couldn't export as a binary {0} file! The {1} chunk was too large!")]
    BinaryBlockTooLarge(DSEFileType, DSEBlockType),
    #[error("The table for the {0} chunk contains too many slots!!")]
    TableTooLong(DSEBlockType),
    #[error("The pointer table for the {0} chunk contains too many slots!!")]
    PointerTableTooLong(DSEBlockType),
    #[error("The pointer table for the {0} chunk is too large, resulting in some pointers into the table overflowing!!")]
    PointerTableTooLarge(DSEBlockType),
    #[error("MIDI messages too far apart to be converted into the Smf0 format!")]
    DSESmf0MessagesTooFarApart(),
    #[error("Some notes are too long to be converted!")]
    DSESmfNotesTooLong(),

    // Internal errors: these should theoretically never happen
    #[error("Seek failed!")]
    _InMemorySeekFailed(),
    #[error("Write failed!")]
    _InMemoryWriteFailed(),
    #[error("Valid dynamic field access failed!")]
    _ValidDynamicFieldAccessFailed(),
    #[error("Valid dynamic field downcast failed!")]
    _ValidDynamicFieldDowncastFailed(),
    #[error("`bevy_reflect` dynamic type info access failed!")]
    _DynamicTypeInfoAccessFailed(),
    #[error("Failed to read the filename of file '{0}'!")]
    _FileNameReadFailed(String),
    #[error("Failed to remove a valid key from a HashMap!")]
    _ValidHashMapKeyRemovalFailed(),
    #[error("Corresponding note on event with known index missing!")]
    _CorrespondingNoteOnNotFound(),
    #[error("Sample {0} specified in a preset is missing from `sample_infos`!")]
    _SampleInPresetMissing(u16),

    // Intended for use when a function wants to delegate the elaboration of an error to its parent caller
    #[error("Parent caller should have overwritten this")]
    Placeholder()
}

#[repr(i8)]
pub enum DSEPan {
    FullLeft = 0,
    Middle = 64,
    FullRight = 127
}

macro_rules! read_n_bytes {
    ($file:ident, $n:literal) => {{
        let mut buf: [u8; $n] = [0; $n];
        $file.read_exact(&mut buf).map(|_| buf)
    }};
}
#[macro_export]
macro_rules! peek_magic {
    ($file:ident) => {{
        let mut buf: [u8; 4] = [0; 4];
        $file.read_exact(&mut buf).and_then(|_| {
            $file.seek(SeekFrom::Current(-4))
        }).map(move |_| buf)
    }};
}
#[macro_export]
macro_rules! peek_byte {
    ($file:ident) => {{
        let mut buf: [u8; 1] = [0; 1];
        $file.read_exact(&mut buf).and_then(|_| {
            $file.seek(SeekFrom::Current(-1))
        }).map(move |_| buf[0])
    }};
}

pub struct GenericDefaultI8<const U: i8>;
impl<const U: i8> GenericDefaultI8<U> {
    pub fn value() -> i8 {
        U
    }
}
pub struct GenericDefaultU8<const U: u8>;
impl<const U: u8> GenericDefaultU8<U> {
    pub fn value() -> u8 {
        U
    }
}
pub struct GenericDefaultU16<const U: u16>;
impl<const U: u16> GenericDefaultU16<U> {
    pub fn value() -> u16 {
        U
    }
}
pub struct GenericDefaultU32<const U: u32>;
impl<const U: u32> GenericDefaultU32<U> {
    pub fn value() -> u32 {
        U
    }
}

pub struct GenericDefaultByteArray<const V: u8, const U: usize>;
impl<const V: u8, const U: usize> GenericDefaultByteArray<V, U> {
    pub fn value() -> [u8; U] {
        [V; U]
    }
}

pub trait AutoReadWrite: Reflect + Struct + Default {  }
pub trait ReadWrite {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError>;
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError>;
}
impl<T: Reflect + Struct + Default + AutoReadWrite> ReadWrite for T {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = 0;
        for field_i in 0..self.field_len() {
            let field = self.field_at(field_i).ok_or(DSEError::_ValidDynamicFieldAccessFailed())?;
            let type_info = field.get_represented_type_info().ok_or(DSEError::_DynamicTypeInfoAccessFailed())?;
            match type_info {
                bevy_reflect::TypeInfo::Array(array_info) => {
                    let capacity = array_info.capacity();
                    if array_info.item_type_name() == "u8" {
                        if capacity == 2 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 2]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                            bytes_written += 2;
                        } else if capacity == 4 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 4]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                            bytes_written += 4;
                        } else if capacity == 8 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 8]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                            bytes_written += 8;
                        } else if capacity == 16 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 16]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                            bytes_written += 16;
                        } else {
                            panic!("Unsupported auto type!");
                        }
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                bevy_reflect::TypeInfo::Value(value_info) => {
                    if value_info.type_name() == "bool" {
                        writer.write_u8(*field.as_any().downcast_ref::<bool>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? as u8)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "u8" {
                        writer.write_u8(*field.as_any().downcast_ref::<u8>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "u16" {
                        writer.write_u16::<LittleEndian>(*field.as_any().downcast_ref::<u16>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 2;
                    } else if value_info.type_name() == "u32" {
                        writer.write_u32::<LittleEndian>(*field.as_any().downcast_ref::<u32>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 4;
                    } else if value_info.type_name() == "i8" {
                        writer.write_i8(*field.as_any().downcast_ref::<i8>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "i16" {
                        writer.write_i16::<LittleEndian>(*field.as_any().downcast_ref::<i16>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 2;
                    } else if value_info.type_name() == "i32" {
                        writer.write_i32::<LittleEndian>(*field.as_any().downcast_ref::<i32>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())?)?;
                        bytes_written += 4;
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                bevy_reflect::TypeInfo::Struct(_) => {
                    if let Some(vol_envelope) = field.as_any().downcast_ref::<ADSRVolumeEnvelope>() {
                        bytes_written += vol_envelope.write_to_file(writer)?;
                    } else if let Some(dse_string) = field.as_any().downcast_ref::<DSEString<0xAA>>() {
                        bytes_written += dse_string.write_to_file(writer)?;
                    } else if let Some(dse_string) = field.as_any().downcast_ref::<DSEString<0xFF>>() {
                        bytes_written += dse_string.write_to_file(writer)?;
                    } else if let Some(tuning) = field.as_any().downcast_ref::<Tuning>() {
                        bytes_written += tuning.write_to_file(writer)?;
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                _ => panic!("Unsupported auto type!")
            }
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, file: &mut R) -> Result<(), DSEError> {
        for field_i in 0..self.field_len() {
            let field = self.field_at_mut(field_i).ok_or(DSEError::_ValidDynamicFieldAccessFailed())?;
            let type_info = field.get_represented_type_info().ok_or(DSEError::_DynamicTypeInfoAccessFailed())?;
            match type_info {
                bevy_reflect::TypeInfo::Array(array_info) => {
                    let capacity = array_info.capacity();
                    if array_info.item_type_name() == "u8" {
                        if capacity == 2 {
                            *field.as_any_mut().downcast_mut::<[u8; 2]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = read_n_bytes!(file, 2)?;
                        } else if capacity == 4 {
                            *field.as_any_mut().downcast_mut::<[u8; 4]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = read_n_bytes!(file, 4)?;
                        } else if capacity == 8 {
                            *field.as_any_mut().downcast_mut::<[u8; 8]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = read_n_bytes!(file, 8)?;
                        } else if capacity == 16 {
                            *field.as_any_mut().downcast_mut::<[u8; 16]>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = read_n_bytes!(file, 16)?;
                        } else {
                            panic!("Unsupported auto type!");
                        }
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                bevy_reflect::TypeInfo::Value(value_info) => {
                    if value_info.type_name() == "bool" {
                        *field.as_any_mut().downcast_mut::<bool>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_u8()? != 0;
                    } else if value_info.type_name() == "u8" {
                        *field.as_any_mut().downcast_mut::<u8>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_u8()?;
                    } else if value_info.type_name() == "u16" {
                        *field.as_any_mut().downcast_mut::<u16>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_u16::<LittleEndian>()?;
                    } else if value_info.type_name() == "u32" {
                        *field.as_any_mut().downcast_mut::<u32>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_u32::<LittleEndian>()?;
                    } else if value_info.type_name() == "i8" {
                        *field.as_any_mut().downcast_mut::<i8>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_i8()?;
                    } else if value_info.type_name() == "i16" {
                        *field.as_any_mut().downcast_mut::<i16>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_i16::<LittleEndian>()?;
                    } else if value_info.type_name() == "i32" {
                        *field.as_any_mut().downcast_mut::<i32>().ok_or(DSEError::_ValidDynamicFieldDowncastFailed())? = file.read_i32::<LittleEndian>()?;
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                bevy_reflect::TypeInfo::Struct(_) => {
                    if let Some(vol_envelope) = field.as_any_mut().downcast_mut::<ADSRVolumeEnvelope>() {
                        vol_envelope.read_from_file(file)?;
                    } else if let Some(dse_string) = field.as_any_mut().downcast_mut::<DSEString<0xAA>>() {
                        dse_string.read_from_file(file)?;
                    } else if let Some(dse_string) = field.as_any_mut().downcast_mut::<DSEString<0xFF>>() {
                        dse_string.read_from_file(file)?;
                    } else if let Some(tuning) = field.as_any_mut().downcast_mut::<Tuning>() {
                        tuning.read_from_file(file)?;
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                _ => panic!("Unsupported auto type!")
            }
        }
        Ok(())
    }
}

/// Binary blob
impl ReadWrite for Vec<u8> {
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, DSEError> {
        Ok(writer.write_all(&self).map(|_| self.len())?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        Ok(reader.read_exact(self)?)
    }
}

/// Trait indicating that the type implementing it indexes itself
/// Its behavior changes with the data type of the storage.
/// 
/// If it is a `Table`,
///  the code will assume that all the self-indices are in order and start writing,
///  but **will** panic the moment the self-index does not match the actual index 
///  of the object in the talbe.
/// 
/// If it is a `PointerTable`,
///  the self-index will be preserved as sparseness is allowed in this data type.
///  However, if one or more index conflicts emerge, the code **will** panic.
pub trait IsSelfIndexed {
    fn is_self_indexed(&self) -> Option<usize>;
    fn change_self_index(&mut self, new_index: usize) -> Result<(), DSEError>;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Table<T: ReadWrite + Default + IsSelfIndexed + Serialize> {
    /// ONLY USE AS THE NUMBER OF OBJECTS TO READ!!! USE objects.len() INSTEAD OUTSIDE OF read_from_file!!!
    #[serde(default)]
    #[serde(skip_serializing)]
    _read_n: usize,
    #[serde(rename = "o")]
    pub objects: Vec<T>
}
impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> Table<T> {
    pub fn table_is_empty(table: &Table<T>) -> bool {
        table.len() == 0
    }
    pub fn new(n: usize) -> Table<T> {
        Table { _read_n: n, objects: Vec::with_capacity(n) }
    }
    pub fn set_read_params(&mut self, n: usize) {
        self._read_n = n;
    }
    pub fn len(&self) -> usize {
        self.objects.len()
    }
}
impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> ReadWrite for Table<T> {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let mut bytes_written = 0;
        for (i, object) in self.objects.iter().enumerate() {
            if let Some(self_index) = object.is_self_indexed() {
                if self_index != i {
                    return Err(DSEError::TableNonMatchingSelfIndex(i, self_index));
                }
            }
            bytes_written += object.write_to_file(writer)?;
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        for _ in 0..self._read_n {
            let mut object = T::default();
            object.read_from_file(reader)?;
            self.objects.push(object);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointerTable<T: ReadWrite + Default + IsSelfIndexed + Serialize> {
    /// ONLY USE AS THE NUMBER OF OBJECTS TO READ!!! USE objects.len() INSTEAD OUTSIDE OF read_from_file!!!
    #[serde(default)]
    #[serde(skip_serializing)]
    _read_n: usize,
    #[serde(default)]
    #[serde(skip_serializing)]
    _chunk_len: u32,
    #[serde(rename = "o")]
    pub objects: Vec<T>
}
impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> PointerTable<T> {
    pub fn new(n: usize, chunk_len: u32) -> PointerTable<T> {
        PointerTable { _read_n: n, _chunk_len: chunk_len, objects: Vec::with_capacity(n) }
    }
    pub fn set_read_params(&mut self, n: usize, chunk_len: u32) {
        self._read_n = n;
        self._chunk_len = chunk_len;
    }
    pub fn slots(&self) -> usize {
        if self.objects.len() == 0 {
            return 0;
        }
        if let Some(_) = self.objects[0].is_self_indexed() {
            self.objects.iter().map(|x| x.is_self_indexed().unwrap()).max().unwrap() + 1
        } else {
            self.objects.len()
        }
    }
    pub fn last(&self) -> Option<&T> {
        if let Some(_) = self.objects[0].is_self_indexed() {
            self.objects.iter().map(|x| (x, x.is_self_indexed().unwrap())).max_by_key(|x| x.1).map(|x| x.0)
        } else {
            self.objects.last()
        }
    }
}
pub trait Pointer<O: ByteOrder>: AsPrimitive<u64> + TryFrom<usize> + Eq + Zero {
    fn pointer_size() -> usize;
    fn read_from_bytes(buf: &[u8]) -> Self;
    fn read<R: Read>(reader: &mut R) -> Result<Self, std::io::Error>;
    fn write_as_bytes(self, buf: &mut [u8]);
    fn write<W: Write>(self, writer: &mut W) -> Result<(), std::io::Error>;
    fn use_magic() -> Option<Self>;
}
impl<O: ByteOrder> Pointer<O> for u16 {
    fn pointer_size() -> usize {
        std::mem::size_of::<u16>()
    }
    fn read_from_bytes(buf: &[u8]) -> u16 {
        O::read_u16(buf)
    }
    fn read<R: Read>(reader: &mut R) -> Result<u16, std::io::Error> {
        reader.read_u16::<O>()
    }
    fn write_as_bytes(self, buf: &mut [u8]) {
        O::write_u16(buf, self)
    }
    fn write<W: Write>(self, writer: &mut W) -> Result<(), std::io::Error> {
        writer.write_u16::<O>(self)
    }
    fn use_magic() -> Option<u16> {
        None
    }
}
impl<O: ByteOrder> Pointer<O> for u32 {
    fn pointer_size() -> usize {
        std::mem::size_of::<u32>()
    }
    fn read_from_bytes(buf: &[u8]) -> u32 {
        O::read_u32(buf)
    }
    fn read<R: Read>(reader: &mut R) -> Result<u32, std::io::Error> {
        reader.read_u32::<O>()
    }
    fn write_as_bytes(self, buf: &mut [u8]) {
        O::write_u32(buf, self)
    }
    fn write<W: Write>(self, writer: &mut W) -> Result<(), std::io::Error> {
        writer.write_u32::<O>(self)
    }
    fn use_magic() -> Option<u32> {
        Some(u32::MAX)
    }
}
impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> PointerTable<T> {
    pub fn write_to_file<P: Pointer<LittleEndian>, W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
        let bytes_per_pointer = P::pointer_size();
        let pointer_table_byte_len = if P::use_magic().is_some() {
            (self.slots() + 1) * bytes_per_pointer
        } else {
            self.slots() * bytes_per_pointer
        };
        let pointer_table_byte_len_aligned = ((pointer_table_byte_len - 1) | 15) + 1; // Round the length of the pointer table in bytes to the next multiple of 16
        let first_pointer = pointer_table_byte_len_aligned;
        let mut accumulated_write = 0;
        let mut accumulated_object_data: Vec<u8> = Vec::new();
        let mut accumulated_object_data_cursor = Cursor::new(&mut accumulated_object_data);
        let pointer_table_start = writer.seek(SeekFrom::Current(0))?;
        writer.write_all(&vec![0; pointer_table_byte_len as usize])?;
        if let Some(magic) = P::use_magic() {
            writer.seek(SeekFrom::Start(pointer_table_start))?;
            magic.write(writer)?;
        }
        for (i, val) in self.objects.iter().enumerate() {
            let i = val.is_self_indexed().unwrap_or(i) + P::use_magic().is_some() as usize;
            writer.seek(SeekFrom::Start(pointer_table_start + i as u64 * bytes_per_pointer as u64))?;
            if P::read(writer)? == P::zero() {
                // Pointer has not been written in yet
                writer.seek(SeekFrom::Current(-(bytes_per_pointer as i64)))?;
                println!("{} pointer", first_pointer + accumulated_write);
                let p: P = (first_pointer + accumulated_write).try_into().map_err(|_| DSEError::Placeholder())?;
                p.write(writer)?;
            } else {
                // Overlapping pointers!
                return Err(DSEError::PointerTableDuplicateSelfIndex())
            }
            accumulated_write += val.write_to_file(&mut accumulated_object_data_cursor)?;
        }
        let padding_aa = pointer_table_byte_len_aligned - pointer_table_byte_len;
        writer.seek(SeekFrom::End(0))?;
        for _ in 0..padding_aa {
            writer.write_u8(0xAA)?;
        }
        writer.write_all(&accumulated_object_data)?;
        println!("==============================");
        Ok(pointer_table_byte_len_aligned + accumulated_object_data.len())
    }
    pub fn read_from_file<P: Pointer<LittleEndian>, R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
        let bytes_per_pointer = P::pointer_size();
        let start_of_pointer_table = reader.seek(SeekFrom::Current(0))?;
        if P::use_magic().is_some() {
            reader.seek(SeekFrom::Current(bytes_per_pointer as i64))?;
        }
        let start = P::use_magic().is_some() as usize;
        for i in start..(self._read_n + start) {
            let nbyte_offset_from_start_of_pointer_table = P::read(reader)?;
            if nbyte_offset_from_start_of_pointer_table != P::zero() {
                reader.seek(SeekFrom::Start(start_of_pointer_table + nbyte_offset_from_start_of_pointer_table.as_()))?;
                let mut object = T::default();
                object.read_from_file(reader)?;
                reader.seek(SeekFrom::Start(start_of_pointer_table + (i as u64 + 1) * bytes_per_pointer as u64))?;
                self.objects.push(object);
            }
        }
        reader.seek(SeekFrom::Start(start_of_pointer_table + self._chunk_len as u64))?; // Set the file cursor to after the entire chunk
        Ok(())
    }
}
// impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> ReadWrite for PointerTable<T> {
//     fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, DSEError> {
//         self.write_to_file::<u16, _>(writer)
//     }
//     fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), DSEError> {
//         self.read_from_file::<u16, _>(reader)
//     }
// }

/// Trait defining the getters and setters for the DSE link bytes
pub trait DSELinkBytes {
    fn get_link_bytes(&self) -> (u8, u8);
    fn set_link_bytes(&mut self, link_bytes: (u8, u8));
    fn set_unk1(&mut self, unk1: u8);
    fn set_unk2(&mut self, unk2: u8);
}

