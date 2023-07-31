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
                bevy_reflect::TypeInfo::Struct(_) => {
                    if let Some(vol_envelope) = field.as_any().downcast_ref::<ADSRVolumeEnvelope>() {
                        bytes_written += vol_envelope.write_to_file(writer)?;
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
pub struct Table<T: ReadWrite + Default> {
    /// ONLY USE AS THE NUMBER OF OBJECTS TO READ!!! USE objects.len() INSTEAD OUTSIDE OF read_from_file!!!
    _read_n: usize,
    objects: Vec<T>
}
impl<T: ReadWrite + Default> Table<T> {
    pub fn new(n: usize) -> Table<T> {
        Table { _read_n: n, objects: Vec::with_capacity(n) }
    }
    pub fn set_read_params(&mut self, n: usize) {
        self._read_n = n;
    }
}
impl<T: ReadWrite + Default> ReadWrite for Table<T> {
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = 0;
        for object in self.objects.iter() {
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

#[derive(Debug)]
pub struct PointerTable<T: ReadWrite + Default> {
    /// ONLY USE AS THE NUMBER OF OBJECTS TO READ!!! USE objects.len() INSTEAD OUTSIDE OF read_from_file!!!
    _read_n: usize,
    _chunk_len: u32,
    objects: Vec<T>
}
impl<T: ReadWrite + Default> PointerTable<T> {
    pub fn new(n: usize, chunk_len: u32) -> PointerTable<T> {
        PointerTable { _read_n: n, _chunk_len: chunk_len, objects: Vec::with_capacity(n) }
    }
    pub fn set_read_params(&mut self, n: usize, chunk_len: u32) {
        self._read_n = n;
        self._chunk_len = chunk_len;
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
pub struct ChunkHeader {
    label: [u8; 4], // Always "wavi"  {0x77, 0x61, 0x76, 0x69} 
    unk1: u16, // Always 0.
    unk2: u16, // Always 0x1504
    chunkbeg: u32, //  Seems to always be 0x10, possibly the start of the chunk data.
    chunklen: u32, //  Length of the chunk data.
}
impl AutoReadWrite for ChunkHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct ADSRVolumeEnvelope {
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
impl AutoReadWrite for ADSRVolumeEnvelope {  }

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
    volume_envelope: ADSRVolumeEnvelope
}
impl AutoReadWrite for SampleInfo {  }

#[derive(Debug, Default, Reflect)]
pub struct ProgramInfoHeader {
    id: u16, // Index of the pointer in the pointer table. Also corresponding to the program ID in the corresponding SMDL file!
    nbsplits: u16, // Nb of samples mapped to this preset in the split table.
    prgvol: i8, // Volume of the entire program.
    prgpan: i8, // Pan of the entire program (0-127, 64 mid, 127 right, 0 left)
    unk3: u8, // Most of the time 0x00
    thatFbyte: u8, // Most of the time 0x0F
    unk4: u16, // Most of the time 0x200
    unk5: u8, // Most of the time is 0x00
    nblfos: u8, // Nb of entries in the LFO table.
    PadByte: u8, // Most of the time is 0xAA, or 0x00. Value here used as the delimiter and padding later between the LFOTable and the SplitEntryTable (and more)
    unk7: u8, // Most of the time is 0x0
    unk8: u8, // Most of the time is 0x0
    unk9: u8, // Most of the time is 0x0
}
impl AutoReadWrite for ProgramInfoHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct LFOEntry {
    unk34: u8, // Unknown, usually 0x00. Does seem to have an effect with a certain combination of other values in the other parameters.
    unk52: u8, // Unknown, usually 0x00. Most of the time, value is 1 when the LFO is in use.
    dest: u8, // 0x0: disabled, 0x1: pitch, 0x2: volume, 0x3: pan, 0x4: lowpass/cutoff filter?
    wshape: u8, // Shape/function of the waveform. When the LFO is disabled, its always 1.
    rate: u16, // Rate at which the LFO "oscillate". May or may not be in Hertz.
    unk29: u16, // uint16? Changing the value seems to induce feedback or resonance. (Corrupting engine?)
    depth: u16, // The depth parameter of the LFO.
    delay: u16, // Delay in ms before the LFO's effect is applied after the sample begins playing. (Per-note LFOs! So fancy!)
    unk32: u16, // Unknown, usually 0x0000. Possibly fade-out in ms.
    unk33: u16, // Unknown, usually 0x0000. Possibly an extra parameter? Or a cutoff/lowpass filter's frequency cutoff?
}
impl AutoReadWrite for LFOEntry {  }

#[derive(Debug, Default, Reflect)]
pub struct SplitEntry {
    unk10: u8, // A leading 0.
    id: u8, //  The Index of the sample in the SplitsTbl! (So, a simple array with elements that reference the index of itself)
    unk11: u8, // Unknown. Is always the same value as offset 0x1A below! (It doesn't seem to match kgrpid, so I'm wondering which byte this might be referring to) (Possibly "bend range" according to assumptions made from teh DSE screenshots)
    unk25: u8, // Unknown. Possibly a boolean.
    lowkey: i8, // Usually 0x00. Lowest MIDI key this sample can play on.
    hikey: i8, // Usually 0x7F. Highest MIDI key this sample can play on.
    lowkey2: i8, // A copy of lowkey, for unknown purpose.
    hikey2: i8, // A copy of hikey, for unknown purpose.
    lovel: i8, // Lowest note velocity the sample is played on. (0-127) (DSE has velocity layers!)
    hivel: i8, // Highest note velocity the sample is played on. (0-127)
    lovel2: i8, // A copy of lovel, for unknown purpose. Usually 0x00. 
    hivel2: i8, // A copy of hivel, for unknown purpose. Usually 0x7F.
    unk16: [u8; 4], // Usually the same value as "PadByte", or 0. Possibly padding.
    unk17: [u8; 2], // Usually the same value as "PadByte", or 0. Possibly padding.
    SmplID: u16, // The ID/index of sample in the "wavi" chunk's lookup table.
    ftune: i8, // Fine tune in cents.
    ctune: i8, // Coarse tuning. Default is -7.
    rootkey: i8, // Note at which the sample is sampled at!
    ktps: i8, // Key transpose. Diff between rootkey and 60.
    smplvol: i8, // Volume of the sample
    smplpan: i8, // Pan of the sample
    kgrpid: u8, // Keygroup ID of the keygroup this split belongs to!
    unk22: u8, // Unknown, possibly a flag. Usually 0x02.
    unk23: u16, // Unknown, usually 0000.
    unk24: [u8; 2], // Usually the same value as "PadByte", or 0. Possibly padding?
    // After here, the last 16 bytes are for the volume enveloped. They override the sample's original volume envelope!
    volume_envelope: ADSRVolumeEnvelope
}
impl AutoReadWrite for SplitEntry {  }

#[derive(Debug, Default, Reflect)]
pub struct _ProgramInfoDelimiter {
    pub delimiter: [u8; 16],
}
impl AutoReadWrite for _ProgramInfoDelimiter {  }
#[derive(Debug)]
pub struct ProgramInfo {
    header: ProgramInfoHeader,
    lfo_table: Table<LFOEntry>,
    delimiter: _ProgramInfoDelimiter,
    splits_table: Table<SplitEntry>
}
impl Default for ProgramInfo {
    fn default() -> ProgramInfo {
        ProgramInfo {
            header: ProgramInfoHeader::default(),
            lfo_table: Table::new(4), // Rough estimate
            delimiter: _ProgramInfoDelimiter::default(),
            splits_table: Table::new(8) // Rough estimate
        }
    }
}
impl ReadWrite for ProgramInfo {
    fn write_to_file<W: Write>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.lfo_table.write_to_file(writer)?;
        bytes_written += self.delimiter.write_to_file(writer)?;
        bytes_written += self.splits_table.write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.lfo_table.set_read_params(self.header.nblfos as usize);
        self.lfo_table.read_from_file(reader)?;
        self.delimiter.read_from_file(reader)?;
        self.splits_table.set_read_params(self.header.nbsplits as usize);
        self.splits_table.read_from_file(reader)?;
        Ok(())
    }
}

#[derive(Debug, Default, Reflect)]
pub struct Keygroup {
    id: u16, // Index/ID of the keygroup
    poly: i8, // Polyphony. Max number of simultaneous notes played. 0 to 15. -1 means disabled. (Technical documentation describes this field as unsigned, but I've switched it to signed since -1 is off instead of 255 being off)
    priority: u8, // Priority over the assignment of voice channels for members of this group. 0-possibly 99, default is 8. Higher is higeher priority.
    vclow: u8, // Lowest voice channel the group may use. Usually between 0 and 15
    vchigh: u8, // Highest voice channel this group may use. 0-15
    unk50: u8, // Unown
    unk51: u8, // Unknown
}
impl AutoReadWrite for Keygroup {  }

#[derive(Debug, Default, Reflect)]
pub struct _KeygroupsSampleDataDelimiter {
    pub delimiter: [u8; 8],
}
impl AutoReadWrite for _KeygroupsSampleDataDelimiter {  }
#[derive(Debug)]
pub struct SWDL {
    header: SWDLHeader,
    wavi_header: ChunkHeader,
    wavi_data: PointerTable<SampleInfo>,
    prgi_header: ChunkHeader,
    prgi_data: PointerTable<ProgramInfo>,
    kgrp_header: ChunkHeader,
    kgrp_data: Table<Keygroup>,
    padding: Option<_KeygroupsSampleDataDelimiter>
}
impl SWDL {
    pub fn from_file(mut file: File) -> Result<SWDL, Box<dyn std::error::Error>> {
        let mut header = SWDLHeader::default();
        header.read_from_file(&mut file)?;
        let mut wavi_header = ChunkHeader::default();
        wavi_header.read_from_file(&mut file)?;
        let mut wavi_data: PointerTable<SampleInfo> = PointerTable::new(header.nbwavislots as usize, wavi_header.chunklen);
        wavi_data.read_from_file(&mut file)?;
        let mut prgi_header = ChunkHeader::default();
        prgi_header.read_from_file(&mut file)?;
        let mut prgi_data: PointerTable<ProgramInfo> = PointerTable::new(header.nbprgislots as usize, prgi_header.chunklen);
        prgi_data.read_from_file(&mut file)?;
        let mut kgrp_header = ChunkHeader::default();
        kgrp_header.read_from_file(&mut file)?;
        let mut kgrp_data: Table<Keygroup> = Table::new(kgrp_header.chunklen as usize / 8);
        kgrp_data.read_from_file(&mut file)?;
        let mut padding = Some(_KeygroupsSampleDataDelimiter::default());
        padding.as_mut().unwrap().read_from_file(&mut file)?;
        // "pcmd" {0x70, 0x63, 0x6D, 0x64}
        // "eod\20" {0x65, 0x6F, 0x64, 0x20}
        if &padding.as_ref().unwrap().delimiter[..4] == &[0x70, 0x63, 0x6D, 0x64] ||
            &padding.as_ref().unwrap().delimiter[..4] == &[0x65, 0x6F, 0x64, 0x20] {
            padding = None;
            file.seek(SeekFrom::Current(-8))?;
        }

        // Optional chunks
        Ok(SWDL {
            header, wavi_header, wavi_data, prgi_header, prgi_data, kgrp_header, kgrp_data, padding
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");

    let raw = File::open("./bgm0016.swd")?;
    let swdl = SWDL::from_file(raw)?;

    // println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.wavi_data.objects.len());
    // println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", 43521, "#", -7, 60, 0, 127, 1, 3, 127, 127, 40, -1);
    // for obj in swdl.wavi_data.objects.iter() {
    //     println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", obj.unk1, obj.id, obj.ctune, obj.rootkey, obj.ktps, obj.volume, obj.volume_envelope.unk19, obj.volume_envelope.unk20, obj.volume_envelope.sustain, obj.volume_envelope.decay2, obj.volume_envelope.release, obj.volume_envelope.unk57);
    // }

    // println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi_data.objects.len());
    // println!("{}\t{}\t{}\t{}\t{}\t{}\t{}", "#", 0x0F, 0x200, 0xAA, 0, 0, "16 bytes of padbyte");
    // for obj in swdl.prgi_data.objects.iter() {
    //     println!("{}\t{}\t{}\t{}\t{}\t{}\t{:?}", obj.header.id, obj.header.thatFbyte, obj.header.unk4, obj.header.PadByte, obj.header.unk7, obj.header.unk9, obj.delimiter.delimiter);
    // }

    // println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi_data.objects.len());
    // println!("{}\t{}\t{}\t{}\t{}", "0off1on", "0-4", "1-7", 0x0000, 0x0000);
    // for obj in swdl.prgi_data.objects.iter() {
    //     for obj in obj.lfo_table.objects.iter() {
    //         println!("{}\t{}\t{}\t{}\t{}", obj.unk52, obj.dest, obj.wshape, obj.unk32, obj.unk33);
    //     }
    // }

    // println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi_data.objects.len());
    // println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", 0, "=kgrpid", "kgrpid", -7, 0x02, 1, 3, 127, 127, 40, -1);
    // for obj in swdl.prgi_data.objects.iter() {
    //     for obj in obj.splits_table.objects.iter() {
    //         println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", obj.unk10, obj.unk11, obj.kgrpid, obj.ctune, obj.unk22, obj.volume_envelope.unk19, obj.volume_envelope.unk20, obj.volume_envelope.sustain, obj.volume_envelope.decay2, obj.volume_envelope.release, obj.volume_envelope.unk57);
    //     }
    // }

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.kgrp_data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}", "#", "poly0-15(-1 off)", "priority(8 default)", "0-15", "0-15");
    for obj in swdl.kgrp_data.objects.iter() {
        println!("{}\t{}\t\t\t{}\t\t\t{}\t{}", obj.id, obj.poly, obj.priority, obj.vclow, obj.vchigh);
    }

    Ok(())
}
