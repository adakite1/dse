/// Example: .\swdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.swd -o unpack
/// Example: .\swdl_tool.exe from-xml .\unpack\*.swd.xml -o .\NDS_UNPACK\data\SOUND\BGM\

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, command, Subcommand};
use dse::swdl::SWDL;
use dse::dtype::ReadWrite;

#[path = "../binutils.rs"]
mod binutils;
use binutils::VERSION;
use crate::binutils::{get_final_output_folder, get_input_output_pairs, open_file_overwrite_rw};

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::FromXML { input_glob, output_folder } | Commands::ToXML { input_glob, output_folder } => {
            let (source_file_format, change_ext) = match &cli.command {
                Commands::FromXML { input_glob: _, output_folder: _ } => ("xml", ""),
                Commands::ToXML { input_glob: _, output_folder: _ } => ("swd", "swd.xml")
            };
            let output_folder = get_final_output_folder(output_folder)?;
            let input_file_paths: Vec<(PathBuf, PathBuf)> = get_input_output_pairs(input_glob, source_file_format, &output_folder, change_ext);

            for (input_file_path, output_file_path) in input_file_paths {
                print!("Converting {}... ", input_file_path.display());
                if source_file_format == "swd" {
                    let mut raw = File::open(input_file_path)?;
                    let mut swdl = SWDL::default();
                    swdl.read_from_file(&mut raw)?;

                    let st = quick_xml::se::to_string(&swdl)?;
                    open_file_overwrite_rw(output_file_path)?.write_all(st.as_bytes())?;
                } else if source_file_format == "xml" {
                    let st = std::fs::read_to_string(input_file_path)?;
                    let mut swdl_recreated = quick_xml::de::from_str::<SWDL>(&st)?;
                    swdl_recreated.regenerate_read_markers()?;
                    swdl_recreated.regenerate_automatic_parameters()?;

                    swdl_recreated.write_to_file(&mut open_file_overwrite_rw(output_file_path)?)?;
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

