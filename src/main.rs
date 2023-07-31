use core::panic;
use std::{fs::{File, OpenOptions}, io::{Read, Write, Seek, SeekFrom, Cursor}};
use bevy_reflect::{Reflect, Struct};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

mod dtype;
mod swdl;

use dtype::*;
use swdl::*;

//// NOTE: Any struct fields starting with an _ indicates that that struct field will be ignored when writing, with its appropriate value generate on-the-fly based on the other fields

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");

    let mut raw = File::open("./bgm0043.swd")?;
    let mut swdl = SWDL::default();
    swdl.read_from_file(&mut raw)?;
    println!("{}", swdl.write_to_file(&mut OpenOptions::new().write(true).read(true).create_new(true).open("./tmp.swd")?)?);
    println!("{:#?}", swdl);

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.wavi.data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", 43521, "#", -7, 60, 0, 127, 1, 3, 127, 127, 40, -1);
    for obj in swdl.wavi.data.objects.iter() {
        println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", obj.unk1, obj.id, obj.ctune, obj.rootkey, obj.ktps, obj.volume, obj.volume_envelope.unk19, obj.volume_envelope.unk20, obj.volume_envelope.sustain, obj.volume_envelope.decay2, obj.volume_envelope.release, obj.volume_envelope.unk57);
    }

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi.as_ref().unwrap().data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}\t{}\t{}", "#", 0x0F, 0x200, 0xAA, 0, 0, "16 bytes of padbyte");
    for obj in swdl.prgi.as_ref().unwrap().data.objects.iter() {
        println!("{}\t{}\t{}\t{}\t{}\t{}\t{:?}", obj.header.id, obj.header.thatFbyte, obj.header.unk4, obj.header.PadByte, obj.header.unk7, obj.header.unk9, obj._delimiter.delimiter);
    }

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi.as_ref().unwrap().data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}", "0off1on", "0-4", "1-7", 0x0000, 0x0000);
    for obj in swdl.prgi.as_ref().unwrap().data.objects.iter() {
        for obj in obj.lfo_table.objects.iter() {
            println!("{}\t{}\t{}\t{}\t{}", obj.unk52, obj.dest, obj.wshape, obj.unk32, obj.unk33);
        }
    }

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.prgi.as_ref().unwrap().data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", 0, "=kgrpid", "kgrpid", -7, 0x02, 1, 3, 127, 127, 40, -1);
    for obj in swdl.prgi.as_ref().unwrap().data.objects.iter() {
        for obj in obj.splits_table.objects.iter() {
            println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", obj.unk10, obj.unk11, obj.kgrpid, obj.ctune, obj.unk22, obj.volume_envelope.unk19, obj.volume_envelope.unk20, obj.volume_envelope.sustain, obj.volume_envelope.decay2, obj.volume_envelope.release, obj.volume_envelope.unk57);
        }
    }

    println!("{} objects extracted, check over the following values, they should mostly match the first row.", swdl.kgrp.as_ref().unwrap().data.objects.len());
    println!("{}\t{}\t{}\t{}\t{}", "#", "poly0-15(-1 off)", "priority(8 default)", "0-15", "0-15");
    for obj in swdl.kgrp.as_ref().unwrap().data.objects.iter() {
        println!("{}\t{}\t\t\t{}\t\t\t{}\t{}", obj.id, obj.poly, obj.priority, obj.vclow, obj.vchigh);
    }

    Ok(())
}
