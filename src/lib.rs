#![allow(dead_code)]

use clap::ArgMatches;
use clap::{value_parser, Arg};
use log::*;

use std::process::ExitCode;

use std::{
    env,
    path::{Path, PathBuf},
};

mod build;
mod clean;
mod config;
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

    if let Some(_matches) = matches.subcommand_matches("watch") {
        crate::watch::glue_gun_watch(&manifests, cli_options).await;
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

    let artifacts = crate::build::glue_gun_build(&kernel_exec_path, &manifests, cli_options);

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

pub struct Manifest {
    crate_name: String,
    crate_path: PathBuf,
    cargo_toml: PathBuf,
}

pub struct Manifests {
    kernel: Manifest,
    bootloader: Manifest,
}

fn get_crate_paths() -> Manifests {
    let kernel_manifest_dir_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            debug!("CARGO_MANIFEST_DIR not set. Using current directory");
            std::env::current_dir()
        })
        .expect("Failed to a cargo manifest path");
    if !kernel_manifest_dir_path.is_dir() {
        panic!(
            "Manifest path does not point to a directory {}",
            kernel_manifest_dir_path.to_str().unwrap()
        );
    }
    debug!(
        "Kernel manifest dir path: {}",
        kernel_manifest_dir_path.display()
    );

    let kernel_manifest_file_path = kernel_manifest_dir_path.join("Cargo.toml");
    if !kernel_manifest_file_path.is_file() {
        panic!(
            "Manifest path does not contain a Cargo.toml {}",
            kernel_manifest_file_path.to_str().unwrap()
        );
    }

    let kernel_name: String = {
        let kernel_names = get_crate_names(&kernel_manifest_file_path);
        if kernel_names.len() > 1 {
            panic!("Crate generates more then one binary. Only one supported");
        }

        kernel_names
            .get(0)
            .expect("Failed to get crate name")
            .to_string()
    };

    let bootloader_crate_path = get_dep_crate_path(&kernel_manifest_file_path, "bootloader");
    let bootloader_manifest_path = bootloader_crate_path.join("Cargo.toml");
    if !bootloader_manifest_path.is_file() {
        panic!(
            "Manifest path does not contain a Cargo.toml {}",
            bootloader_manifest_path.to_str().unwrap()
        );
    }

    Manifests {
        kernel: Manifest {
            crate_name: kernel_name,
            crate_path: kernel_manifest_dir_path,
            cargo_toml: kernel_manifest_file_path,
        },
        bootloader: Manifest {
            crate_name: "bootloader".to_string(),
            crate_path: bootloader_crate_path,
            cargo_toml: bootloader_manifest_path,
        },
    }
}

fn get_crate_names(manifest_file_path: &Path) -> Vec<String> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_file_path)
        .exec()
        .unwrap();
    metadata
        .workspace_members
        .into_iter()
        .map(|x| {
            x.to_string()
                .split_ascii_whitespace()
                .next()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn get_dep_crate_path(manifest_file: &Path, dep_name: &str) -> PathBuf {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_file)
        .exec()
        .expect("Manifest path is incorrect");

    let kernel_pkg = metadata
        .packages
        .iter()
        .find(|p| p.manifest_path == manifest_file)
        .expect("Couldn't find package with same manifest as kernel in metadata");

    let bootloader_name = kernel_pkg
        .dependencies
        .iter()
        .find(|d| d.rename.as_ref().unwrap_or(&d.name) == dep_name)
        .unwrap_or_else(|| panic!("Couldn't find needed dependency '{}' in kernel", dep_name))
        .name
        .clone();

    let bootloader_pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == bootloader_name)
        .unwrap();

    let bootloader_manifest = bootloader_pkg
        .manifest_path
        .clone()
        .to_path_buf()
        .into_std_path_buf();
    debug!(
        "Dependency {} crate location: {:?}",
        dep_name, bootloader_manifest
    );
    bootloader_manifest.parent().unwrap().to_path_buf()
}
