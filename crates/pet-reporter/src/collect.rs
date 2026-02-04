// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use std::sync::{Arc, Mutex};

/// Used to just collect the environments and managers and will not report anytihng anywhere.
pub struct CollectReporter {
    pub managers: Arc<Mutex<Vec<EnvManager>>>,
    pub environments: Arc<Mutex<Vec<PythonEnvironment>>>,
}

impl Default for CollectReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CollectReporter {
    pub fn new() -> CollectReporter {
        CollectReporter {
            managers: Arc::new(Mutex::new(vec![])),
            environments: Arc::new(Mutex::new(vec![])),
        }
    }
}
impl Reporter for CollectReporter {
    fn report_telemetry(&self, _event: &pet_core::telemetry::TelemetryEvent) {
        //
    }
    fn report_manager(&self, manager: &EnvManager) {
        self.managers
            .lock()
            .expect("managers mutex poisoned")
            .push(manager.clone());
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        self.environments
            .lock()
            .expect("environments mutex poisoned")
            .push(env.clone());
    }
}

pub fn create_reporter() -> CollectReporter {
    CollectReporter::new()
}
