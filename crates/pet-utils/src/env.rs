// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

#[derive(Debug)]
pub struct PythonEnv {
    pub executable: PathBuf,
    pub prefix: Option<PathBuf>,
    pub version: Option<String>,
}

impl PythonEnv {
    pub fn new(executable: PathBuf, prefix: Option<PathBuf>, version: Option<String>) -> Self {
        Self {
            executable,
            prefix,
            version,
        }
    }
}
