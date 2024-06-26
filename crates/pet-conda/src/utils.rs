// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// conda-meta must exist as this contains a mandatory `history` file.
pub fn is_conda_install(path: &Path) -> bool {
    (path.join("condabin").metadata().is_ok() || path.join("envs").metadata().is_ok())
        && path.join("conda-meta").metadata().is_ok()
}

/// conda-meta must exist as this contains a mandatory `history` file.
/// The root conda installation folder is also a conda environment (its the base environment).
pub fn is_conda_env(path: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(path.join("conda-meta")) {
        metadata.is_dir()
    } else {
        false
    }
}

/// Only used in tests, noop in production.
///
/// Change the root of the path to a new root.
/// Lets assume some config file is located in the root directory /etc/config/config.toml.
/// We cannot test this unless we create such a file on the root of the filesystem.
/// Thats very risky and not recommended (ideally we want to create stuff in separate test folders).
/// The solution is to change the root of the path to a test folder.
pub fn change_root_of_path(path: &Path, new_root: &Option<PathBuf>) -> PathBuf {
    if cfg!(windows) {
        return path.to_path_buf();
    }
    if let Some(new_root) = new_root {
        // This only applies in tests.
        // We need this, as the root folder cannot be mocked.
        // Strip the first `/` (this path is only for testing purposes)
        new_root.join(&path.to_string_lossy()[1..])
    } else {
        path.to_path_buf()
    }
}
