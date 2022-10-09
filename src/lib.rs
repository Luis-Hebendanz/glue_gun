use clap::ArgMatches;
use log::*;
use std::process::ExitCode;
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process,
};
use std::{fs::OpenOptions, io::Write};

mod run;
mod config;


pub fn  parse_matches(matches: &ArgMatches) -> ExitCode {
    if matches.is_present("verbose") {
        log::set_max_level(LevelFilter::Debug);
    }

    if let Some(matches) = matches.subcommand_matches("run") {
        let is_verbose = matches.is_present("verbose");
        if is_verbose {
            log::set_max_level(LevelFilter::Debug);
        }
        debug!("Args: {:?}", std::env::args());

        let kernel_manifest_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .or_else(|_| {
                info!("CARGO_MANIFEST_DIR not set using current directory");
                std::env::current_dir()
            })
            .expect("Failed to a cargo manifest path");

        let kernel_path: &PathBuf = matches.get_one("kernel").expect("Path to kernel missing");

        let artifacts = build_bootloader(kernel_path, &kernel_manifest_path, is_verbose);

        run::run(
            artifacts.config,
            &artifacts.iso_img,
            artifacts.is_test,
            matches.is_present("debug"),
        )
        .unwrap();
        return ExitCode::SUCCESS;
    }

    if let Some(matches) = matches.subcommand_matches("build") {
        let is_verbose = matches.is_present("verbose");
        if is_verbose {
            log::set_max_level(LevelFilter::Debug);
        }
        debug!("Args: {:?}", std::env::args());

        let kernel_manifest_path: PathBuf = env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .or_else(|_| {
                info!("CARGO_MANIFEST_DIR not set using current directory");
                std::env::current_dir()
            })
            .expect("Failed to a cargo manifest path");

        if !kernel_manifest_path.join("Cargo.toml").is_file() {
            panic!("Couldn't find Cargo.toml in current directory");
        }

        let is_release = matches.is_present("release");
        let kernel_path = cargo_build(
            &kernel_manifest_path,
            None,
            is_release,
            is_verbose,
            None,
            None,
        );

        if kernel_path.len() != 1 {
            panic!(
                "Expected kernel to generate exactly one binary however {} habe been build",
                kernel_path.len()
            );
        }

        build_bootloader(
            kernel_path.first().unwrap(),
            &kernel_manifest_path,
            is_verbose,
        );

        return ExitCode::SUCCESS;
    }
    ExitCode::FAILURE
}

#[derive(Debug, Clone)]
struct BuildMetadata {
    pub config: config::Config,
    pub is_test: bool,
    pub iso_img: PathBuf,
}

fn build_bootloader(kernel_path: &Path, manifest_path: &Path, verbose: bool) -> BuildMetadata {
    let kernel_manifest;
    let kernel_crate;
    let config;
    {
        if !manifest_path.is_dir() {
            panic!(
                "Manifest path does not point to a directory {}",
                manifest_path.to_str().unwrap()
            );
        }
        kernel_manifest = manifest_path.join("Cargo.toml");
        kernel_crate = kernel_manifest
            .parent()
            .expect("Kernel directory does not have a parent dir")
            .to_path_buf();
        debug!("Manifest path: {:#?}", kernel_manifest);

        config = config::read_config(&kernel_manifest).unwrap();
    }

    let bootloader_manifest;
    let bootloader_crate;
    let kernel_pkg;
    {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(kernel_manifest.as_path())
            .exec()
            .unwrap();

        kernel_pkg = metadata
            .packages
            .iter()
            .find(|p| p.manifest_path == kernel_manifest)
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

        bootloader_manifest = bootloader_pkg
            .manifest_path
            .clone()
            .to_path_buf()
            .into_std_path_buf();
        bootloader_crate = bootloader_manifest.parent().unwrap();
    }
    debug!("Bootloader manifest: {:?}", bootloader_manifest);
    debug!("Bootloader crate: {:?}", bootloader_crate);

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

    let merged_exe;
    {
        let mut full_kernel_path = kernel_crate;
        full_kernel_path.push(kernel_path);
        let env_vars = [("KERNEL", full_kernel_path.to_str().unwrap())];
        let features = ["binary"];
        let exes = cargo_build(
            bootloader_crate,
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

    let iso_img;
    {
        let kernel_name = merged_exe.file_stem().unwrap().to_str().unwrap();
        iso_img = target_dir.join(format!("{}.iso", kernel_name));
        let iso_dir = target_dir.join("isofiles");

        println!("Iso for {} -> {}", kernel_name, iso_img.to_str().unwrap());

        glue_grub(&iso_dir, &iso_img, &merged_exe);
    }

    BuildMetadata {
        config,
        iso_img,
        is_test,
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
