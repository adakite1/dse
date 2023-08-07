use core::panic;
use std::io::{Read, Write, Seek, SeekFrom, Cursor};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use serde::{Serialize, Deserialize};

use crate::swdl::{ADSRVolumeEnvelope, DSEString};

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


/// Generic Error to represent a variety of errors
#[derive(Debug, Clone)]
pub struct GenericError(String);
impl GenericError {
    pub fn new(message: &str) -> GenericError {
        GenericError(String::from(message))
    }
}
impl std::fmt::Display for GenericError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}
impl std::error::Error for GenericError {  }

pub trait AutoReadWrite: Reflect + Struct + Default {  }
pub trait ReadWrite {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>>;
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>>;
}
impl<T: Reflect + Struct + Default + AutoReadWrite> ReadWrite for T {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = 0;
        for field_i in 0..self.field_len() {
            let field = self.field_at(field_i).ok_or("Failed to get field!")?;
            let type_info = field.get_represented_type_info().ok_or("Failed to get type info of field!")?;
            match type_info {
                bevy_reflect::TypeInfo::Array(array_info) => {
                    let capacity = array_info.capacity();
                    if array_info.item_type_name() == "u8" {
                        if capacity == 2 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 2]>().ok_or("Error in bevy_reflect!")?)?;
                            bytes_written += 2;
                        } else if capacity == 4 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 4]>().ok_or("Error in bevy_reflect!")?)?;
                            bytes_written += 4;
                        } else if capacity == 8 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 8]>().ok_or("Error in bevy_reflect!")?)?;
                            bytes_written += 8;
                        } else if capacity == 16 {
                            writer.write_all(field.as_any().downcast_ref::<[u8; 16]>().ok_or("Error in bevy_reflect!")?)?;
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
                        writer.write_u8(*field.as_any().downcast_ref::<bool>().ok_or("Error in bevy_reflect!")? as u8)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "u8" {
                        writer.write_u8(*field.as_any().downcast_ref::<u8>().ok_or("Error in bevy_reflect!")?)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "u16" {
                        writer.write_u16::<LittleEndian>(*field.as_any().downcast_ref::<u16>().ok_or("Error in bevy_reflect!")?)?;
                        bytes_written += 2;
                    } else if value_info.type_name() == "u32" {
                        writer.write_u32::<LittleEndian>(*field.as_any().downcast_ref::<u32>().ok_or("Error in bevy_reflect!")?)?;
                        bytes_written += 4;
                    } else if value_info.type_name() == "i8" {
                        writer.write_i8(*field.as_any().downcast_ref::<i8>().ok_or("Error in bevy_reflect!")?)?;
                        bytes_written += 1;
                    } else if value_info.type_name() == "i16" {
                        writer.write_i16::<LittleEndian>(*field.as_any().downcast_ref::<i16>().ok_or("Error in bevy_reflect!")?)?;
                        bytes_written += 2;
                    } else if value_info.type_name() == "i32" {
                        writer.write_i32::<LittleEndian>(*field.as_any().downcast_ref::<i32>().ok_or("Error in bevy_reflect!")?)?;
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
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                _ => panic!("Unsupported auto type!")
            }
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, file: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        for field_i in 0..self.field_len() {
            let field = self.field_at_mut(field_i).ok_or("Failed to get field!")?;
            let type_info = field.get_represented_type_info().ok_or("Failed to get type info of field!")?;
            match type_info {
                bevy_reflect::TypeInfo::Array(array_info) => {
                    let capacity = array_info.capacity();
                    if array_info.item_type_name() == "u8" {
                        if capacity == 2 {
                            *field.as_any_mut().downcast_mut::<[u8; 2]>().ok_or("Error in bevy_reflect!")? = read_n_bytes!(file, 2)?;
                        } else if capacity == 4 {
                            *field.as_any_mut().downcast_mut::<[u8; 4]>().ok_or("Error in bevy_reflect!")? = read_n_bytes!(file, 4)?;
                        } else if capacity == 8 {
                            *field.as_any_mut().downcast_mut::<[u8; 8]>().ok_or("Error in bevy_reflect!")? = read_n_bytes!(file, 8)?;
                        } else if capacity == 16 {
                            *field.as_any_mut().downcast_mut::<[u8; 16]>().ok_or("Error in bevy_reflect!")? = read_n_bytes!(file, 16)?;
                        } else {
                            panic!("Unsupported auto type!");
                        }
                    } else {
                        panic!("Unsupported auto type!");
                    }
                },
                bevy_reflect::TypeInfo::Value(value_info) => {
                    if value_info.type_name() == "bool" {
                        *field.as_any_mut().downcast_mut::<bool>().ok_or("Error in bevy_reflect!")? = file.read_u8()? != 0;
                    } else if value_info.type_name() == "u8" {
                        *field.as_any_mut().downcast_mut::<u8>().ok_or("Error in bevy_reflect!")? = file.read_u8()?;
                    } else if value_info.type_name() == "u16" {
                        *field.as_any_mut().downcast_mut::<u16>().ok_or("Error in bevy_reflect!")? = file.read_u16::<LittleEndian>()?;
                    } else if value_info.type_name() == "u32" {
                        *field.as_any_mut().downcast_mut::<u32>().ok_or("Error in bevy_reflect!")? = file.read_u32::<LittleEndian>()?;
                    } else if value_info.type_name() == "i8" {
                        *field.as_any_mut().downcast_mut::<i8>().ok_or("Error in bevy_reflect!")? = file.read_i8()?;
                    } else if value_info.type_name() == "i16" {
                        *field.as_any_mut().downcast_mut::<i16>().ok_or("Error in bevy_reflect!")? = file.read_i16::<LittleEndian>()?;
                    } else if value_info.type_name() == "i32" {
                        *field.as_any_mut().downcast_mut::<i32>().ok_or("Error in bevy_reflect!")? = file.read_i32::<LittleEndian>()?;
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
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(writer.write_all(&self).map(|_| self.len())?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>>;
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = 0;
        for (i, object) in self.objects.iter().enumerate() {
            if let Some(self_index) = object.is_self_indexed() {
                if self_index != i {
                    panic!("Table<T> write_to_file: Self-index of object {} is {}. The self-index of an object in a table must match its actual index in the table!!", i, self_index);
                }
            }
            bytes_written += object.write_to_file(writer)?;
        }
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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
impl<T: ReadWrite + Default + IsSelfIndexed + Serialize> ReadWrite for PointerTable<T> {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let pointer_table_byte_len = self.slots() * 2;
        let pointer_table_byte_len_aligned = ((pointer_table_byte_len - 1) | 15) + 1; // Round the length of the pointer table in bytes to the next multiple of 16
        let first_pointer = pointer_table_byte_len_aligned;
        let mut accumulated_write = 0;
        let mut accumulated_object_data: Vec<u8> = Vec::new();
        let mut accumulated_object_data_cursor = Cursor::new(&mut accumulated_object_data);
        let pointer_table_start = writer.seek(SeekFrom::Current(0))?;
        writer.write_all(&vec![0; pointer_table_byte_len as usize])?;
        for (i, val) in self.objects.iter().enumerate() {
            let i = val.is_self_indexed().unwrap_or(i);
            writer.seek(SeekFrom::Start(pointer_table_start + i as u64 * 2))?;
            if writer.read_u16::<LittleEndian>()? == 0 {
                // Pointer has not been written in yet
                writer.seek(SeekFrom::Current(-2))?;
                writer.write_u16::<LittleEndian>((first_pointer + accumulated_write).try_into()?)?;
            } else {
                // Overlapping pointers!
                panic!("PointerTable<T> write_to_file: The self-index of an object in a pointer table must be unique!!")
            }
            accumulated_write += val.write_to_file(&mut accumulated_object_data_cursor)?;
        }
        let padding_aa = pointer_table_byte_len_aligned - pointer_table_byte_len;
        writer.seek(SeekFrom::End(0))?;
        for _ in 0..padding_aa {
            writer.write_u8(0xAA)?;
        }
        writer.write_all(&accumulated_object_data)?;
        Ok(pointer_table_byte_len_aligned + accumulated_object_data.len())
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        let start_of_pointer_table = reader.seek(SeekFrom::Current(0))?;
        for i in 0..self._read_n {
            let twobyte_offset_from_start_of_pointer_table = reader.read_u16::<LittleEndian>()?;
            if twobyte_offset_from_start_of_pointer_table != 0 {
                reader.seek(SeekFrom::Start(start_of_pointer_table + twobyte_offset_from_start_of_pointer_table as u64))?;
                let mut object = T::default();
                object.read_from_file(reader)?;
                reader.seek(SeekFrom::Start(start_of_pointer_table + (i as u64 + 1) * 2))?;
                self.objects.push(object);
            }
        }
        reader.seek(SeekFrom::Start(start_of_pointer_table + self._chunk_len as u64))?; // Set the file cursor to after the entire chunk
        Ok(())
    }
}