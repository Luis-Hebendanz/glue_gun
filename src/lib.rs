#![allow(dead_code)]

use clap::ArgMatches;
use clap::{value_parser, Arg};
use log::*;

use std::process::ExitCode;

use std::{env, path::PathBuf};

mod build;
mod clean;
mod config;
mod metadata;
mod run;
mod sym;
mod watch;

pub fn create_cli() -> clap::Command {
    clap::Command::new("glue_gun")
        .author("Luis Hebendanz <luis.nixos@gmail.com")
        .about("Glues together a rust bootloader and ELF kernel to generate a bootable ISO file")
        .alias("gg")
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enables verbose mode")
                .action(clap::ArgAction::Count)
                .global(true),
        )
        .arg(
            Arg::new("kernel")
                .help("Path to kernel executable")
                .short('k')
                .long("kernel")
                .global(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("release")
                .global(true)
                .help("Building in release mode")
                .long("release")
                .short('r')
                .action(clap::ArgAction::SetTrue)
                .conflicts_with("kernel"),
        )
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .help_expected(true)
        .subcommand(clap::Command::new("build").about("Builds the ISO file"))
        .subcommand(
            clap::Command::new("run")
                .about("Builds and runs the ISO file")
                .arg(
                    Arg::new("debug")
                        .help("Runs the emulator in debug mode")
                        .short('d')
                        .long("debug")
                        .action(clap::ArgAction::SetTrue)
                        .required(false),
                ),
        )
        .subcommand(
            clap::Command::new("watch").about("Watches for changes in kernel and bootloader"),
        )
        .subcommand(
            clap::Command::new("clean")
                .about("Deletes build artifacts of the kernel and bootloader crate")
                .arg(
                    Arg::new("all")
                        .help("Deletes all artifacts")
                        .short('a')
                        .long("all")
                        .action(clap::ArgAction::SetTrue)
                        .required(false),
                ),
        )
}

#[derive(Debug, Clone, Copy)]
pub struct CliOptions {
    is_release: bool,
    is_verbose: bool,
    is_very_verbose: bool,
}

pub async fn parse_matches(matches: &ArgMatches) -> Result<(), ExitCode> {
    let cli_options = CliOptions {
        is_release: matches.get_flag("release"),
        is_verbose: matches.get_count("verbose") >= 1,
        is_very_verbose: matches.get_count("verbose") > 1,
    };

    if cli_options.is_verbose {
        log::set_max_level(LevelFilter::Debug);
    }
    debug!("Args: {:?}", std::env::args());

    let manifests = get_crate_paths();

    if let Some(matches) = matches.subcommand_matches("clean") {
        let is_all = matches.get_flag("all");
        crate::clean::glue_gun_clean(&manifests, cli_options, is_all);
        return Ok(());
    }

    // If subcommand 'build' or 'run'
    let kernel_exec_path: PathBuf = {
        match matches.get_one::<PathBuf>("kernel") {
            Some(path) => path.clone(),
            None => {
                let kernel_path = crate::build::cargo_build(
                    &manifests.kernel.crate_path,
                    None,
                    cli_options.is_release,
                    cli_options.is_very_verbose,
                    None,
                    None,
                );

                if kernel_path.len() != 1 {
                    panic!(
                        "Expected kernel to generate exactly one binary however {} habe been build",
                        kernel_path.len()
                    );
                }
                kernel_path.get(0).unwrap().clone()
            }
        }
    };

    if let Some(_matches) = matches.subcommand_matches("watch") {
        crate::watch::glue_gun_watch(kernel_exec_path, manifests, cli_options).await;
        return Ok(());
    }

    let artifacts = crate::build::glue_gun_build(&kernel_exec_path, &manifests, &cli_options);

    if let Some(matches) = matches.subcommand_matches("run") {
        run::glue_gun_run(
            artifacts.config,
            &artifacts.iso_img,
            artifacts.is_test,
            matches.get_flag("debug"),
        )
        .unwrap();
        return Ok(());
    }

    Ok(())
}

use crate::metadata::CrateMetadata;

pub struct Manifest {
    crate_name: String,
    crate_path: PathBuf,
    cargo_toml: PathBuf,
    target_dir: PathBuf,
    meta: CrateMetadata,
}

pub struct Manifests {
    kernel: Manifest,
    bootloader: Manifest,
}

fn get_crate_paths() -> Manifests {
    let kernel_manifest = {
        let kernel_crate_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .or_else(|_| {
                debug!("CARGO_MANIFEST_DIR not set. Using current directory");
                std::env::current_dir()
            })
            .expect("Failed to a cargo manifest path");
        if !kernel_crate_path.is_dir() {
            panic!(
                "Manifest path does not point to a directory {}",
                kernel_crate_path.to_str().unwrap()
            );
        }
        debug!("Kernel manifest dir path: {}", kernel_crate_path.display());

        let kernel_cargo_toml = kernel_crate_path.join("Cargo.toml");
        if !kernel_cargo_toml.is_file() {
            panic!(
                "Manifest path does not contain a Cargo.toml {}",
                kernel_cargo_toml.to_str().unwrap()
            );
        }

        let kernel_meta = CrateMetadata::new(&kernel_cargo_toml);
        let kernel_names = kernel_meta.get_crate_names();
        if kernel_names.len() != 1 {
            panic!(
                "Expected crate to generate exactly one binary, generates however {}",
                kernel_names.len()
            );
        }

        if !kernel_meta.metadata.workspace_metadata.is_null() {
            panic!(
                "Workspace crates are not supported: {}",
                kernel_cargo_toml.display()
            );
        }
        Manifest {
            crate_name: kernel_names.first().unwrap().to_string(),
            crate_path: kernel_crate_path,
            cargo_toml: kernel_cargo_toml,
            target_dir: kernel_meta.get_target_dir(),
            meta: kernel_meta,
        }
    };

    let bootloader_manifest = {
        let boot_crate_path = kernel_manifest.meta.get_crate_of_dependency("bootloader");
        let boot_cargo_toml = boot_crate_path.join("Cargo.toml");
        if !boot_cargo_toml.is_file() {
            panic!("Couldn't find Cargo.toml in {}", boot_cargo_toml.display())
        }
        let boot_meta = CrateMetadata::new(&boot_cargo_toml);
        let boot_names = boot_meta.get_crate_names();
        if boot_names.len() != 1 {
            panic!(
                "Expected crate to generate exactly one binary, generates however {}",
                boot_names.len()
            );
        }
        if !boot_meta.metadata.workspace_metadata.is_null() {
            panic!(
                "Workspace crates are not supported: {}",
                boot_cargo_toml.display()
            );
        }
        Manifest {
            crate_name: boot_names.first().unwrap().to_string(),
            crate_path: boot_crate_path,
            cargo_toml: boot_cargo_toml,
            target_dir: boot_meta.get_target_dir(),
            meta: boot_meta,
        }
    };

    Manifests {
        bootloader: bootloader_manifest,
        kernel: kernel_manifest,
    }
}
