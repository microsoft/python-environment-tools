// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use pet_core::os_environment::Environment;
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
// NOTE: Do not implement Default trait, as we do not want to ever forget to set the values.
// Lets be explicit, this way we never miss a value (in Windows or Unix).
pub struct EnvVariables {
    pub home: Option<PathBuf>,
    pub workon_home: Option<String>,
}

impl EnvVariables {
    pub fn from(env: Arc<dyn Environment>) -> Self {
        EnvVariables {
            home: env.get_user_home(),
            workon_home: env.get_env_var("WORKON_HOME".to_string()),
        }
    }
}
