use log::*;

use std::path::Path;

pub async fn glue_gun_watch(kernel_manifest_dir_path: &Path, bootloader_crate_path: &Path) {
    use watchexec::{
        action::{Action, Outcome},
        config::{InitConfig, RuntimeConfig},
        error::RuntimeError,
        handler::PrintDebug,
        signal::source::MainSignal,
        Watchexec,
    };

    let mut init = InitConfig::default();
    init.on_error(PrintDebug(std::io::stderr()));

    let mut runtime = RuntimeConfig::default();
    runtime.pathset([bootloader_crate_path, kernel_manifest_dir_path]);

    let we = Watchexec::new(init, runtime.clone()).unwrap();

    runtime.on_action(move |action: Action| async move {
        for event in action.events.iter() {
            info!("event: {:?}", event);

            if event.signals().any(|x| {
                matches!(
                    x,
                    MainSignal::Interrupt | MainSignal::Quit | MainSignal::Terminate
                )
            }) {
                action.outcome(Outcome::both(Outcome::Stop, Outcome::Exit));
                return Ok::<(), RuntimeError>(());
            }
        }

        action.outcome(Outcome::DoNothing);
        Ok::<(), RuntimeError>(())
    });

    we.reconfigure(runtime).unwrap();
    we.main().await.unwrap().unwrap();
}
