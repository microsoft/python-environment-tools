// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use environments::get_registry_pythons;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{
    env::PythonEnv,
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    Locator, LocatorKind, LocatorResult, RefreshStatePersistence, RefreshStateSyncScope,
};
use pet_virtualenv::is_virtualenv;
use std::sync::{Arc, Mutex};

mod environments;

pub struct WindowsRegistry {
    #[allow(dead_code)]
    conda_locator: Arc<dyn CondaLocator>,
    #[allow(dead_code)]
    search_result: Arc<Mutex<Option<LocatorResult>>>,
}

impl WindowsRegistry {
    pub fn from(conda_locator: Arc<dyn CondaLocator>) -> WindowsRegistry {
        WindowsRegistry {
            conda_locator,
            search_result: Arc::new(Mutex::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self, reporter: Option<&dyn Reporter>) -> Option<LocatorResult> {
        let mut result = self
            .search_result
            .lock()
            .expect("search_result mutex poisoned");
        if let Some(result) = result.clone() {
            return Some(result);
        }

        let registry_result = get_registry_pythons(&self.conda_locator, &reporter);
        result.replace(registry_result.clone());

        Some(registry_result)
    }
    #[cfg(windows)]
    fn clear(&self) {
        let mut search_result = self
            .search_result
            .lock()
            .expect("search_result mutex poisoned");
        search_result.take();
    }

    fn sync_search_result_from(&self, source: &WindowsRegistry) {
        let search_result = source
            .search_result
            .lock()
            .expect("search_result mutex poisoned")
            .clone();
        self.search_result
            .lock()
            .expect("search_result mutex poisoned")
            .clone_from(&search_result);
    }
}

impl Locator for WindowsRegistry {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::WindowsRegistry
    }
    fn refresh_state(&self) -> RefreshStatePersistence {
        RefreshStatePersistence::SyncedDiscoveryState
    }
    fn sync_refresh_state_from(&self, source: &dyn Locator, scope: &RefreshStateSyncScope) {
        let source = source
            .as_any()
            .downcast_ref::<WindowsRegistry>()
            .unwrap_or_else(|| {
                panic!(
                    "attempted to sync WindowsRegistry state from {:?}",
                    source.get_kind()
                )
            });

        match scope {
            RefreshStateSyncScope::Full => self.sync_search_result_from(source),
            RefreshStateSyncScope::GlobalFiltered(kind)
                if self.supported_categories().contains(kind) =>
            {
                self.sync_search_result_from(source)
            }
            RefreshStateSyncScope::GlobalFiltered(_) | RefreshStateSyncScope::Workspace => {}
        }
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![
            PythonEnvironmentKind::WindowsRegistry,
            PythonEnvironmentKind::Conda,
        ]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }
        // We need to check this here, as its possible to install
        // a Python environment via an Installer that ends up in Windows Registry
        // However that environment is a conda environment.
        if let Some(env_path) = &env.prefix {
            if is_conda_env(env_path) {
                return None;
            }
        }
        #[cfg(windows)]
        if let Some(result) = self.find_with_cache(None) {
            // Find the same env here
            for found_env in result.environments {
                if let Some(ref python_executable_path) = found_env.executable {
                    if python_executable_path == &env.executable {
                        return Some(found_env);
                    }
                }
            }
        }
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        self.clear();
        let _ = self.find_with_cache(Some(reporter));
    }
    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_conda::Conda;
    use pet_core::os_environment::EnvironmentApi;
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    fn create_virtualenv(prefix: &Path) -> PathBuf {
        let scripts_dir = prefix.join(if cfg!(windows) { "Scripts" } else { "bin" });
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(
            scripts_dir.join(if cfg!(windows) {
                "activate.bat"
            } else {
                "activate"
            }),
            b"",
        )
        .unwrap();
        let executable = scripts_dir.join(if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        });
        fs::write(&executable, b"").unwrap();
        executable
    }

    fn create_locator() -> WindowsRegistry {
        let environment = EnvironmentApi::new();
        WindowsRegistry::from(Arc::new(Conda::from(&environment)))
    }

    #[test]
    fn test_windows_registry_reports_kind_categories_and_refresh_state() {
        let locator = create_locator();

        assert_eq!(locator.get_kind(), LocatorKind::WindowsRegistry);
        assert_eq!(
            locator.supported_categories(),
            vec![
                PythonEnvironmentKind::WindowsRegistry,
                PythonEnvironmentKind::Conda
            ]
        );
        assert_eq!(
            locator.refresh_state(),
            RefreshStatePersistence::SyncedDiscoveryState
        );
    }

    #[test]
    fn test_full_refresh_sync_replaces_registry_cache() {
        let shared = create_locator();
        let refreshed = create_locator();

        shared.search_result.lock().unwrap().replace(LocatorResult {
            managers: vec![],
            environments: vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }],
        });
        refreshed
            .search_result
            .lock()
            .unwrap()
            .replace(LocatorResult {
                managers: vec![],
                environments: vec![PythonEnvironment {
                    name: Some("fresh".to_string()),
                    ..Default::default()
                }],
            });

        shared.sync_refresh_state_from(&refreshed, &RefreshStateSyncScope::Full);

        let result = shared.search_result.lock().unwrap().clone().unwrap();
        assert_eq!(result.environments[0].name.as_deref(), Some("fresh"));
    }

    #[test]
    fn test_workspace_scope_does_not_replace_registry_cache() {
        let shared = create_locator();
        let refreshed = create_locator();

        shared.search_result.lock().unwrap().replace(LocatorResult {
            managers: vec![],
            environments: vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }],
        });
        refreshed
            .search_result
            .lock()
            .unwrap()
            .replace(LocatorResult {
                managers: vec![],
                environments: vec![PythonEnvironment {
                    name: Some("fresh".to_string()),
                    ..Default::default()
                }],
            });

        shared.sync_refresh_state_from(&refreshed, &RefreshStateSyncScope::Workspace);

        let result = shared.search_result.lock().unwrap().clone().unwrap();
        assert_eq!(result.environments[0].name.as_deref(), Some("stale"));
    }

    #[test]
    fn test_global_filtered_scope_syncs_supported_kinds_only() {
        let shared = create_locator();
        let refreshed = create_locator();

        shared.search_result.lock().unwrap().replace(LocatorResult {
            managers: vec![],
            environments: vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }],
        });
        refreshed
            .search_result
            .lock()
            .unwrap()
            .replace(LocatorResult {
                managers: vec![],
                environments: vec![PythonEnvironment {
                    name: Some("fresh".to_string()),
                    ..Default::default()
                }],
            });

        shared.sync_refresh_state_from(
            &refreshed,
            &RefreshStateSyncScope::GlobalFiltered(PythonEnvironmentKind::WindowsRegistry),
        );
        let result = shared.search_result.lock().unwrap().clone().unwrap();
        assert_eq!(result.environments[0].name.as_deref(), Some("fresh"));

        shared.search_result.lock().unwrap().replace(LocatorResult {
            managers: vec![],
            environments: vec![PythonEnvironment {
                name: Some("stale".to_string()),
                ..Default::default()
            }],
        });

        shared.sync_refresh_state_from(
            &refreshed,
            &RefreshStateSyncScope::GlobalFiltered(PythonEnvironmentKind::Venv),
        );
        let result = shared.search_result.lock().unwrap().clone().unwrap();
        assert_eq!(result.environments[0].name.as_deref(), Some("stale"));
    }

    #[test]
    fn test_try_from_rejects_virtualenv_before_registry_lookup() {
        let temp_dir = TempDir::new().unwrap();
        let prefix = temp_dir.path().to_path_buf();
        let executable = create_virtualenv(&prefix);
        let env = PythonEnv::new(executable, Some(prefix), None);
        let locator = create_locator();

        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn test_try_from_rejects_conda_prefix_before_registry_lookup() {
        let temp_dir = TempDir::new().unwrap();
        let prefix = temp_dir.path().to_path_buf();
        fs::create_dir_all(prefix.join("conda-meta")).unwrap();
        let executable = prefix.join("python.exe");
        fs::write(&executable, b"").unwrap();
        let env = PythonEnv::new(executable, Some(prefix), None);
        let locator = create_locator();

        assert!(locator.try_from(&env).is_none());
    }
}
