use log::*;

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process,
};
use std::{fs::OpenOptions, io::Write};

use crate::{CliOptions, Manifests};

#[derive(Debug, Clone)]
pub struct BuildMetadata {
    pub config: crate::config::Config,
    pub is_test: bool,
    pub iso_img: PathBuf,
}

pub fn glue_gun_build(
    kernel_exec_path: &Path,
    manifests: &Manifests,
    cli_options: &CliOptions,
) -> BuildMetadata {
    // Parse kernel Cargo.toml
    let config = crate::config::read_config(&manifests.kernel.cargo_toml).unwrap(); // parsed Cargo.toml

    // Find out through directory names if we running a release
    // or a test version of the binary
    let target_dir;
    let is_release;
    let is_test;
    {
        target_dir = kernel_exec_path
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
        let kernel_sym_name = kernel_exec_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
            + ".sym";
        kernel_sym_path = target_dir.join(kernel_sym_name);
        crate::sym::create_sym_file(kernel_exec_path, &kernel_sym_path, false);
    }

    // Build bootloader crate and set the KERNEL env var
    // to the kernel binary.
    // The bootloader binary has in its data section the kernel.
    // So our bootloader binary is now our "kernel"
    let merged_exe;
    {
        let mut full_kernel_path = manifests.kernel.crate_path.to_owned();
        full_kernel_path.push(kernel_exec_path);
        let env_vars = [("KERNEL", full_kernel_path.to_str().unwrap())];
        let features = ["binary"];
        let exes = cargo_build(
            &manifests.bootloader.crate_path,
            Some(&config),
            is_release,
            cli_options.is_very_verbose,
            Some(&features),
            Some(&env_vars),
        );

        if exes.len() != 1 {
            panic!("bootloader generated more then one executable");
        }

        let exe = &exes[0];
        let dst = exe
            .parent()
            .unwrap()
            .join(kernel_exec_path.file_name().unwrap());
        std::fs::rename(exe, &dst).expect("Failed to rename bootloader executable");

        merged_exe = dst;
    }
    debug!("Merged executable: {:?}", merged_exe);

    // Create bootloader.sym file in target directory
    let bootloader_sym_path;
    {
        let bootloader_sym_name = "bootloader.sym";
        bootloader_sym_path = target_dir.join(bootloader_sym_name);
        crate::sym::create_sym_file(&merged_exe, &bootloader_sym_path, true);
    }

    // Create bochs symbolfile if command bochsym available
    {
        let bochs_sym_name = "combined.bochsym";
        let bochs_sym_path = target_dir.join(bochs_sym_name);
        crate::sym::create_bochs_symfile(
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

pub fn glue_grub(iso_dir: &PathBuf, iso_img: &PathBuf, executable: &PathBuf) {
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

pub fn cargo_build(
    target_crate: &Path,
    config: Option<&crate::config::Config>,
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
    cmd.arg("--message-format").arg("json");

    cmd.stdout(process::Stdio::piped());
    cmd.stderr(process::Stdio::inherit());
    debug!("Running command: {:#?}", cmd);

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
