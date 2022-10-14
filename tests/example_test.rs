use glue_gun::*;
use std::path::PathBuf;

#[test]
fn print_help() {
    let app = create_cli();

    let parse = app.try_get_matches_from(vec!["glue_gun", "--help"]);

    match parse {
        Err(err) => {
            assert!(err.kind() == clap::ErrorKind::DisplayHelp)
        }
        Ok(_) => panic!("Help does not print help"),
    };
}

#[test]
fn test_submodule() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    assert!(
        res.join("perf_kernel").exists(),
        "Missing submodules execute: git submodule update --init --recursive"
    );
}

#[test]
fn build_normal() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    let app = create_cli();
    let cmd = vec!["glue_gun", "build"];

    let matches = app.try_get_matches_from(cmd).unwrap();

    std::env::set_var("CARGO_MANIFEST_DIR", res.join("perf_kernel/kernel"));
    glue_gun::parse_matches(&matches).expect("Failed to execute test");
}

#[test]
fn run_kernel() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    let app = create_cli();
    let cmd = vec!["glue_gun", "run"];

    let matches = app.try_get_matches_from(cmd).unwrap();

    std::env::set_var("CARGO_MANIFEST_DIR", res.join("perf_kernel/kernel"));
    glue_gun::parse_matches(&matches).expect("Failed to execute test");
}
