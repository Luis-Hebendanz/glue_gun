#![allow(dead_code)]

use clap::ArgMatches;
use clap::{value_parser, Arg};
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

pub fn parse_matches(matches: &ArgMatches) -> Result<(), ExitCode> {
    let is_release = matches.get_flag("release");
    let is_verbose = matches.get_count("verbose") >= 1;
    let is_vv = matches.get_count("verbose") > 1;

    if is_verbose {
        log::set_max_level(LevelFilter::Debug);
    }
    debug!("Args: {:?}", std::env::args());

    let kernel_manifest_dir_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            debug!("CARGO_MANIFEST_DIR not set. Using current directory");
            std::env::current_dir()
        })
        .expect("Failed to a cargo manifest path");

    debug!(
        "Kernel manifest dir path: {}",
        kernel_manifest_dir_path.display()
    );

    if let Some(matches) = matches.subcommand_matches("clean") {
        let is_all = matches.get_flag("all");
        glue_gun_clean(&kernel_manifest_dir_path, is_all, is_release, is_vv);
        return Ok(());
    }

    // If subcommand 'build' or 'run'
    let kernel_path: PathBuf = {
        match matches.get_one::<PathBuf>("kernel") {
            Some(path) => path.clone(),
            None => {
                let kernel_path = cargo_build(
                    &kernel_manifest_dir_path,
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

    let artifacts = build_bootloader(&kernel_path, &kernel_manifest_dir_path, is_vv);

    if let Some(matches) = matches.subcommand_matches("run") {
        run::run(
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

pub fn glue_gun_clean(
    kernel_manifest_dir_path: &PathBuf,
    clean_all: bool,
    is_release: bool,
    is_vv: bool,
) {
    // Clean kernel crate
    let kernel_crate_names: Option<Vec<String>> =
        (!clean_all).then(|| get_crate_names(&kernel_manifest_dir_path.join("Cargo.toml")));
    debug!("Kernel crate: {:?}", kernel_manifest_dir_path);
    debug!("Kernel crate names: {:?}", kernel_crate_names);
    cargo_clean(
        kernel_manifest_dir_path,
        kernel_crate_names,
        is_release,
        is_vv,
        None,
    );

    // Clean bootloader crate
    let bootloader_crate_path =
        get_bootloader_crate_path(&kernel_manifest_dir_path.join("Cargo.toml"));
    let bootloader_crate_names =
        (!clean_all).then(|| get_crate_names(&bootloader_crate_path.join("Cargo.toml")));
    debug!("Bootloader crate: {:?}", bootloader_crate_path);
    debug!("Bootloader crate names: {:?}", bootloader_crate_names);
    cargo_clean(
        &bootloader_crate_path,
        bootloader_crate_names,
        is_release,
        is_vv,
        None,
    );
}

fn cargo_clean(
    target_crate: &Path,
    dep_names: Option<Vec<String>>,
    is_release: bool,
    is_verbose: bool,
    env: Option<&[(&str, &str)]>,
) {
    info!(
        "Cleaning crate {}",
        target_crate.file_name().unwrap().to_str().unwrap()
    );

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = process::Command::new(&cargo);
    cmd.current_dir(target_crate);
    if let Some(env) = env {
        for (key, val) in env {
            cmd.env(key, val);
        }
        debug!("Env vars: {:?}", env);
    }
    cmd.arg("clean");

    if let Some(dep_names) = dep_names {
        for dep_name in dep_names {
            cmd.arg("--package");
            cmd.arg(dep_name);
        }
    }

    if is_verbose {
        cmd.arg("-vv");
    }

    if is_release {
        cmd.arg("--release");
    }

    cmd.stdout(process::Stdio::inherit());
    cmd.stderr(process::Stdio::inherit());
    debug!("Running command: {:#?}", cmd);

    let output = cmd.output().expect("Failed to clean crate");
    if !output.status.success() {
        panic!("Failed to clean crate");
    }
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
    let bootloader_crate = get_bootloader_crate_path(&kernel_manifest_file_path);
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
    let kernel_sym_path;
    {
        let kernel_sym_name = kernel_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
            + ".sym";
        kernel_sym_path = target_dir.join(kernel_sym_name);
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
    let bootloader_sym_path;
    {
        let bootloader_sym_name = "bootloader.sym";
        bootloader_sym_path = target_dir.join(bootloader_sym_name);
        create_sym_file(&merged_exe, &bootloader_sym_path, true);
    }

    // Create bochs symbolfile if command bochsym available
    {
        let bochs_sym_name = "combined.bochsym";
        let bochs_sym_path = target_dir.join(bochs_sym_name);
        create_bochs_symfile(
            [bootloader_sym_path.as_path(), kernel_sym_path.as_path()],
            &bochs_sym_path,
        );
    }

    // Create an ISO image from our merged exe
    let iso_img;
    let iso_dir;
    {
        let kernel_name = merged_exe.file_stem().unwrap().to_str().unwrap();
        iso_img = target_dir.join(format!("{}.iso", kernel_name));
        iso_dir = target_dir.join("isofiles");
        info!("Created Iso image at: {}", iso_img.to_str().unwrap());

        glue_grub(&iso_dir, &iso_img, &merged_exe);
    }

    BuildMetadata {
        config,
        iso_img,
        is_test,
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

fn get_bootloader_crate_path(manifest_file_path: &Path) -> PathBuf {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_file_path)
        .exec()
        .unwrap();

    let kernel_pkg = metadata
        .packages
        .iter()
        .find(|p| p.manifest_path == manifest_file_path)
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
    debug!("Bootloader manifest: {:?}", bootloader_manifest);
    bootloader_manifest.parent().unwrap().to_path_buf()
}

fn create_bochs_symfile<'a, I>(symfiles: I, out_path: &Path)
where
    I: IntoIterator<Item = &'a Path>,
{
    use std::process::Command;
    let mut cmd = Command::new("bochsym");
    for i in symfiles.into_iter() {
        cmd.arg("--symfile").arg(i);
    }
    cmd.arg("-o");
    cmd.arg(out_path);
    debug!("Executing:\n {:#?}", cmd);
    let exit_status = match cmd.status() {
        Ok(exit_status) => exit_status,
        Err(_) => {
            warn!("Missing cli tool bochsym. Skipping creation of symbol file for bochs emulator");
            return;
        }
    };
    if !exit_status.success() {
        eprintln!("Error: bochsym exited with nonzero");
        process::exit(1);
    }

    info!(
        "Created bochs symbol file: {}",
        out_path.file_name().unwrap().to_str().unwrap()
    );
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

    info!(
        "Created symbol file: {}",
        out_path.file_name().unwrap().to_str().unwrap()
    );
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
        debug!("Stripped symbols from {}", in_path.display());
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
    info!(
        "Building crate {}",
        target_crate.file_name().unwrap().to_str().unwrap()
    );
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
