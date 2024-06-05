// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::path::normalize;
use std::path::PathBuf;

#[derive(Debug)]
pub struct PythonEnv {
    pub executable: PathBuf,
    pub prefix: Option<PathBuf>,
    pub version: Option<String>,
}

impl PythonEnv {
    pub fn new(executable: PathBuf, prefix: Option<PathBuf>, version: Option<String>) -> Self {
        let mut prefix = prefix.clone();
        if let Some(value) = prefix {
            prefix = normalize(value).into();
        }
        Self {
            executable: normalize(executable),
            prefix,
            version,
        }
    }
}
