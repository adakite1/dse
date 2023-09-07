use std::{path::{Path, PathBuf}, fs::{File, OpenOptions}, io::Seek};

use chrono::{DateTime, Local, Datelike, Timelike};
use dse::dtype::DSEError;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn get_file_last_modified_date_with_default<P: AsRef<Path>>(input_file_path: P) -> Result<(u16, u8, u8, u8, u8, u8, u8), DSEError> {
    if let Ok(time) = std::fs::metadata(&input_file_path)?.modified() {
        let dt: DateTime<Local> = time.into();
        Ok((
            dt.year() as u16,
            dt.month() as u8,
            dt.day() as u8,
            dt.hour() as u8,
            dt.minute() as u8,
            dt.second() as u8,
            (dt.nanosecond() / 10_u32.pow(7)) as u8
        ))
    } else {
        Ok((
            2008,
            11,
            16,
            13,
            40,
            57,
            3
        ))
    }
}

pub fn open_file_overwrite_rw<P: AsRef<Path>>(path: P) -> Result<File, DSEError> {
    let mut file = OpenOptions::new().append(false).create(true).read(true).write(true).open(path)?;
    file.set_len(0)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    Ok(file)
}

pub fn get_input_output_pairs(input_glob: &str, source_file_format: &str, output_folder: &PathBuf, change_ext: &str) -> Result<Vec<(PathBuf, PathBuf)>, DSEError> {
    Ok(glob::glob(input_glob)?.into_iter().filter_map(|entry| {
        match entry {
            Ok(path) => {
                if !valid_file_of_type(&path, source_file_format) {
                    println!("Skipping {}!", path.display());
                    None
                } else {
                    if let Some(input_file_name) = path.file_name() {
                        let mut output_path = output_folder.clone();
                        PathBuf::push(&mut output_path, input_file_name);
                        output_path.set_extension(change_ext);
                        Some((path, output_path))
                    } else {
                        None
                    }
                }
            },
            Err(e) => {
                println!("{:?}", e);
                None
            }
        }
    }).collect())
}

pub fn get_final_output_folder(_output_folder: &Option<PathBuf>) -> Result<PathBuf, DSEError> {
    let output_folder;
    if let Some(custom_output_folder) = _output_folder {
        if std::fs::metadata(&custom_output_folder)?.is_dir() {
            output_folder = custom_output_folder.clone();
        } else {
            return Err(DSEError::Invalid("Output path must be a folder!".to_string()));
        }
    } else {
        output_folder = std::env::current_dir()?;
    }
    Ok(output_folder)
}

pub fn valid_file_of_type<P: AsRef<Path>>(path: P, t: &str) -> bool {
    if let Ok(file_metadata) = std::fs::metadata(&path) {
        let is_file = file_metadata.is_file();
        let extension = path.as_ref().extension();
        if let Some(extension) = extension {
            if let Some(extension) = extension.to_str() {
                is_file && extension.to_lowercase() == t.to_lowercase()
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    }
}

