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
use pet_core::{os_environment::Environment, Locator};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use pet_core::python_environment::PythonEnvironmentBuilder;
#[cfg(windows)]
use pet_virtualenv::is_virtualenv;
#[cfg(windows)]
use std::path::PathBuf;

pub fn is_windows_app_folder_in_program_files(path: &Path) -> bool {
    path.to_str().unwrap_or_default().to_string().to_lowercase()[1..]
        .starts_with(":\\program files\\windowsapps")
}

pub struct WindowsStore {
    pub env_vars: EnvVariables,
    #[allow(dead_code)]
    environments: Arc<Mutex<Option<Vec<PythonEnvironment>>>>,
}

impl WindowsStore {
    pub fn from(environment: &dyn Environment) -> WindowsStore {
        WindowsStore {
            env_vars: EnvVariables::from(environment),
            environments: Arc::new(Mutex::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<Vec<PythonEnvironment>> {
        let mut environments = self.environments.lock().unwrap();
        if let Some(environments) = environments.clone() {
            return Some(environments);
        }

        let envs = list_store_pythons(&self.env_vars).unwrap_or_default();
        environments.replace(envs.clone());
        Some(envs)
    }
    #[cfg(windows)]
    fn clear(&self) {
        self.environments.lock().unwrap().take();
    }
}

impl Locator for WindowsStore {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::WindowsStore
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::WindowsStore]
    }

    #[cfg(windows)]
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }
        let list_of_possible_exes = vec![env.executable.clone()]
            .into_iter()
            .chain(env.symlinks.clone().unwrap_or_default())
            .collect::<Vec<PathBuf>>();
        if let Some(environments) = self.find_with_cache() {
            for found_env in environments {
                if let Some(symlinks) = &found_env.symlinks {
                    // Check if we have found this exe.
                    if list_of_possible_exes
                        .iter()
                        .any(|exe| symlinks.contains(exe))
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
