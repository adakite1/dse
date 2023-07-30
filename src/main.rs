use core::panic;
use std::{fs::File, io::Read};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian, LittleEndian};

macro_rules! read_n_bytes {
    ($file:ident, $n:literal) => {{
        let mut buf: [u8; $n] = [0; $n];
        $file.read_exact(&mut buf).map(|_| buf)
    }};
}

#[derive(Debug)]
pub struct SWDL {
    header: SWDLHeader,
}
impl SWDL {
    pub fn from_file(mut file: File) -> Result<SWDL, Box<dyn std::error::Error>> {
        Ok(SWDL {
            header: SWDLHeader::from_file(&mut file)?
        })
    }
    pub fn from_file_old(mut file: File) -> Result<SWDL, Box<dyn std::error::Error>> {
        Ok(SWDL {
            header: SWDLHeader::from_file_old(&mut file)?
        })
    }
}

#[derive(Debug, Default, Reflect)]
pub struct SWDLHeader {
    magicn: [u8; 4],
    unk18: [u8; 4], // Always zeroes
    flen: u32,
    version: u16,
    unk1: u8,
    unk2: u8,
    unk3: [u8; 4], // Always zeroes
    unk4: [u8; 4], // Always zeroes
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    centisecond: u8, // unsure
    fname: [u8; 16],
    unk10: [u8; 4], // Always 0x00AA AAAA
    unk11: [u8; 4], // Always zeroes
    unk12: [u8; 4], // Always zeroes
    unk13: u32, // Always 0x10
    pcmdlen: u32, //  Length of "pcmd" chunk if there is one. If not, is null! If set to 0xAAAA0000 (The 0000 may contains something else), the file refers to samples inside an external "pcmd" chunk, inside another SWDL ! 
    unk14: [u8; 4], // Always zeroes
    nbwavislots: u16,
    nbprgislots: u16,
    unk17: u16,
    wavilen: u32
}
pub trait AutoReadWrite<T: Reflect + Struct + Default>: Reflect + Struct {
    fn from_file(file: &mut File) -> Result<T, Box<dyn std::error::Error>>;
    fn read_from_file(&mut self, file: &mut File) -> Result<(), Box<dyn std::error::Error>>;
}
impl<T: Reflect + Struct + Default> AutoReadWrite<T> for T {
    fn from_file(file: &mut File) -> Result<T, Box<dyn std::error::Error>> {
        let mut out = Self::default();
        out.read_from_file(file)?;
        Ok(out)
    }
    fn read_from_file(&mut self, file: &mut File) -> Result<(), Box<dyn std::error::Error>> {
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
                    if value_info.type_name() == "u8" {
                        *field.as_any_mut().downcast_mut::<u8>().ok_or("Error in bevy_reflect!")? = file.read_u8()?;
                    } else if value_info.type_name() == "u16" {
                        *field.as_any_mut().downcast_mut::<u16>().ok_or("Error in bevy_reflect!")? = file.read_u16::<LittleEndian>()?;
                    } else if value_info.type_name() == "u32" {
                        *field.as_any_mut().downcast_mut::<u32>().ok_or("Error in bevy_reflect!")? = file.read_u32::<LittleEndian>()?;
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
impl SWDLHeader {
    pub fn from_file_old(file: &mut File) -> Result<SWDLHeader, Box<dyn std::error::Error>> {
        Ok(SWDLHeader {
            magicn: read_n_bytes!(file, 4)?,
            unk18: read_n_bytes!(file, 4)?,
            flen: file.read_u32::<LittleEndian>()?,
            version: file.read_u16::<LittleEndian>()?,
            unk1: file.read_u8()?,
            unk2: file.read_u8()?,
            unk3: read_n_bytes!(file, 4)?,
            unk4: read_n_bytes!(file, 4)?,
            year: file.read_u16::<LittleEndian>()?,
            month: file.read_u8()?,
            day: file.read_u8()?,
            hour: file.read_u8()?,
            minute: file.read_u8()?,
            second: file.read_u8()?,
            centisecond: file.read_u8()?,
            fname: read_n_bytes!(file, 16)?,
            unk10: read_n_bytes!(file, 4)?,
            unk11: read_n_bytes!(file, 4)?,
            unk12: read_n_bytes!(file, 4)?,
            unk13: file.read_u32::<LittleEndian>()?,
            pcmdlen: file.read_u32::<LittleEndian>()?,
            unk14: read_n_bytes!(file, 4)?,
            nbwavislots: file.read_u16::<LittleEndian>()?,
            nbprgislots: file.read_u16::<LittleEndian>()?,
            unk17: file.read_u16::<LittleEndian>()?,
            wavilen: file.read_u32::<LittleEndian>()?
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");

    let raw = File::open("./bgm0016.swd")?;
    let swdl = SWDL::from_file(raw)?;
    let raw = File::open("./bgm0016.swd")?;
    let swdl2 = SWDL::from_file_old(raw)?;

    println!("{:?}", swdl);
    println!("{:?}", swdl2);

    Ok(())
}
