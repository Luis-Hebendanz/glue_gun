#![allow(dead_code)]

use clap::ArgMatches;
use clap::{value_parser, Arg, SubCommand};
use log::*;

use std::process::ExitCode;
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process,
};
use std::{fs::OpenOptions, io::Write};

mod config;
mod run;

pub fn create_cli() -> clap::App<'static> {
    let app = clap::Command::new("glue_gun")
        .author("Luis Hebendanz <luis.nixos@gmail.com")
        .about("Glues together a rust bootloader and ELF kernel to generate a bootable ISO file")
        .arg(
            Arg::with_name("verbose")
                .short('v')
                .long("verbose")
                .help("Enables verbose mode")
                .global(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("vv")
                .long("vv")
                .help("Enables very verbose mode")
                .global(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("kernel")
                .help("Path to kernel executable")
                .short('k')
                .long("kernel")
                .global(true)
                .takes_value(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::with_name("release")
                .global(true)
                .help("Building in release mode")
                .long("release")
                .short('r')
                .conflicts_with("kernel")
                .takes_value(false),
        )
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .help_expected(true)
        .subcommand(SubCommand::with_name("build").about("Builds the ISO file"))
        .subcommand(
            SubCommand::with_name("run")
                .about("Builds and runs the ISO file")
                .arg(
                    Arg::with_name("debug")
                        .help("Runs the emulator in debug mode")
                        .short('d')
                        .long("debug")
                        .required(false)
                        .takes_value(false),
                ),
        );

    app
}

pub fn parse_matches(matches: &ArgMatches) -> Result<(), ExitCode> {
    let is_release = matches.is_present("release");
    let is_verbose = matches.is_present("verbose");
    let is_vv = matches.is_present("vv");
    if is_verbose {
        log::set_max_level(LevelFilter::Debug);
    }
    debug!("Args: {:?}", std::env::args());

    let kernel_manifest_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            info!("CARGO_MANIFEST_DIR not set. Using current directory");
            std::env::current_dir()
        })
        .expect("Failed to a cargo manifest path");

    if !kernel_manifest_path.join("Cargo.toml").is_file() {
        panic!("Couldn't find Cargo.toml in {:?}", kernel_manifest_path);
    }

    let kernel_path: PathBuf = {
        match matches.get_one::<PathBuf>("kernel") {
            Some(path) => path.clone(),
            None => {
                let kernel_path = cargo_build(
                    &kernel_manifest_path,
                    None,
                    is_release,
                    is_vv,
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

    let artifacts = build_bootloader(&kernel_path, &kernel_manifest_path, is_vv);

    if let Some(matches) = matches.subcommand_matches("run") {
        run::run(
            artifacts.config,
            &artifacts.iso_img,
            artifacts.is_test,
            matches.is_present("debug"),
        )
        .unwrap();
        return Ok(());
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct BuildMetadata {
    pub config: config::Config,
    pub is_test: bool,
    pub iso_img: PathBuf,
}

fn build_bootloader(
    kernel_path: &Path,
    kernel_manifest_dir_path: &Path,
    verbose: bool,
) -> BuildMetadata {
    // Parse kernel Cargo.toml
    let kernel_manifest_file_path;
    let config; // parsed Cargo.toml
    {
        if !kernel_manifest_dir_path.is_dir() {
            panic!(
                "Manifest path does not point to a directory {}",
                kernel_manifest_dir_path.to_str().unwrap()
            );
        }
        kernel_manifest_file_path = kernel_manifest_dir_path.join("Cargo.toml");
        debug!("Manifest path: {:#?}", kernel_manifest_file_path);

        config = config::read_config(&kernel_manifest_file_path).unwrap();
    }

    // Find directory of bootloader
    let bootloader_crate: PathBuf;
    {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(kernel_manifest_file_path.as_path())
            .exec()
            .unwrap();

        let kernel_pkg = metadata
            .packages
            .iter()
            .find(|p| p.manifest_path == kernel_manifest_file_path)
            .expect("Couldn't find package with same manifest as kernel in metadata");

        let bootloader_name = kernel_pkg
            .dependencies
            .iter()
            .find(|d| d.rename.as_ref().unwrap_or(&d.name) == "bootloader")
            .expect("Couldn't find needed dependencie 'bootloader' in kernel")
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
        bootloader_crate = bootloader_manifest.parent().unwrap().to_path_buf();
        debug!("Bootloader manifest: {:?}", bootloader_manifest);
    }
    debug!("Bootloader crate: {:?}", bootloader_crate);

    // Find out through directory names if we running a release
    // or a test version of the binary
    let target_dir;
    let is_release;
    let is_test;
    {
        target_dir = kernel_path
            .parent()
            .expect("Target executable does not have a parent directory")
            .to_path_buf();
        is_release = target_dir.iter().last().unwrap() == OsStr::new("release");

        let is_doctest = target_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("rustdoctest");
        is_test = is_doctest || target_dir.ends_with("deps");
    }
    debug!("Building in release mode? {}", is_release);
    debug!("Running a test? {}", is_test);

    // Create kernel.sym file in target directory
    {
        let kernel_sym_name = kernel_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
            + ".sym";
        let kernel_sym_path = target_dir.join(kernel_sym_name);
        create_sym_file(kernel_path, &kernel_sym_path, false);
    }

    // Build bootloader crate and set the KERNEL env var
    // to the kernel binary.
    // The bootloader binary has in its data section the kernel.
    // So our bootloader binary is now our "kernel"
    let merged_exe;
    {
        let mut full_kernel_path = kernel_manifest_dir_path.to_owned();
        full_kernel_path.push(kernel_path);
        let env_vars = [("KERNEL", full_kernel_path.to_str().unwrap())];
        let features = ["binary"];
        let exes = cargo_build(
            &bootloader_crate,
            Some(&config),
            is_release,
            verbose,
            Some(&features),
            Some(&env_vars),
        );

        if exes.len() != 1 {
            panic!("bootloader generated more then one executable");
        }

        let exe = &exes[0];
        let dst = exe.parent().unwrap().join(kernel_path.file_name().unwrap());
        std::fs::rename(exe, &dst).expect("Failed to rename bootloader executable");

        merged_exe = dst;
    }
    debug!("Merged executable: {:?}", merged_exe);

    // Create bootloader.sym file in target directory
    {
        let bootloader_sym_name = "bootloader.sym";
        let bootloader_sym_path = target_dir.join(bootloader_sym_name);
        create_sym_file(&merged_exe, &bootloader_sym_path, true);
    }

    // Create an ISO image from our merged exe
    let iso_img;
    let iso_dir;
    {
        let kernel_name = merged_exe.file_stem().unwrap().to_str().unwrap();
        iso_img = target_dir.join(format!("{}.iso", kernel_name));
        iso_dir = target_dir.join("isofiles");
        println!("Iso for {} -> {}", kernel_name, iso_img.to_str().unwrap());

        glue_grub(&iso_dir, &iso_img, &merged_exe);
    }

    BuildMetadata {
        config,
        iso_img,
        is_test,
    }
}

fn create_sym_file(in_path: &Path, out_path: &Path, strip_in: bool) {
    use std::process::Command;
    // get access to llvm tools shipped in the llvm-tools-preview rustup component
    let llvm_tools = match llvm_tools::LlvmTools::new() {
        Ok(tools) => tools,
        Err(llvm_tools::Error::NotFound) => {
            eprintln!("Error: llvm-tools not found");
            eprintln!("Maybe the rustup component `llvm-tools-preview` is missing?");
            eprintln!("  Install it through: `rustup component add llvm-tools-preview`");
            process::exit(1);
        }
        Err(err) => {
            eprintln!("Failed to retrieve llvm-tools component: {:?}", err);
            process::exit(1);
        }
    };

    let objcopy = llvm_tools
        .tool(&llvm_tools::exe("llvm-objcopy"))
        .expect("llvm-objcopy not found in llvm-tools");

    // Create separate symbol file
    let mut cmd = Command::new(&objcopy);
    cmd.arg("--only-keep-debug");
    cmd.arg(in_path);
    cmd.arg(out_path);
    debug!("Executing:\n {:#?}", cmd);
    let exit_status = cmd
        .status()
        .expect("failed to run objcopy to separate debug symbols");
    if !exit_status.success() {
        eprintln!("Error: Separating debug symbols failed");
        process::exit(1);
    }

    if strip_in {
        // Strip symbols inplace from in_path
        let mut cmd = Command::new(&objcopy);
        cmd.arg("--strip-debug");
        cmd.arg(in_path);
        cmd.arg(in_path);
        debug!("Executing: {:#?}", cmd);
        let exit_status = cmd
            .status()
            .expect("failed to run objcopy to strip debug symbols");
        if !exit_status.success() {
            eprintln!("Error: Stripping debug symbols failed");
            process::exit(1);
        }
    }
}

fn glue_grub(iso_dir: &PathBuf, iso_img: &PathBuf, executable: &PathBuf) {
    match std::fs::create_dir(iso_dir) {
        Ok(_) => (),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
            } else {
                panic!(
                    "{} Failed to create iso dir {}",
                    e,
                    iso_dir.to_str().unwrap()
                );
            }
        }
    };

    let grub_dir = iso_dir.join("boot/grub");
    match std::fs::create_dir_all(&grub_dir) {
        Ok(_) => (),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
            } else {
                panic!(
                    "{} Failed to create iso dir {}",
                    e,
                    iso_dir.to_str().unwrap()
                );
            }
        }
    };

    let mut grubcfg = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&grub_dir.join("grub.cfg"))
        .unwrap();

    grubcfg
        .write_all(
            r#"
            set timeout=0
            set default=0

            menuentry "kernel" {
                multiboot2 /boot/kernel.elf
                boot
            }
            "#
            .as_bytes(),
        )
        .unwrap();

    std::fs::copy(executable, iso_dir.join("boot/kernel.elf")).unwrap();

    let mut cmd = process::Command::new("grub-mkrescue");
    cmd.arg("-o").arg(iso_img);
    cmd.arg(iso_dir);

    let output = cmd.output().expect("Failed to build bootloader crate");
    if !output.status.success() {
        panic!(
            "Failed to build grub image: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        );
    }
}

fn cargo_build(
    target_crate: &Path,
    config: Option<&config::Config>,
    is_release: bool,
    is_verbose: bool,
    features: Option<&[&str]>,
    env: Option<&[(&str, &str)]>,
) -> Vec<PathBuf> {
    let mut executables = Vec::new();

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = process::Command::new(&cargo);
    cmd.current_dir(target_crate);
    if let Some(env) = env {
        for (key, val) in env {
            cmd.env(key, val);
        }
        debug!("Env vars: {:?}", env);
    }

    if let Some(config) = config {
        cmd.args(config.build_command.clone());
    } else {
        cmd.arg("build");
    }

    if let Some(features) = features {
        cmd.arg(format!(
            "--features={}",
            features
                .iter()
                .fold("".to_string(), |acc, x| format!("{},{}", acc, x))
        ));
    }

    if is_release {
        cmd.arg("--release");
    }

    if is_verbose {
        cmd.arg("-vv");
    }

    cmd.stdout(process::Stdio::inherit());
    cmd.stderr(process::Stdio::inherit());
    debug!("Running command: {:#?}", cmd);

    let output = cmd.output().expect("Failed to build bootloader crate");
    if !output.status.success() {
        panic!("Failed to build bootloader crate");
    }

    // Redo build just to parse out json and get executable paths
    cmd.arg("--message-format").arg("json");
    cmd.stderr(process::Stdio::piped());
    cmd.stdout(process::Stdio::piped());
    let output = cmd.output().expect("Failed to build bootloader crate");
    if !output.status.success() {
        panic!(
            "Failed to build bootloader crate: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        );
    }
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let mut artifact = json::parse(line).expect("Failed parsing json from cargo");
        if let Some(executable) = artifact["executable"].take_string() {
            executables.push(PathBuf::from(executable));
        }
    }
    executables
}
