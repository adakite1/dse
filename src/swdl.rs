use core::panic;
use std::{fs::{File, OpenOptions}, io::{Read, Write, Seek, SeekFrom, Cursor}};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

use crate::peek_magic;
use crate::dtype::{*};

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields

#[derive(Debug, Default, Reflect)]
pub struct SWDLHeader {
    pub magicn: [u8; 4],
    pub unk18: [u8; 4], // Always zeroes
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
    pub unk10: [u8; 4], // Always 0x00AA AAAA
    pub unk11: [u8; 4], // Always zeroes
    pub unk12: [u8; 4], // Always zeroes
    pub unk13: u32, // Always 0x10
    pub pcmdlen: u32, //  Length of "pcmd" chunk if there is one. If not, is null! If set to 0xAAAA0000 (The 0000 may contains something else), the file refers to samples inside an external "pcmd" chunk, inside another SWDL ! 
    pub unk14: [u8; 2], // Always zeroes (The technical documentation on Project Pokemon describes this as 4 bytes, but in my testing for bgm0016.swd at least, it's 2 bytes. I've modified it here)
    pub nbwavislots: u16,
    pub nbprgislots: u16,
    pub unk17: u16,
    pub wavilen: u32
}
impl AutoReadWrite for SWDLHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct ChunkHeader {
    pub label: [u8; 4], // Always "wavi"  {0x77, 0x61, 0x76, 0x69} 
    pub unk1: u16, // Always 0.
    pub unk2: u16, // Always 0x1504
    pub chunkbeg: u32, //  Seems to always be 0x10, possibly the start of the chunk data.
    pub chunklen: u32, //  Length of the chunk data.
}
impl AutoReadWrite for ChunkHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct ADSRVolumeEnvelope {
    pub envon: bool, // Volume envelope on
    pub envmult: u8, //  If not == 0, is used as multiplier for envelope paramters, and the 16bits lookup table is used for parameter durations. If 0, the 32bits duration lookup table is used instead. This value has no effects on volume parameters, like sustain, and atkvol. 
    pub unk19: u8, // Usually 0x1
    pub unk20: u8, // Usually 0x3
    pub unk21: u16, // Usually 0x03FF (little endian -253)
    pub unk22: u16, // Usually 0xFFFF
    pub atkvol: i8, // Sample volume envelope attack volume (0-127) Higher values towards 0x7F means the volume at which the attack phase begins at is louder. Doesn't shorten the attack time. 
    pub attack: i8, // Sample volume envelope attack (0-127) 126 is ~10 secs
    pub decay: i8, // Sample volume envelope decay (0-127) Time it takes for note to fall in volume to sustain volume after hitting attack stage
    pub sustain: i8, // Sample volume envelope sustain (0-127) Note stays at this until noteoff
    pub hold: i8, // Sample volume envelope hold (0-127) After attack, do not immediately start decaying towards the sustain level. Keep the full volume for some time based on the hold value here.
    pub decay2: i8, // Sample volume envelope decay 2 (0-127) Time it takes for note to fade after hitting sustain volume.
    pub release: i8, // Kinda similar to decay2, but I'd hazard a guess that this controls release *after* note off while `decay2` is release while the note is still pressed.
    pub unk57: i8 // Usually 0xFF
}
impl AutoReadWrite for ADSRVolumeEnvelope {  }

