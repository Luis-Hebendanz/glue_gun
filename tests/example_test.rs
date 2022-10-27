use glue_gun::*;
use std::path::PathBuf;
use std::sync::Once;
static START: Once = Once::new();
//Sure to run this once
fn setup_tests() {
    START.call_once(|| {
        simple_logger::SimpleLogger::new()
            .with_level(log::LevelFilter::Trace)
            .with_timestamps(false)
            .init()
            .unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    });
}

#[test]
fn print_help() {
    setup_tests();
    let app = create_cli();

    let parse = app.try_get_matches_from(vec!["glue_gun", "--help"]);

    match parse {
        Err(err) => {
            assert!(err.kind() == clap::error::ErrorKind::DisplayHelp)
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

#[tokio::test]
async fn build_normal() {
    setup_tests();
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    let app = create_cli();
    let cmd = vec!["glue_gun", "build"];

    let matches = app.try_get_matches_from(cmd).unwrap();

    std::env::set_var("CARGO_MANIFEST_DIR", res.join("perf_kernel/kernel"));
    glue_gun::parse_matches(&matches)
        .await
        .expect("Failed to execute test");
}

#[tokio::test]
async fn run_kernel() {
    setup_tests();
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    let app = create_cli();
    let cmd = vec!["glue_gun", "run"];

    let matches = app.try_get_matches_from(cmd).unwrap();

    std::env::set_var("CARGO_MANIFEST_DIR", res.join("perf_kernel/kernel"));
    glue_gun::parse_matches(&matches)
        .await
        .expect("Failed to execute test");
}

#[tokio::test]
async fn clean() {
    setup_tests();
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");

    let app = create_cli();
    let cmd = vec!["glue_gun", "clean"];

    let matches = app.try_get_matches_from(cmd).unwrap();

    std::env::set_var("CARGO_MANIFEST_DIR", res.join("perf_kernel/kernel"));
    glue_gun::parse_matches(&matches)
        .await
        .expect("Failed to execute test");
}
