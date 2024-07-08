// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::environment::get_environment_key;
use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct CacheReporter {
    reporter: Arc<dyn Reporter>,
    reported_managers: Arc<Mutex<HashMap<PathBuf, EnvManager>>>,
    reported_environments: Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
}

impl CacheReporter {
    pub fn new(reporter: Arc<dyn Reporter>) -> Self {
        Self {
            reporter,
            reported_managers: Arc::new(Mutex::new(HashMap::new())),
            reported_environments: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
impl Reporter for CacheReporter {
    fn report_telemetry(&self, event: &pet_core::telemetry::TelemetryEvent) {
        self.reporter.report_telemetry(event);
    }
    fn report_manager(&self, manager: &EnvManager) {
        let mut reported_managers = self.reported_managers.lock().unwrap();
        if !reported_managers.contains_key(&manager.executable) {
            reported_managers.insert(manager.executable.clone(), manager.clone());
            self.reporter.report_manager(manager);
        }
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        if let Some(key) = get_environment_key(env) {
            let mut reported_environments = self.reported_environments.lock().unwrap();
            if !reported_environments.contains_key(&key) {
                reported_environments.insert(key.clone(), env.clone());
                self.reporter.report_environment(env);
            }
        }
    }
}
