use std::path::Path;

use log::*;

use crate::{CliOptions, Manifests};

pub async fn glue_gun_watch<'a>(
    kernel_exec_path: &'a Path,
    manifests: &'a Manifests,
    cli_options: &'a CliOptions,
) {
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
    runtime.pathset([
        manifests.bootloader.crate_path.clone(),
        manifests.kernel.crate_path.clone(),
    ]);

    let we = Watchexec::new(init, runtime.clone()).unwrap();

    runtime.on_action(move |action: Action| {
        let fut = async { Ok::<(), RuntimeError>(()) };

        // TODO: FIX ME!!!
        let kernel_exec_path = kernel_exec_path.clone();

        for event in action.events.iter() {
            info!("event: {:?}", event);

            if event.signals().any(|x| {
                matches!(
                    x,
                    MainSignal::Interrupt | MainSignal::Quit | MainSignal::Terminate
                )
            }) {
                action.outcome(Outcome::both(Outcome::Stop, Outcome::Exit));
                return fut;
            }
        }

        //TODO: Put build code here
        // let artifacts = crate::build::glue_gun_build(
        //     &kernel_exec_path.clone(),
        //     &manifests.clone(),
        //     &cli_options.clone(),
        // );
        //action.outcome(Outcome::DoNothing);
        fut
    });

    we.reconfigure(runtime).unwrap();
    we.main().await.unwrap().unwrap();
}
