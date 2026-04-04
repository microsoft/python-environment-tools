// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

mod env_variables;
mod environment_locations;
mod environments;

use crate::env_variables::EnvVariables;
#[cfg(windows)]
use environments::list_store_pythons;
use pet_core::env::PythonEnv;
use pet_core::python_environment::{PythonEnvironment, PythonEnvironmentKind};
use pet_core::reporter::Reporter;
use pet_core::LocatorKind;
use pet_core::{
    os_environment::Environment, Locator, RefreshStatePersistence, RefreshStateSyncScope,
};
use std::path::Path;
use std::sync::{Arc, RwLock};

pub fn is_windows_app_folder_in_program_files(path: &Path) -> bool {
    path.to_str().unwrap_or_default().to_string().to_lowercase()[1..]
        .starts_with(":\\program files\\windowsapps")
}

pub struct WindowsStore {
    pub env_vars: EnvVariables,
    #[allow(dead_code)]
    environments: Arc<RwLock<Option<Vec<PythonEnvironment>>>>,
}

impl WindowsStore {
    pub fn from(environment: &dyn Environment) -> WindowsStore {
        WindowsStore {
            env_vars: EnvVariables::from(environment),
            environments: Arc::new(RwLock::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<Vec<PythonEnvironment>> {
        // First check if we have cached results
        {
            let environments = self.environments.read().unwrap();
            if let Some(environments) = environments.clone() {
                return Some(environments);
            }
        }

        let envs = list_store_pythons(&self.env_vars).unwrap_or_default();
        self.environments.write().unwrap().replace(envs.clone());
        Some(envs)
    }
    #[cfg(windows)]
    fn clear(&self) {
        self.environments.write().unwrap().take();
    }

    fn sync_environments_from(&self, source: &WindowsStore) {
        let environments = source.environments.read().unwrap().clone();
        self.environments.write().unwrap().clone_from(&environments);
    }
}

impl Locator for WindowsStore {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::WindowsStore
    }
    fn refresh_state(&self) -> RefreshStatePersistence {
        RefreshStatePersistence::SyncedDiscoveryState
    }
    fn sync_refresh_state_from(&self, source: &dyn Locator, scope: &RefreshStateSyncScope) {
        let source = source
            .as_any()
            .downcast_ref::<WindowsStore>()
            .unwrap_or_else(|| {
                panic!(
                    "attempted to sync WindowsStore state from {:?}",
                    source.get_kind()
                )
            });

        match scope {
            RefreshStateSyncScope::Full => self.sync_environments_from(source),
            RefreshStateSyncScope::GlobalFiltered(kind)
                if self.supported_categories().contains(kind) =>
            {
                self.sync_environments_from(source)
            }
            RefreshStateSyncScope::GlobalFiltered(_) | RefreshStateSyncScope::Workspace => {}
        }
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::WindowsStore]
    }

    #[cfg(windows)]
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        use std::path::PathBuf;

        use pet_core::python_environment::PythonEnvironmentBuilder;
        use pet_fs::path::norm_case;
        use pet_virtualenv::is_virtualenv;

        // Helper to normalize paths for comparison by stripping \\?\ prefix
        fn normalize_for_comparison(path: &PathBuf) -> PathBuf {
            let normalized = norm_case(path);
            let path_str = normalized.to_string_lossy();
            if path_str.starts_with(r"\\?\") {
                PathBuf::from(path_str.trim_start_matches(r"\\?\"))
            } else {
                normalized
            }
        }

        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }
        // Normalize paths to handle \\?\ prefix differences
        let list_of_possible_exes: Vec<PathBuf> = vec![env.executable.clone()]
            .into_iter()
            .chain(env.symlinks.clone().unwrap_or_default())
            .map(|p| normalize_for_comparison(&p))
            .collect();
        if let Some(environments) = self.find_with_cache() {
            for found_env in environments {
                if let Some(symlinks) = &found_env.symlinks {
                    // Normalize symlinks for comparison
                    let normalized_symlinks: Vec<PathBuf> =
                        symlinks.iter().map(normalize_for_comparison).collect();
                    // Check if we have found this exe.
                    if list_of_possible_exes
                        .iter()
                        .any(|exe| normalized_symlinks.contains(exe))
                    {
                        // Its possible the env discovery was not aware of the symlink
                        // E.g. if we are asked to resolve `../WindowsApp/python.exe`
                        // We will have no idea, hence this will get spawned, and then exe
                        // might be something like `../WindowsApp/PythonSoftwareFoundation.Python.3.10...`
                        // However the env found by the locator will almost never contain python.exe nor python3.exe
                        // See README.md
                        // As a result, we need to add those symlinks here.
                        let builder = PythonEnvironmentBuilder::from_environment(found_env.clone())
                            .symlinks(env.symlinks.clone());
                        return Some(builder.build());
                    }
                }
            }
        }
        None
    }

    #[cfg(unix)]
    fn try_from(&self, _env: &PythonEnv) -> Option<PythonEnvironment> {
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        self.clear();
        if let Some(environments) = self.find_with_cache() {
            environments
                .iter()
                .for_each(|e| reporter.report_environment(e))
        }
    }

    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_core::os_environment::EnvironmentApi;

    #[test]
    fn test_full_refresh_sync_replaces_store_cache() {
        let environment = EnvironmentApi::new();
        let shared = WindowsStore::from(&environment);
        let refreshed = WindowsStore::from(&environment);

        shared
            .environments
            .write()
            .unwrap()
            .replace(vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }]);
        refreshed
            .environments
            .write()
            .unwrap()
            .replace(vec![PythonEnvironment {
                name: Some("fresh".to_string()),
                ..Default::default()
            }]);

        shared.sync_refresh_state_from(&refreshed, &RefreshStateSyncScope::Full);

        let result = shared.environments.read().unwrap().clone().unwrap();
        assert_eq!(result[0].name.as_deref(), Some("fresh"));
    }

    #[test]
    fn test_workspace_scope_does_not_replace_store_cache() {
        let environment = EnvironmentApi::new();
        let shared = WindowsStore::from(&environment);
        let refreshed = WindowsStore::from(&environment);

        shared
            .environments
            .write()
            .unwrap()
            .replace(vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }]);
        refreshed
            .environments
            .write()
            .unwrap()
            .replace(vec![PythonEnvironment {
                name: Some("fresh".to_string()),
                ..Default::default()
            }]);

        shared.sync_refresh_state_from(&refreshed, &RefreshStateSyncScope::Workspace);

        let result = shared.environments.read().unwrap().clone().unwrap();
        assert_eq!(result[0].name.as_deref(), Some("stale"));
    }
}
