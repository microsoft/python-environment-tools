// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use pet_core::os_environment::Environment;

#[derive(Debug, Clone)]
// NOTE: Do not implement Default trait, as we do not want to ever forget to set the values.
// Lets be explicit, this way we never miss a value (in Windows or Unix).
pub struct EnvVariables {
    pub home: Option<PathBuf>,
    pub known_global_search_locations: Vec<PathBuf>,
}

impl EnvVariables {
    pub fn from(env: &dyn Environment) -> Self {
        EnvVariables {
            home: env.get_user_home(),
            known_global_search_locations: env.get_know_global_search_locations(),
        }
    }
}
