use std::path::PathBuf;

use crate::{run, CliOptions, Manifests};
use ignore::WalkBuilder;
use log::*;
use std::collections::BTreeSet;
use watchexec::{
    action::{Action, Outcome},
    config::{InitConfig, RuntimeConfig},
    error::RuntimeError,
    handler::PrintDebug,
    signal::source::MainSignal,
    Watchexec,
};

pub fn compute_whitelist<I>(crates: I) -> BTreeSet<PathBuf>
where
    I: IntoIterator<Item = std::path::PathBuf>,
{
    let watch_files: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());
    for cr in crates {
        WalkBuilder::new(cr).build_parallel().run(|| {
            Box::new(
                |result: Result<ignore::DirEntry, ignore::Error>| match result {
                    Ok(entry) => {
                        debug!("{}", entry.path().display());
                        let mut watch_files = watch_files.lock().expect("Failed to get lock");
                        watch_files.insert(entry.into_path());
                        ignore::WalkState::Continue
                    }
                    Err(err) => {
                        error!("{}", err);
                        ignore::WalkState::Skip
                    }
                },
            )
        });
    }
    let watch_files = watch_files.lock().expect("Failed to get lock");
    watch_files.iter().map(|x| x.to_path_buf()).collect()
}

#[derive(Debug, Default)]
struct MyFilter {
    ignore_files: BTreeSet<PathBuf>,
}
use std::sync::Arc;
use watchexec::filter::Filterer;

impl MyFilter {
    pub fn new(ignore_files: BTreeSet<PathBuf>) -> Self {
        Self { ignore_files }
    }

    pub fn add_ignored(&mut self, path: PathBuf) {
        self.ignore_files.insert(path);
    }
}

impl Filterer for MyFilter {
    fn check_event(
        &self,
        event: &watchexec::event::Event,
        _priority: watchexec::event::Priority,
    ) -> Result<bool, RuntimeError> {
        for (p, _) in event.paths() {
            for ignore_path in self.ignore_files.iter() {
                if p.starts_with(ignore_path) {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }
}

#[allow(clippy::await_holding_lock)]
pub async fn glue_gun_watch(
    kernel_exec_path: std::path::PathBuf,
    manifests: Manifests,
    cli_options: CliOptions,
) {
    // General default init
    let mut init = InitConfig::default();
    init.on_error(PrintDebug(std::io::stderr()));
    let mut runtime = RuntimeConfig::default();

    let watch_dirs = [
        manifests.bootloader.crate_path.clone(),
        manifests.kernel.crate_path.clone(),
    ];

    // Compute a file whitelist, adhering to the .gitignore file
    let mut file_whitelist = compute_whitelist(watch_dirs.clone());
    for x in file_whitelist.clone() {
        println!("{}", x.display());
    }

    // Set watch command to notify on these directories
    runtime.pathset(watch_dirs.clone());

    // Add event filter
    let mut myfilter = MyFilter::default();
    myfilter.add_ignored(manifests.bootloader.target_dir.clone());
    myfilter.add_ignored(manifests.kernel.target_dir.clone());
    runtime.filterer(Arc::new(myfilter));

    // Init
    let we = Watchexec::new(init, runtime.clone()).unwrap();

    // Block below gets executed on file change
    runtime.on_action(move |action: Action| {
        let kernel_exec_path = kernel_exec_path.clone();
        let manifests = manifests.clone();
        let file_whitelist = file_whitelist.clone();

        async move {
            let fut = Ok::<(), RuntimeError>(());

            // Iter over events
            for event in action.events.iter() {
                debug!("event: {:?}", event);

                // Check for Ctrl+C or SIGTERM signal
                if event.signals().any(|x| {
                    matches!(
                        x,
                        MainSignal::Interrupt | MainSignal::Quit | MainSignal::Terminate
                    )
                }) {
                    action.outcome(Outcome::both(Outcome::Stop, Outcome::Exit));
                    return fut;
                }

                let mut is_whitelisted = event
                    .paths()
                    .any(|p| file_whitelist.contains(&p.0.to_path_buf()));

                // Iterator over file event kind (created, modified, etc)
                let event_kinds = event.tags.iter().filter_map(|p| match p {
                    watchexec::event::Tag::FileEventKind(event_kind) => Some(event_kind),
                    _ => None,
                });

                for ek in event_kinds {
                    // // If file has been created recompute file whitelist
                    // if e.is_create() {
                    //     file_whitelist = compute_whitelist(watch_dirs.clone());
                    //     is_whitelisted = event
                    //         .paths()
                    //         .any(|p| file_whitelist.contains(&p.0.to_path_buf()));
                    // }

                    // If file has been modified or removed and is in whitelist
                    // rebuild and return.
                    if (ek.is_modify() || ek.is_remove()) && is_whitelisted {
                        info!("file changed: {:?}", event);

                        let _artifacts = crate::build::glue_gun_build(
                            &kernel_exec_path.clone(),
                            &manifests.clone(),
                            &cli_options.clone(),
                        )
                        .await;

                        return fut;
                    }
                }
            }

            action.outcome(Outcome::DoNothing);
            fut
        }
    });

    we.reconfigure(runtime).unwrap();
    we.main().await.unwrap().unwrap();
}
