use log::*;

use std::{path::Path, process};

pub fn create_bochs_symfile<'a, I>(symfiles: I, out_path: &Path)
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

pub fn create_sym_file(in_path: &Path, out_path: &Path, strip_in: bool) {
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
