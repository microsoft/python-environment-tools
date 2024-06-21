// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::manager::{EnvManager, EnvManagerType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn tool_to_string(tool: &EnvManagerType) -> &'static str {
    match tool {
        EnvManagerType::Conda => "conda",
        EnvManagerType::Pyenv => "pyenv",
        EnvManagerType::Poetry => "poery",
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct Manager {
    pub executable: PathBuf,
    pub version: Option<String>,
    pub tool: String,
}

impl Manager {
    pub fn from(env: &EnvManager) -> Manager {
        Manager {
            executable: env.executable.clone(),
            version: env.version.clone(),
            tool: tool_to_string(&env.tool).to_string(),
        }
    }
}
