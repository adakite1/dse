use core::panic;
use std::{fs::File, io::{Read, Write, Seek, SeekFrom}};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

macro_rules! read_n_bytes {
    ($file:ident, $n:literal) => {{
        let mut buf: [u8; $n] = [0; $n];
        $file.read_exact(&mut buf).map(|_| buf)
    }};
}

/// Generic Error to represent a variety of errors emitted by the mixer
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
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>>;
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>>;
}
impl<T: Reflect + Struct + Default + AutoReadWrite> ReadWrite for T {
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = 0;
        for field_i in 0..self.field_len() {
            let field = self.field_at(field_i).ok_or("Failed to get field!")?;
            let type_info = field.get_represented_type_info().ok_or("Failed to get type info of field!")?;
            match type_info {
                bevy_reflect::TypeInfo::Array(array_info) => {
                    if array_info.item_type_name() == "u8" {
                        let sl = field.as_any().downcast_ref::<&[u8]>().ok_or("Error in bevy_reflect!")?;
                        writer.write_all(sl)?;
                        bytes_written += sl.len();
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
                _ => panic!("Unsupported auto type!")
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PointerTable<T: ReadWrite + Default> {
    /// ONLY USE AS THE INITIAL NUMBER OF OBJECTS TO READ!!! AFTER READING FROM A FILE, USE objects.len() INSTEAD!!!
    _initial_n: usize,
    objects: Vec<T>
}
impl<T: ReadWrite + Default> PointerTable<T> {
    pub fn new(n: usize) -> PointerTable<T> {
        PointerTable { _initial_n: n, objects: Vec::with_capacity(n) }
    }
}
impl<T: ReadWrite + Default> ReadWrite for PointerTable<T> {
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let pointer_table_byte_len = self.objects.len() * 2;
        let pointer_table_byte_len_aligned = ((pointer_table_byte_len - 1) | 15) + 1; // Round the length of the pointer table in bytes to the next multiple of 16
        let first_pointer = pointer_table_byte_len_aligned;
        let mut accumulated_write = 0;
        let mut accumulated_object_data: Vec<u8> = Vec::new();
        for val in self.objects.iter() {
            writer.write_u16::<LittleEndian>((first_pointer + accumulated_write).try_into()?)?;
            accumulated_write += val.write_to_file(&mut accumulated_object_data)?;
        }
        let padding_aa = pointer_table_byte_len_aligned - pointer_table_byte_len;
        for _ in 0..padding_aa {
            writer.write_u8(10)?;
        }
        writer.write_all(&accumulated_object_data)?;
        Ok(pointer_table_byte_len_aligned + accumulated_object_data.len())
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        let start_of_pointer_table = reader.seek(SeekFrom::Current(0))?;
        for i in 0..self._initial_n {
            let twobyte_offset_from_start_of_pointer_table = reader.read_u16::<LittleEndian>()?;
            if twobyte_offset_from_start_of_pointer_table != 0 {
                reader.seek(SeekFrom::Start(start_of_pointer_table + twobyte_offset_from_start_of_pointer_table as u64))?;
                let mut object = T::default();
                object.read_from_file(reader)?;
                reader.seek(SeekFrom::Start(start_of_pointer_table + (i as u64 + 1) * 2))?;
                self.objects.push(object);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SWDL {
    header: SWDLHeader,
    wavi_header: WAVIHeader,
    wavi_data: PointerTable<SampleInfo>
}
impl SWDL {
    pub fn from_file(mut file: File) -> Result<SWDL, Box<dyn std::error::Error>> {
        let mut header = SWDLHeader::default();
        header.read_from_file(&mut file)?;
        let mut wavi_header = WAVIHeader::default();
        wavi_header.read_from_file(&mut file)?;
        let mut wavi_data: PointerTable<SampleInfo> = PointerTable::new(header.nbwavislots as usize);
        wavi_data.read_from_file(&mut file)?;
        Ok(SWDL {
            header, wavi_header, wavi_data
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
    unk14: [u8; 2], // Always zeroes (The technical documentation on Project Pokemon describes this as 4 bytes, but in my testing for bgm0016.swd at least, it's 2 bytes. I've modified it here)
    nbwavislots: u16,
    nbprgislots: u16,
    unk17: u16,
    wavilen: u32
}
impl AutoReadWrite for SWDLHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct WAVIHeader {
    label: [u8; 4], // Always "wavi"  {0x77, 0x61, 0x76, 0x69} 
    unk1: u16, // Always 0.
    unk2: u16, // Always 0x1504
    chunkbeg: u32, //  Seems to always be 0x10, possibly the start of the chunk data.
    chunklen: u32, //  Length of the chunk data.
}
impl AutoReadWrite for WAVIHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct SampleInfo {
    unk1: u16, // Entry marker? Always 0x01AA
    id: u16,
    ftune: i8, // Pitch fine tuning in cents(?)
    ctune: i8, // Coarse tuning, possibly in semitones(?). Default is -7
    rootkey: i8, // MIDI note
    ktps: i8, // Key transpose. Diff between rootkey and 60.
    volume: i8, // Volume of the sample.
    pan: i8, // Pan of the sample.
    unk5: u8, // Possibly Keygroup parameter for the sample. Always 0x00.
    unk58: u8, // Always 0x02
    unk6: u16, // Always 0x0000
    unk7: [u8; 2], // 0xAA padding.
    unk59: u16, // Always 0x1504.
    smplfmt: u16, // Sample format. 0x0000: 8-bit PCM, 0x0100: 16-bits PCM, 0x0200: 4-bits ADPCM, 0x0300: Possibly PSG
    unk9: u8, // Often 0x09
    smplloop: bool, // true = looped, false = not looped
    unk10: u16, // Often 0x0108
    unk11: u16, // Often 0004
    unk12: u16, // Often 0x0101
    unk13: [u8; 4], // Often 0x0000 0000
    smplrate: u32, // Sample rate in hertz
    smplpos: u32, // Offset of the sound sample in the "pcmd" chunk when there is one. Otherwise, possibly offset of the exact sample among all the sample data loaded in memory? (The value usually doesn't match the main bank's)
    loopbeg: u32, //  The position in bytes divided by 4, the loop begins at, from smplpos. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! (For ADPCM samples, the 4 bytes preamble is counted in the loopbeg!)
    looplen: u32, //  The length of the loop in bytes, divided by 4. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! 
    envon: bool, // Volume envelope on
    envmult: u8, //  If not == 0, is used as multiplier for envelope paramters, and the 16bits lookup table is used for parameter durations. If 0, the 32bits duration lookup table is used instead. This value has no effects on volume parameters, like sustain, and atkvol. 
    unk19: u8, // Usually 0x1
    unk20: u8, // Usually 0x3
    unk21: u16, // Usually 0x03FF (little endian -253)
    unk22: u16, // Usually 0xFFFF
    atkvol: i8, // Sample volume envelope attack volume (0-127) Higher values towards 0x7F means the volume at which the attack phase begins at is louder. Doesn't shorten the attack time. 
    attack: i8, // Sample volume envelope attack (0-127) 126 is ~10 secs
    decay: i8, // Sample volume envelope decay (0-127) Time it takes for note to fall in volume to sustain volume after hitting attack stage
    sustain: i8, // Sample volume envelope sustain (0-127) Note stays at this until noteoff
    hold: i8, // Sample volume envelope hold (0-127) After attack, do not immediately start decaying towards the sustain level. Keep the full volume for some time based on the hold value here.
    decay2: i8, // Sample volume envelope decay 2 (0-127) Time it takes for note to fade after hitting sustain volume.
    release: i8, // Kinda similar to decay2, but I'd hazard a guess that this controls release *after* note off while `decay2` is release while the note is still pressed.
    unk57: i8 // Usually 0xFF
}
impl AutoReadWrite for SampleInfo {  }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");

    let raw = File::open("./bgm0016.swd")?;
    let swdl = SWDL::from_file(raw)?;

    // println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.wavi_data.objects.len());
    // println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", 43521, "#", -7, 60, 0, 127, 1, 3, 127, 127, 40, -1);
    // for obj in swdl.wavi_data.objects.iter() {
    //     println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", obj.unk1, obj.id, obj.ctune, obj.rootkey, obj.ktps, obj.volume, obj.unk19, obj.unk20, obj.sustain, obj.decay2, obj.release, obj.unk57);
    // }

    Ok(())
}
