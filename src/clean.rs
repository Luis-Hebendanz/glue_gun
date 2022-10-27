use log::*;

use std::{
    env,
    path::{Path, PathBuf},
    process,
};

pub fn glue_gun_clean(
    kernel_manifest_dir_path: &PathBuf,
    bootloader_crate_path: &Path,
    clean_all: bool,
    is_release: bool,
    is_vv: bool,
) {
    // Clean kernel crate
    let kernel_crate_names: Option<Vec<String>> =
        (!clean_all).then(|| crate::get_crate_names(&kernel_manifest_dir_path.join("Cargo.toml")));
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
    let bootloader_crate_names =
        (!clean_all).then(|| crate::get_crate_names(&bootloader_crate_path.join("Cargo.toml")));
    debug!("Bootloader crate: {:?}", bootloader_crate_path);
    debug!("Bootloader crate names: {:?}", bootloader_crate_names);
    cargo_clean(
        bootloader_crate_path,
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
