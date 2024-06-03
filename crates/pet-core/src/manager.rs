// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Copy, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub enum EnvManagerType {
    Conda,
    Pyenv,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct EnvManager {
    pub executable: PathBuf,
    pub version: Option<String>,
    pub tool: EnvManagerType,
}

impl EnvManager {
    pub fn new(executable_path: PathBuf, tool: EnvManagerType, version: Option<String>) -> Self {
        Self {
            executable: executable_path,
            version,
            tool,
        }
    }
}
