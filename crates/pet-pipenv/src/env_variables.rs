// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use std::sync::Arc;

use pet_core::os_environment::Environment;

#[derive(Debug, Clone)]
// NOTE: Do not implement Default trait, as we do not want to ever forget to set the values.
// Lets be explicit, this way we never miss a value (in Windows or Unix).
pub struct EnvVariables {
    #[allow(dead_code)]
    pub pipenv_max_depth: u16,
    pub pipenv_pipfile: String,
}

impl EnvVariables {
    pub fn from(env: Arc<dyn Environment>) -> Self {
        EnvVariables {
            pipenv_max_depth: env
                .get_env_var("PIPENV_MAX_DEPTH".to_string())
                .map(|s| s.parse::<u16>().ok().unwrap_or(3))
                .unwrap_or(3),
            pipenv_pipfile: env
                .get_env_var("PIPENV_PIPFILE".to_string())
                .unwrap_or("Pipfile".to_string()),
        }
    }
}
