// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

// conda-meta must exist as this contains a mandatory `history` file.
pub fn is_conda_install(path: &Path) -> bool {
    path.join("envs").metadata().is_ok() && path.join("conda-meta").metadata().is_ok()
}

// conda-meta must exist as this contains a mandatory `history` file.
// The root conda installation folder is also a conda environment (its the base environment).
pub fn is_conda_env(path: &Path) -> bool {
    if let Some(metadata) = fs::metadata(path.join("conda-meta")).ok() {
        metadata.is_dir()
    } else {
        false
    }
}

#[derive(Debug, Clone)]
// NOTE: Do not implt Default trait, as we do not want to ever forget to set the values.
// Lets be explicit, this way we never miss a value (in Windows or Unix).
pub struct CondaEnvironmentVariables {
    pub home: Option<PathBuf>,
    pub root: Option<PathBuf>,
    pub path: Option<String>,
    pub userprofile: Option<String>,
    pub allusersprofile: Option<String>,
    pub programdata: Option<String>,
    pub homedrive: Option<String>,
    pub conda_root: Option<String>,
    pub conda_prefix: Option<String>,
    pub condarc: Option<String>,
    pub xdg_config_home: Option<String>,
    pub known_global_search_locations: Vec<PathBuf>,
}
