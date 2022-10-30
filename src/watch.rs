use std::path::{Path, PathBuf};

use crate::{CliOptions, Manifests};

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

#[derive(Debug, Default)]
struct Watchlist {
    watch: BTreeSet<PathBuf>,
}

impl Watchlist {
    pub fn add(&mut self, root: &Path, str: &str) {
        let a = root.join(str);
        if a.exists() {
            self.watch.insert(a);
        }
    }

    pub fn get(&self) -> &BTreeSet<PathBuf> {
        &self.watch
    }

    pub fn append<I>(&mut self, data: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        for crate_root in data {
            self.add_default(&crate_root);
        }
    }
    pub fn add_default(&mut self, root: &Path) {
        self.add(root, "Cargo.toml");
        self.add(root, "build.rs");
        self.add(root, "src");
        self.add(root, "tests");
        self.add(root, "Cargo.lock");
    }

    pub fn new() -> Self {
        Self {
            watch: BTreeSet::new(),
        }
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

    let mut crates_to_watch: BTreeSet<PathBuf> = BTreeSet::new();
    crates_to_watch.insert(manifests.bootloader.crate_path.clone());
    crates_to_watch.insert(manifests.kernel.crate_path.clone());

    let mut boot_deps: BTreeSet<PathBuf> = manifests
        .bootloader
        .meta
        .get_recurisve_local_dependencies()
        .iter()
        .map(|x| x.path.clone())
        .collect();
    crates_to_watch.append(&mut boot_deps);

    let mut kernel_deps: BTreeSet<PathBuf> = manifests
        .kernel
        .meta
        .get_recurisve_local_dependencies()
        .iter()
        .map(|x| x.path.clone())
        .collect();
    crates_to_watch.append(&mut kernel_deps);

    for cr in &crates_to_watch {
        println!("{}", cr.display());
    }

    let mut watchlist = Watchlist::new();
    watchlist.append(crates_to_watch);

    // Set watch command to notify on these directories
    runtime.pathset(watchlist.get());

    // Init
    let we = Watchexec::new(init, runtime.clone()).unwrap();

    // Block below gets executed on file change
    runtime.on_action(move |action: Action| {
        let fut = async { Ok::<(), RuntimeError>(()) };

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

            // Iterator over file event kind (created, modified, etc)
            let event_kinds = event.tags.iter().filter_map(|p| match p {
                watchexec::event::Tag::FileEventKind(event_kind) => Some(event_kind),
                _ => None,
            });

            for ek in event_kinds {
                // If file has been modified or removed and is in whitelist
                // rebuild and return.
                if ek.is_modify() || ek.is_remove() || ek.is_create() {
                    info!("file changed: {:?}", event);

                    let _artifacts =
                        crate::build::glue_gun_build(&kernel_exec_path, &manifests, &cli_options);

                    return fut;
                }
            }
        }

        action.outcome(Outcome::DoNothing);
        fut
    });

    we.reconfigure(runtime).unwrap();
    we.main().await.unwrap().unwrap();
}
