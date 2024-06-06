// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::environment::get_environment_key;
use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use std::collections::HashMap;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct TestReporter {
    pub reported_managers: Arc<Mutex<HashMap<PathBuf, EnvManager>>>,
    pub reported_environments: Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
}

impl Reporter for TestReporter {
    fn report_manager(&self, manager: &EnvManager) {
        let mut reported_managers = self.reported_managers.lock().unwrap();
        if !reported_managers.contains_key(&manager.executable) {
            reported_managers.insert(manager.executable.clone(), manager.clone());
        }
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        if let Some(key) = get_environment_key(env) {
            let mut reported_environments = self.reported_environments.lock().unwrap();
            if !reported_environments.contains_key(key) {
                reported_environments.insert(key.clone(), env.clone());
            }
        }
    }
    fn report_completion(&self, _duration: std::time::Duration) {
        //
    }
}

pub fn create_reporter() -> TestReporter {
    TestReporter {
        reported_managers: Arc::new(Mutex::new(HashMap::new())),
        reported_environments: Arc::new(Mutex::new(HashMap::new())),
    }
}