#[derive(Debug, Default, Reflect)]
pub struct SampleInfo {
    pub unk1: u16, // Entry marker? Always 0x01AA
    pub id: u16,
    pub ftune: i8, // Pitch fine tuning in cents(?)
    pub ctune: i8, // Coarse tuning, possibly in semitones(?). Default is -7
    pub rootkey: i8, // MIDI note
    pub ktps: i8, // Key transpose. Diff between rootkey and 60.
    pub volume: i8, // Volume of the sample.
    pub pan: i8, // Pan of the sample.
    pub unk5: u8, // Possibly Keygroup parameter for the sample. Always 0x00.
    pub unk58: u8, // Always 0x02
    pub unk6: u16, // Always 0x0000
    pub unk7: [u8; 2], // 0xAA padding.
    pub unk59: u16, // Always 0x1504.
    pub smplfmt: u16, // Sample format. 0x0000: 8-bit PCM, 0x0100: 16-bits PCM, 0x0200: 4-bits ADPCM, 0x0300: Possibly PSG
    pub unk9: u8, // Often 0x09
    pub smplloop: bool, // true = looped, false = not looped
    pub unk10: u16, // Often 0x0108
    pub unk11: u16, // Often 0004
    pub unk12: u16, // Often 0x0101
    pub unk13: [u8; 4], // Often 0x0000 0000
    pub smplrate: u32, // Sample rate in hertz
    pub smplpos: u32, // Offset of the sound sample in the "pcmd" chunk when there is one. Otherwise, possibly offset of the exact sample among all the sample data loaded in memory? (The value usually doesn't match the main bank's)
    pub loopbeg: u32, //  The position in bytes divided by 4, the loop begins at, from smplpos. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! (For ADPCM samples, the 4 bytes preamble is counted in the loopbeg!)
    pub looplen: u32, //  The length of the loop in bytes, divided by 4. ( multiply by 4 to get size in bytes ) Adding loopbeg + looplen gives the sample's length ! 
    pub volume_envelope: ADSRVolumeEnvelope
}
impl IsSelfIndexed for SampleInfo {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.id = new_index.try_into()?;
        Ok(())
    }
}
impl AutoReadWrite for SampleInfo {  }

#[derive(Debug, Default, Reflect)]
pub struct ProgramInfoHeader {
    pub id: u16, // Index of the pointer in the pointer table. Also corresponding to the program ID in the corresponding SMDL file!
    pub nbsplits: u16, // Nb of samples mapped to this preset in the split table.
    pub prgvol: i8, // Volume of the entire program.
    pub prgpan: i8, // Pan of the entire program (0-127, 64 mid, 127 right, 0 left)
    pub unk3: u8, // Most of the time 0x00
    pub thatFbyte: u8, // Most of the time 0x0F
    pub unk4: u16, // Most of the time 0x200
    pub unk5: u8, // Most of the time is 0x00
    pub nblfos: u8, // Nb of entries in the LFO table.
    pub PadByte: u8, // Most of the time is 0xAA, or 0x00. Value here used as the delimiter and padding later between the LFOTable and the SplitEntryTable (and more)
    pub unk7: u8, // Most of the time is 0x0
    pub unk8: u8, // Most of the time is 0x0
    pub unk9: u8, // Most of the time is 0x0
}
impl IsSelfIndexed for ProgramInfoHeader {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.id = new_index.try_into()?;
        Ok(())
    }
}
impl AutoReadWrite for ProgramInfoHeader {  }

#[derive(Debug, Default, Reflect)]
pub struct LFOEntry {
    pub unk34: u8, // Unknown, usually 0x00. Does seem to have an effect with a certain combination of other values in the other parameters.
    pub unk52: u8, // Unknown, usually 0x00. Most of the time, value is 1 when the LFO is in use.
    pub dest: u8, // 0x0: disabled, 0x1: pitch, 0x2: volume, 0x3: pan, 0x4: lowpass/cutoff filter?
    pub wshape: u8, // Shape/function of the waveform. When the LFO is disabled, its always 1.
    pub rate: u16, // Rate at which the LFO "oscillate". May or may not be in Hertz.
    pub unk29: u16, // uint16? Changing the value seems to induce feedback or resonance. (Corrupting engine?)
    pub depth: u16, // The depth parameter of the LFO.
    pub delay: u16, // Delay in ms before the LFO's effect is applied after the sample begins playing. (Per-note LFOs! So fancy!)
    pub unk32: u16, // Unknown, usually 0x0000. Possibly fade-out in ms.
    pub unk33: u16, // Unknown, usually 0x0000. Possibly an extra parameter? Or a cutoff/lowpass filter's frequency cutoff?
}
impl IsSelfIndexed for LFOEntry {
    fn is_self_indexed(&self) -> Option<usize> {
        None
    }
    fn change_self_index(&mut self, _: usize) -> Result<(), Box<dyn std::error::Error>> {
        Err(Box::new(GenericError::new("LFO entries do not have indices!!")))
    }
}
impl AutoReadWrite for LFOEntry {  }

