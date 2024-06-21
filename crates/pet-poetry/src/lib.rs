// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::list_environments;
use log::{error, warn};
use manager::PoetryManager;
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
    reporter::Reporter,
    Configuration, Locator, LocatorResult,
};
use pet_python_utils::env::PythonEnv;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

mod config;
mod env_variables;
mod environment;
mod environment_locations;
mod environment_locations_spawn;
mod manager;
mod pyproject_toml;

pub struct Poetry {
    pub project_dirs: Arc<Mutex<Vec<PathBuf>>>,
    pub env_vars: EnvVariables,
    pub poetry_executable: Arc<Mutex<Option<PathBuf>>>,
    searched: AtomicBool,
    environments: Arc<Mutex<Vec<PythonEnvironment>>>,
    manager: Arc<Mutex<Option<PoetryManager>>>,
}

impl Poetry {
    pub fn new(environment: &dyn Environment) -> Self {
        Poetry {
            searched: AtomicBool::new(false),
            project_dirs: Arc::new(Mutex::new(vec![])),
            env_vars: EnvVariables::from(environment),
            poetry_executable: Arc::new(Mutex::new(None)),
            environments: Arc::new(Mutex::new(vec![])),
            manager: Arc::new(Mutex::new(None)),
        }
    }
    pub fn from(environment: &dyn Environment) -> impl Locator {
        Poetry::new(environment)
    }
    pub fn find_with_executable(&self) -> Option<()> {
        let manager = manager::PoetryManager::find(
            self.poetry_executable.lock().unwrap().clone(),
            &self.env_vars,
        )?;

        let environments_using_spawn = environment_locations_spawn::list_environments(
            &manager.executable,
            self.project_dirs.lock().unwrap().clone(),
            &manager,
        )
        .iter()
        .filter_map(|env| env.prefix.clone())
        .collect::<Vec<_>>();

        // Get environments using the faster way.
        if let Some(environments) = &self.find_with_cache() {
            let environments = environments
                .environments
                .iter()
                .filter_map(|env| env.prefix.clone())
                .collect::<Vec<_>>();

            for env in environments_using_spawn {
                if !environments.contains(&env) {
                    warn!(
                        "Found a Poetry env {:?} using the poetry exe {:?}",
                        env, manager.executable
                    );
                    // TODO: Send telemetry.
                }
            }
        } else {
            // TODO: Send telemetry.
            for env in environments_using_spawn {
                warn!(
                    "Found a Poetry env {:?} using the poetry exe {:?}",
                    env, manager.executable
                );
            }
        }
        Some(())
    }
    fn find_with_cache(&self) -> Option<LocatorResult> {
        if let Ok(environments) = self.environments.lock() {
            if !environments.is_empty() {
                if let Ok(manager) = self.manager.lock() {
                    if let Some(manager) = manager.as_ref() {
                        return Some(LocatorResult {
                            managers: vec![manager.to_manager()],
                            environments: environments.clone(),
                        });
                    }
                }
            }
            if self.searched.load(Ordering::Relaxed) {
                return None;
            }
        }
        // First find the manager
        let manager = manager::PoetryManager::find(
            self.poetry_executable.lock().unwrap().clone(),
            &self.env_vars,
        );
        let mut managers = vec![];
        if let Some(manager) = manager {
            let mut mgr = self.manager.lock().unwrap();
            mgr.replace(manager.clone());
            drop(mgr);
            managers.push(manager.to_manager());
        }
        let project_dirs = self.project_dirs.lock().unwrap().clone();

        if let Some(result) = list_environments(&self.env_vars, &project_dirs) {
            match self.environments.lock() {
                Ok(mut environments) => {
                    environments.clear();
                    environments.extend(result);
                    self.searched.store(true, Ordering::Relaxed);
                    Some(LocatorResult {
                        managers: managers.clone(),
                        environments: environments.clone(),
                    })
                }
                Err(err) => {
                    error!("Failed to cache to Poetry environments: {:?}", err);
                    None
                }
            }
        } else {
            self.searched.store(true, Ordering::Relaxed);
            None
        }
    }
}

impl Locator for Poetry {
    fn configure(&self, config: &Configuration) {
        if let Some(search_paths) = &config.search_paths {
            if !search_paths.is_empty() {
                self.project_dirs.lock().unwrap().clear();
                self.project_dirs
                    .lock()
                    .unwrap()
                    .extend(search_paths.clone());
            }
        }
        if let Some(exe) = &config.poetry_executable {
            self.poetry_executable.lock().unwrap().replace(exe.clone());
        }
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentCategory> {
        vec![PythonEnvironmentCategory::Poetry]
    }

    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if let Some(result) = self.find_with_cache() {
            for found_env in result.environments {
                if let Some(symlinks) = &found_env.symlinks {
                    if symlinks.contains(&env.executable) {
                        return Some(found_env.clone());
                    }
                }
            }
        }
        None
    }

    fn find(&self, reporter: &dyn Reporter) {
        if let Some(result) = self.find_with_cache() {
            for found_env in result.environments {
                if let Some(manager) = &found_env.manager {
                    reporter.report_manager(manager);
                }
                reporter.report_environment(&found_env);
            }
        }
    }
}
