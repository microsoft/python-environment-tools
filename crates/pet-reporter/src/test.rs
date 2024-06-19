// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::environment::get_environment_key;
use pet_core::LocatorResult;
use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use std::collections::HashMap;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct TestReporter {
    managers: Arc<Mutex<HashMap<PathBuf, EnvManager>>>,
    environments: Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
}

impl TestReporter {
    pub fn get_result(&self) -> LocatorResult {
        LocatorResult {
            managers: self.managers.lock().unwrap().values().cloned().collect(),
            environments: self
                .environments
                .lock()
                .unwrap()
                .values()
                .cloned()
                .collect(),
        }
    }
}

impl Reporter for TestReporter {
    fn report_manager(&self, manager: &EnvManager) {
        let mut reported_managers = self.managers.lock().unwrap();
        reported_managers.insert(manager.executable.clone(), manager.clone());
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        if let Some(key) = get_environment_key(env) {
            let mut reported_environments = self.environments.lock().unwrap();
            // TODO: Sometimes its possible the exe here is actually some symlink that we have no idea about.
            // Hence we'll need to go through the list of reported envs and see if we can find a match.
            // If we do find a match, then ensure we update the symlinks
            // & if necessary update the other information.
            reported_environments.insert(key.clone(), env.clone());
        }
    }
}

pub fn create_reporter() -> TestReporter {
    TestReporter {
        managers: Arc::new(Mutex::new(HashMap::new())),
        environments: Arc::new(Mutex::new(HashMap::new())),
    }
}
