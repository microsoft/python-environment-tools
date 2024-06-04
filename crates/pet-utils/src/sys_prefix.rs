// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use super::headers::Headers;
use super::pyvenv_cfg::PyVenvCfg;
use std::path::Path;

pub struct SysPrefix {}

impl SysPrefix {
    pub fn get_version(path: &Path) -> Option<String> {
        if let Some(cfg) = PyVenvCfg::find(path) {
            return Some(cfg.version);
        }
        Headers::get_version(path)
    }
}
