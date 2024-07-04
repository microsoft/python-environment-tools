// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{path::PathBuf, sync::Arc};

use pet_core::os_environment::Environment;

#[derive(Debug, Clone)]
// NOTE: Do not implement Default trait, as we do not want to ever forget to set the values.
// Lets be explicit, this way we never miss a value (in Windows or Unix).
pub struct EnvVariables {
    pub home: Option<PathBuf>,
    /// Only used in tests, None in production.
    pub root: Option<PathBuf>,
    pub path: Option<String>,
    pub userprofile: Option<String>,
    pub allusersprofile: Option<String>,
    pub programdata: Option<String>,
    pub homedrive: Option<String>,
    pub conda_root: Option<String>,
    pub conda: Option<String>,
    pub conda_prefix: Option<String>,
    pub condarc: Option<String>,
    pub xdg_config_home: Option<String>,
    pub known_global_search_locations: Vec<PathBuf>,
}

impl EnvVariables {
    pub fn from(env: Arc<dyn Environment>) -> Self {
        EnvVariables {
            home: env.get_user_home(),
            root: env.get_root(),
            path: env.get_env_var("PATH".to_string()),
            userprofile: env.get_env_var("USERPROFILE".to_string()),
            allusersprofile: env.get_env_var("ALLUSERSPROFILE".to_string()),
            programdata: env.get_env_var("PROGRAMDATA".to_string()),
            homedrive: env.get_env_var("HOMEDRIVE".to_string()),
            conda_root: env.get_env_var("CONDA_ROOT".to_string()),
            conda: env.get_env_var("CONDA".to_string()),
            conda_prefix: env.get_env_var("CONDA_PREFIX".to_string()),
            condarc: env.get_env_var("CONDARC".to_string()),
            xdg_config_home: env.get_env_var("XDG_CONFIG_HOME".to_string()),
            known_global_search_locations: env.get_know_global_search_locations(),
        }
    }
}
