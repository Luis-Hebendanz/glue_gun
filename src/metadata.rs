use cargo_metadata::Metadata;
use log::*;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Dependency {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CrateMetadata {
    pub metadata: Metadata,
    pub cargo_toml: PathBuf,
}

impl CrateMetadata {
    pub fn new(cargo_toml: &Path) -> Self {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(cargo_toml)
            .exec()
            .unwrap();

        Self {
            metadata,
            cargo_toml: cargo_toml.to_path_buf(),
        }
    }

    pub fn get_target_dir(&self) -> PathBuf {
        PathBuf::from(self.metadata.target_directory.clone())
    }

    pub fn get_crate_names(&self) -> Vec<String> {
        self.metadata
            .workspace_members
            .clone()
            .into_iter()
            .map(|x| {
                x.to_string()
                    .split_ascii_whitespace()
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    pub fn get_recurisve_local_dependencies(&self) -> BTreeSet<Dependency> {
        recursive_local_dependencies(&self.cargo_toml)
    }

    pub fn get_local_dependencies(&self) -> BTreeSet<PathBuf> {
        let current_pkg = self
            .metadata
            .packages
            .iter()
            .find(|p| p.manifest_path == self.cargo_toml)
            .unwrap_or_else(|| {
                panic!(
                    "Couldn't find cargo toml with path {}",
                    self.cargo_toml.display()
                )
            });
        let mut local_deps: BTreeSet<PathBuf> = BTreeSet::new();

        for dep in &current_pkg.dependencies {
            if let Some(dep) = &dep.path {
                local_deps.insert(PathBuf::from(dep));
            }
        }
        local_deps
    }

    pub fn get_crate_of_dependency(&self, dep_name: &str) -> PathBuf {
        let current_pkg = self
            .metadata
            .packages
            .iter()
            .find(|p| p.manifest_path == self.cargo_toml)
            .unwrap_or_else(|| {
                panic!(
                    "Couldn't find dependency {} in {}",
                    dep_name,
                    self.cargo_toml.display()
                )
            });

        let dependency_name = current_pkg
            .dependencies
            .iter()
            .find(|d| d.rename.as_ref().unwrap_or(&d.name) == dep_name)
            .unwrap_or_else(|| {
                panic!(
                    "Couldn't find needed dependency '{}' in {}",
                    dep_name,
                    self.cargo_toml.display()
                )
            })
            .name
            .clone();

        let dependency_pkg = self
            .metadata
            .packages
            .iter()
            .find(|p| p.name == dependency_name)
            .unwrap();

        let dependency_cargo_toml = dependency_pkg
            .manifest_path
            .clone()
            .to_path_buf()
            .into_std_path_buf();
        debug!(
            "Dependency {} crate location: {:?}",
            dep_name, dependency_cargo_toml
        );
        dependency_cargo_toml.parent().unwrap().to_path_buf()
    }
}

fn recursive_local_dependencies(cargo_toml: &Path) -> BTreeSet<Dependency> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(cargo_toml)
        .exec()
        .unwrap();

    let current_pkg = metadata
        .packages
        .iter()
        .find(|p| p.manifest_path == cargo_toml)
        .unwrap_or_else(|| {
            panic!(
                "Couldn't find cargo toml with path {}",
                cargo_toml.display()
            )
        });
    let mut local_deps: BTreeSet<Dependency> = BTreeSet::new();

    for dep in &current_pkg.dependencies {
        if let Some(dep_path) = &dep.path {
            let cargo_toml = PathBuf::from(dep_path).join("Cargo.toml");
            let mydep = Dependency {
                path: PathBuf::from(dep_path),
                name: dep.name.to_string(),
            };
            local_deps.insert(mydep);
            local_deps.append(&mut recursive_local_dependencies(&cargo_toml));
        }
    }
    local_deps
}
