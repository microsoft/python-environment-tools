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
        // We no longer reset `search_result` here: the cache may have been
        // populated via `sync_refresh_state_from` between refreshes, and
        // `find()` is invoked on transient locators per refresh, so on the
        // first refresh the cache is empty by construction. Re-clearing
        // forced every refresh to re-walk both registry hives, each of
        // which is intercepted by Windows Defender (issue #454).
        //
        // On a cache hit `get_registry_pythons` is NOT called, so we must
        // replay the cached environments/managers to the reporter
        // ourselves — otherwise WindowsRegistry discoveries would silently
        // disappear on every refresh after the first.
        let cached = {
            let result = self
                .search_result
                .lock()
                .expect("search_result mutex poisoned");
            result.clone()
        };
        if let Some(cached) = cached {
            for manager in &cached.managers {
                reporter.report_manager(manager);
            }
            for env in &cached.environments {
                reporter.report_environment(env);
            }
            return;
        }
        // Cache miss: walk the registry. `get_registry_pythons` reports
        // inline as it discovers entries, so no separate replay is needed.
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

    /// `find()` must NOT clear the cache before populating it. The previous
    /// implementation called `self.clear()` first, which forced every
    /// `refresh` RPC to re-walk both registry hives — a Defender-intercepted
    /// hot path tracked in #454. This test pins down the new contract:
    /// pre-populate the cache, run `find()`, and assert (a) the original
    /// entries survived (i.e. the cache was not cleared) and (b) the
    /// reporter was notified with the cached environments and managers,
    /// so cached results are still observable to refresh consumers.
    #[cfg(windows)]
    #[test]
    fn test_find_reuses_cached_results_within_locator_lifetime() {
        use pet_core::manager::EnvManager;
        use pet_core::python_environment::PythonEnvironment;
        use pet_core::reporter::Reporter;
        use pet_core::telemetry::TelemetryEvent;
        use std::sync::Mutex;

        #[derive(Default)]
        struct RecordingReporter {
            environments: Mutex<Vec<String>>,
            managers: Mutex<Vec<PathBuf>>,
        }
        impl Reporter for RecordingReporter {
            fn report_manager(&self, manager: &EnvManager) {
                self.managers
                    .lock()
                    .unwrap()
                    .push(manager.executable.clone());
            }
            fn report_environment(&self, env: &PythonEnvironment) {
                self.environments
                    .lock()
                    .unwrap()
                    .push(env.name.clone().unwrap_or_default());
            }
            fn report_telemetry(&self, _event: &TelemetryEvent) {}
        }

        let locator = create_locator();
        let cached_manager = EnvManager::new(
            PathBuf::from("C:\\fake\\python.exe"),
            pet_core::manager::EnvManagerType::Conda,
            None,
        );
        let cached = LocatorResult {
            managers: vec![cached_manager.clone()],
            environments: vec![PythonEnvironment {
                name: Some("cached".to_string()),
                ..Default::default()
            }],
        };
        locator
            .search_result
            .lock()
            .unwrap()
            .replace(cached.clone());

        let reporter = RecordingReporter::default();
        locator.find(&reporter);

        // (a) The cache must still be populated and unchanged.
        let after = locator
            .search_result
            .lock()
            .unwrap()
            .clone()
            .expect("cache must remain populated after find()");
        assert_eq!(
            after.environments.len(),
            1,
            "find() must not clear the cache before populating",
        );
        assert_eq!(after.environments[0].name.as_deref(), Some("cached"));
        // (b) The cached entries must have been replayed to the reporter
        // — otherwise WindowsRegistry discoveries would silently
        // disappear on every refresh after the first.
        assert_eq!(
            reporter.environments.lock().unwrap().as_slice(),
            &["cached".to_string()],
            "find() must replay cached environments to the reporter on a cache hit",
        );
        assert_eq!(
            reporter.managers.lock().unwrap().as_slice(),
            &[PathBuf::from("C:\\fake\\python.exe")],
            "find() must replay cached managers to the reporter on a cache hit",
        );
    }

    /// Smoke test: on a fresh locator (empty cache), `find()` runs the new
    /// parallel walk through HKLM and HKCU and never panics or deadlocks.
    /// The discovered environment list may legitimately be empty on a CI
    /// runner without any Python registry installs — we only assert the
    /// cache was populated (i.e. the walk completed and `Some(_)` was
    /// stored), not its contents.
    #[cfg(windows)]
    #[test]
    fn test_find_on_fresh_locator_completes_parallel_walk() {
        use pet_core::manager::EnvManager;
        use pet_core::python_environment::PythonEnvironment;
        use pet_core::reporter::Reporter;
        use pet_core::telemetry::TelemetryEvent;

        struct NoopReporter;
        impl Reporter for NoopReporter {
            fn report_manager(&self, _manager: &EnvManager) {}
            fn report_environment(&self, _env: &PythonEnvironment) {}
            fn report_telemetry(&self, _event: &TelemetryEvent) {}
        }

        let locator = create_locator();
        assert!(
            locator.search_result.lock().unwrap().is_none(),
            "freshly built locator must start with an empty cache",
        );

        locator.find(&NoopReporter);

        assert!(
            locator.search_result.lock().unwrap().is_some(),
            "find() must populate the cache after walking both hives",
        );
    }
}
