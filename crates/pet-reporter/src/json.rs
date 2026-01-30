// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter,
    telemetry::TelemetryEvent,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonOutput {
    pub managers: Vec<EnvManager>,
    pub environments: Vec<PythonEnvironment>,
}

/// Reporter that collects environments and managers for JSON output
pub struct JsonReporter {
    managers: Arc<Mutex<Vec<EnvManager>>>,
    environments: Arc<Mutex<Vec<PythonEnvironment>>>,
}

impl Default for JsonReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonReporter {
    pub fn new() -> Self {
        JsonReporter {
            managers: Arc::new(Mutex::new(vec![])),
            environments: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn output_json(&self) {
        let managers = self.managers.lock().unwrap().clone();
        let environments = self.environments.lock().unwrap().clone();

        let output = JsonOutput {
            managers,
            environments,
        };

        match serde_json::to_string_pretty(&output) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing to JSON: {}", e),
        }
    }
}

impl Reporter for JsonReporter {
    fn report_telemetry(&self, _event: &TelemetryEvent) {
        // No telemetry in JSON output
    }

    fn report_manager(&self, manager: &EnvManager) {
        self.managers.lock().unwrap().push(manager.clone());
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        self.environments.lock().unwrap().push(env.clone());
    }
}

pub fn create_reporter() -> JsonReporter {
    JsonReporter::new()
}
