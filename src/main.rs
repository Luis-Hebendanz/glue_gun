use clap::{value_parser, App, Arg, SubCommand};
use log::*;
use std::{process::ExitCode, path::PathBuf};


mod config;
mod run;
fn main() -> ExitCode {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Trace)
        .with_timestamps(false)
        .init()
        .unwrap();
    log::set_max_level(LevelFilter::Info);

    let app = App::new("Glue gun")
        .author("Luis Hebendanz <luis.nixos@gmail.com")
        .about("Glues together a rust bootloader and ELF kernel to generate a bootable ISO file")
        .arg(
            Arg::with_name("verbose")
                .short('v')
                .help("Enables verbose mode")
                .takes_value(false),
        )
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .help_expected(true)
        .subcommand(
            SubCommand::with_name("build")
                .about("Builds the ISO file")
                .arg(
                    Arg::with_name("verbose")
                        .help("Enables verbose mode")
                        .short('v')
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("release")
                        .help("Building in release mode")
                        .short('r')
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("kernel")
                        .help("Path to kernel ELF file")
                        .takes_value(true)
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            SubCommand::with_name("run")
                .about("Builds and runs the ISO file")
                .arg(
                    Arg::with_name("verbose")
                        .help("Enables verbose mode")
                        .short('v')
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("debug")
                        .help("Runs the emulator in debug mode")
                        .short('d')
                        .required(false)
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("kernel")
                        .help("Path to kernel ELF file")
                        .takes_value(true)
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        );

    let matches = app.clone().get_matches();
    glue_gun::parse_matches(&matches)
}

