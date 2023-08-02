use std::fs::{File, OpenOptions};
use std::io::{Write, Read, Seek};
use std::path::{PathBuf, Path};

use clap::{Parser, command, Subcommand};
use dse::swdl::SWDL;
use dse::dtype::ReadWrite;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(author = "Adakite", version = VERSION, about = "Tools for working with SWDL and SWDL.XML files", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands
}

#[derive(Subcommand)]
enum Commands {
    ToXML {
        /// Sets the path of the SWD files to be translated
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the translated files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    },
    FromXML {
        /// Sets the path of the source SWD.XML files
        #[arg(value_name = "INPUT")]
        input_glob: String,

        /// Sets the folder to output the encoded files
        #[arg(short = 'o', long, value_name = "OUTPUT")]
        output_folder: Option<PathBuf>,
    }
}

fn valid_file_of_type<P: AsRef<Path>>(path: P, t: &str) -> bool {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::FromXML { input_glob, output_folder: _output_folder } | Commands::ToXML { input_glob, output_folder: _output_folder } => {
            let (source_file_format, change_ext) = match &cli.command {
                Commands::FromXML { input_glob: _, output_folder: _ } => ("xml", ""),
                Commands::ToXML { input_glob: _, output_folder: _ } => ("swd", "swd.xml")
            };
            
            let output_folder;
            if let Some(custom_output_folder) = _output_folder {
                if std::fs::metadata(&custom_output_folder)?.is_dir() {
                    output_folder = custom_output_folder.clone();
                } else {
                    return Err("Output path must be a folder!".into());
                }
            } else {
                output_folder = std::env::current_dir()?;
            }

            let input_file_paths: Vec<(PathBuf, PathBuf)> = glob::glob(&input_glob).expect("Failed to read glob pattern").into_iter().filter_map(|entry| {
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
            }).collect();

            for (input_file_path, output_file_path) in input_file_paths {
                print!("Converting {}... ", input_file_path.display());
                if source_file_format == "swd" {
                    let mut raw = File::open(input_file_path)?;
                    let mut swdl = SWDL::default();
                    swdl.read_from_file(&mut raw)?;

                    let st = quick_xml::se::to_string(&swdl)?;
                    OpenOptions::new().append(false).create(true).write(true).open(output_file_path)?.write_all(st.as_bytes())?;
                } else if source_file_format == "xml" {
                    let st = std::fs::read_to_string(input_file_path)?;
                    let mut swdl_recreated = quick_xml::de::from_str::<SWDL>(&st)?;
                    swdl_recreated.regenerate_read_markers()?;
                    swdl_recreated.regenerate_automatic_parameters()?;

                    let mut output_file = OpenOptions::new().append(false).create(true).read(true).write(true).open(output_file_path)?;
                    output_file.set_len(0);
                    output_file.seek(std::io::SeekFrom::Start(0));
                    swdl_recreated.write_to_file(&mut output_file);
                } else {
                    panic!("Whaaat?");
                }
                println!("done!");
            }

            println!("\nAll files successfully processed.");
        }
    }

    Ok(())
}

