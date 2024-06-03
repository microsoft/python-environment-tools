// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use std::{fs, path::Path};

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
