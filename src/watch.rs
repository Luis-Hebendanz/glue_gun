use log::*;

use crate::Manifests;

pub async fn glue_gun_watch(manifests: &Manifests) {
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

        //TODO: Put build code here
        action.outcome(Outcome::DoNothing);
        Ok::<(), RuntimeError>(())
    });

    we.reconfigure(runtime).unwrap();
    we.main().await.unwrap().unwrap();
}