#[derive(Debug, Default, Reflect)]
pub struct SplitEntry {
    pub unk10: u8, // A leading 0.
    pub id: u8, //  The Index of the sample in the SplitsTbl! (So, a simple array with elements that reference the index of itself)
    pub unk11: u8, // Unknown. Is always the same value as offset 0x1A below! (It doesn't seem to match kgrpid, so I'm wondering which byte this might be referring to) (Possibly "bend range" according to assumptions made from teh DSE screenshots)
    pub unk25: u8, // Unknown. Possibly a boolean.
    pub lowkey: i8, // Usually 0x00. Lowest MIDI key this sample can play on.
    pub hikey: i8, // Usually 0x7F. Highest MIDI key this sample can play on.
    pub lowkey2: i8, // A copy of lowkey, for unknown purpose.
    pub hikey2: i8, // A copy of hikey, for unknown purpose.
    pub lovel: i8, // Lowest note velocity the sample is played on. (0-127) (DSE has velocity layers!)
    pub hivel: i8, // Highest note velocity the sample is played on. (0-127)
    pub lovel2: i8, // A copy of lovel, for unknown purpose. Usually 0x00. 
    pub hivel2: i8, // A copy of hivel, for unknown purpose. Usually 0x7F.
    pub unk16: [u8; 4], // Usually the same value as "PadByte", or 0. Possibly padding.
    pub unk17: [u8; 2], // Usually the same value as "PadByte", or 0. Possibly padding.
    pub SmplID: u16, // The ID/index of sample in the "wavi" chunk's lookup table.
    pub ftune: i8, // Fine tune in cents.
    pub ctune: i8, // Coarse tuning. Default is -7.
    pub rootkey: i8, // Note at which the sample is sampled at!
    pub ktps: i8, // Key transpose. Diff between rootkey and 60.
    pub smplvol: i8, // Volume of the sample
    pub smplpan: i8, // Pan of the sample
    pub kgrpid: u8, // Keygroup ID of the keygroup this split belongs to!
    pub unk22: u8, // Unknown, possibly a flag. Usually 0x02.
    pub unk23: u16, // Unknown, usually 0000.
    pub unk24: [u8; 2], // Usually the same value as "PadByte", or 0. Possibly padding?
    // After here, the last 16 bytes are for the volume enveloped. They override the sample's original volume envelope!
    pub volume_envelope: ADSRVolumeEnvelope
}
impl IsSelfIndexed for SplitEntry {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.id = new_index.try_into()?;
        Ok(())
    }
}
impl AutoReadWrite for SplitEntry {  }

#[derive(Debug, Default, Reflect)]
pub struct _ProgramInfoDelimiter {
    pub delimiter: [u8; 16],
}
impl AutoReadWrite for _ProgramInfoDelimiter {  }
#[derive(Debug)]
pub struct ProgramInfo {
    pub header: ProgramInfoHeader,
    pub lfo_table: Table<LFOEntry>,
    pub _delimiter: _ProgramInfoDelimiter,
    pub splits_table: Table<SplitEntry>
}
impl IsSelfIndexed for ProgramInfo {
    fn is_self_indexed(&self) -> Option<usize> {
        self.header.is_self_indexed()
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.lfo_table.write_to_file(writer)?;
        // bytes_written += self._delimiter.write_to_file(writer)?;
        bytes_written += vec![self.header.PadByte; 16].write_to_file(writer)?;
        bytes_written += self.splits_table.write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.lfo_table.set_read_params(self.header.nblfos as usize);
        self.lfo_table.read_from_file(reader)?;
        self._delimiter.read_from_file(reader)?;
        self.splits_table.set_read_params(self.header.nbsplits as usize);
        self.splits_table.read_from_file(reader)?;
        Ok(())
    }
}

