


use std::path::PathBuf;
use glue_gun::*;

#[test]
fn print_help() {
    let app = create_cli();

    let parse = app.try_get_matches_from(vec!["glue_gun", "--help"]);

    match parse {
        Err(err) => {
            assert!(err.kind() == clap::ErrorKind::DisplayHelp)
        },
        Ok(_) => panic!("Help does not print help")
    };
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
