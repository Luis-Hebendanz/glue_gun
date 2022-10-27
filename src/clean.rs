use log::*;

use std::{env, path::Path, process};

use crate::CliOptions;

pub fn glue_gun_clean(manifests: &crate::Manifests, cli_options: CliOptions, clean_all: bool) {
    // Clean kernel crate
    let kernel_crate_name: Option<Vec<String>> =
        (!clean_all).then(|| vec![manifests.kernel.crate_name.clone()]);

    cargo_clean(
        &manifests.kernel.crate_path,
        kernel_crate_name,
        cli_options.is_release,
        cli_options.is_very_verbose,
        None,
    );

    // Clean bootloader crate
    let bootloader_crate_names =
        (!clean_all).then(|| vec![manifests.bootloader.crate_name.clone()]);

    cargo_clean(
        &manifests.bootloader.crate_path,
        bootloader_crate_names,
        cli_options.is_release,
        cli_options.is_very_verbose,
        None,
    );
}

pub fn cargo_clean(
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