#[derive(Debug, Default, Reflect)]
pub struct Keygroup {
    pub id: u16, // Index/ID of the keygroup
    pub poly: i8, // Polyphony. Max number of simultaneous notes played. 0 to 15. -1 means disabled. (Technical documentation describes this field as unsigned, but I've switched it to signed since -1 is off instead of 255 being off)
    pub priority: u8, // Priority over the assignment of voice channels for members of this group. 0-possibly 99, default is 8. Higher is higeher priority.
    pub vclow: i8, // Lowest voice channel the group may use. Usually between 0 and 15
    pub vchigh: i8, // Highest voice channel this group may use. 0-15 (While not explicitly stated in the documentation, this value being i8 makes sense as the first keygroup typically has this set to 255 which makes more sense interpreted as -1 disabled)
    pub unk50: u8, // Unown
    pub unk51: u8, // Unknown
}
impl IsSelfIndexed for Keygroup {
    fn is_self_indexed(&self) -> Option<usize> {
        Some(self.id as usize)
    }
    fn change_self_index(&mut self, new_index: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.id = new_index.try_into()?;
        Ok(())
    }
}
impl AutoReadWrite for Keygroup {  }

#[derive(Debug)]
pub struct WAVIChunk {
    _read_n: usize,
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file(reader)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct PRGIChunk {
    _read_n: usize,
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)?)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.read_from_file(reader)?;
        self.data.set_read_params(self._read_n, self.header.chunklen);
        self.data.read_from_file(reader)?;
        Ok(())
    }
}

#[derive(Debug, Default, Reflect)]
pub struct _KeygroupsSampleDataDelimiter {
    pub delimiter: [u8; 8],
}
impl AutoReadWrite for _KeygroupsSampleDataDelimiter {  }
#[derive(Debug)]
pub struct KGRPChunk {
    pub header: ChunkHeader,
    pub data: Table<Keygroup>,
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)? + if self.data.objects.len() % 2 == 1 { vec![0x67, 0xC0, 0x40, 0x00, 0x88, 0x00, 0xFF, 0x04].write_to_file(writer)? } else { 0 })
        // Ok(self.header.write_to_file(writer)? + self.data.write_to_file(writer)? + if let Some(pad) = &self._padding { pad.write_to_file(writer)? } else { 0 })
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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

#[derive(Debug)]
pub struct PCMDChunk {
    pub header: ChunkHeader,
    pub data: Vec<u8>,
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
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let len = self.header.write_to_file(writer)? + self.data.write_to_file(writer)?;
        let len_aligned = ((len - 1) | 15) + 1; // Round the length of the pcmd chunk in bytes to the next multiple of 16
        let padding_zero = len_aligned - len;
        for _ in 0..padding_zero {
            writer.write_u8(0)?;
        }
        Ok(len_aligned)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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

#[derive(Debug)]
pub struct SWDL {
    pub header: SWDLHeader,
    pub wavi: WAVIChunk,
    pub prgi: Option<PRGIChunk>,
    pub kgrp: Option<KGRPChunk>,
    pub pcmd: Option<PCMDChunk>,
    pub eod: ChunkHeader
}
impl Default for SWDL {
    fn default() -> SWDL {
        SWDL {
            header: SWDLHeader::default(),
            wavi: WAVIChunk::new(0),
            prgi: None,
            kgrp: None,
            pcmd: None,
            eod: ChunkHeader::default()
        }
    }
}
impl ReadWrite for SWDL {
    fn write_to_file<W: Read + Write + Seek>(&self, writer: &mut W) -> Result<usize, Box<dyn std::error::Error>> {
        let mut bytes_written = self.header.write_to_file(writer)?;
        bytes_written += self.wavi.write_to_file(writer)?;
        bytes_written += if let Some(prgi) = &self.prgi { prgi.write_to_file(writer)? } else { 0 };
        bytes_written += if let Some(kgrp) = &self.kgrp { kgrp.write_to_file(writer)? } else { 0 };
        bytes_written += if let Some(pcmd) = &self.pcmd { pcmd.write_to_file(writer)? } else { 0 };
        bytes_written += self.eod.write_to_file(writer)?;
        Ok(bytes_written)
    }
    fn read_from_file<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
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
        self.eod.read_from_file(reader)?;
        Ok(())
    }
}