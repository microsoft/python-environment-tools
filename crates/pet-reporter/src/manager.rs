// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::manager::{EnvManager, EnvManagerType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn tool_to_string(tool: &EnvManagerType) -> &'static str {
    match tool {
        EnvManagerType::Conda => "conda",
        EnvManagerType::Pyenv => "pyenv",
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct Manager {
    pub executable: PathBuf,
    pub version: Option<String>,
    pub tool: &'static str,
}

impl Manager {
    pub fn from(env: &EnvManager) -> Manager {
        Manager {
            executable: env.executable.clone(),
            version: env.version.clone(),
            tool: tool_to_string(&env.tool),
        }
    }
}

impl std::fmt::Display for Manager {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Manager ({})", self.tool).unwrap_or_default();
        writeln!(
            f,
            "   Executable  : {}",
            self.executable.to_str().unwrap_or_default()
        )
        .unwrap_or_default();
        if let Some(version) = &self.version {
            writeln!(f, "   Version     : {}", version).unwrap_or_default();
        }
        Ok(())
    }
}
