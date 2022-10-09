#![allow(dead_code)]

use log::*;

mod config;
mod run;
fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Trace)
        .with_timestamps(false)
        .init()
        .unwrap();
    log::set_max_level(LevelFilter::Info);

    let app = glue_gun::create_cli();
    let matches = app.get_matches();
    glue_gun::parse_matches(&matches).unwrap();
}
