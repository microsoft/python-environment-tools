// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::headers::{self, Headers};
use log::{trace, warn};
use pet_core::pyvenv_cfg::PyVenvCfg;
use pet_fs::path::resolve_symlink;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn from_header_files(prefix: &Path) -> Option<String> {
    Headers::get_version(prefix)
}
pub fn from_pyvenv_cfg(prefix: &Path) -> Option<String> {
    PyVenvCfg::find(prefix).map(|cfg| cfg.version)
}
pub fn from_creator_for_virtual_env(prefix: &Path) -> Option<String> {
    if let Some(version) = Headers::get_version(prefix) {
        return Some(version);
    }
    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    let executable = &prefix.join(bin).join("python");

    // Determine who created this virtual environment, and get version of that environment.
    // Note, its unlikely conda envs were used to create virtual envs, thats a very bad idea (known to cause issues and not reccomended).
    // Hence do not support conda envs when getting versio of the parent env.
    if let Some(mut creator_executable) = get_python_exe_used_to_create_venv(executable) {
        // Possible we got resolved to the same bin directory but python3.10
        if creator_executable.starts_with(prefix) {
            creator_executable = resolve_symlink(&creator_executable)?;
        }
        let parent_dir = creator_executable.parent()?;
        if parent_dir.file_name().unwrap_or_default() != bin {
            trace!("Creator of virtual environment found, but the creator of {:?} is located in {:?} , instead of a {:?} directory", prefix, creator_executable, bin);
            None
        } else {
            // Assume the python environment used to create this virtual env is a regular install of Python.
            // Try to get the version of that environment.
            let sys_root = parent_dir.parent()?;
            let pyver = if let Some(pyvenvcfg) = PyVenvCfg::find(prefix) {
                Some((pyvenvcfg.version_major, pyvenvcfg.version_minor))
            } else {
                None
            };
            headers::get_version(sys_root, pyver)
        }
    } else if cfg!(windows) {
        // Only on windows is it difficult to get the creator of the virtual environment.
        get_version_from_pyvenv_if_pyvenv_cfg_and_exe_created_same_time(prefix)
    } else {
        None
    }
}

pub fn from_prefix(prefix: &Path) -> Option<String> {
    if let Some(version) = from_pyvenv_cfg(prefix) {
        Some(version)
    } else {
        from_header_files(prefix)
    }
}

/// When creating virtual envs using `python -m venv` or the like,
/// The executable in the new environment ends up being a symlink to the python executable used to create the env.
/// Using this information its possible to determine the version of the Python environment used to create the env.
fn get_python_exe_used_to_create_venv<T: AsRef<Path>>(executable: T) -> Option<PathBuf> {
    let parent_dir = executable.as_ref().parent()?;
    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    if parent_dir.file_name().unwrap_or_default() != bin {
        warn!("Attempted to determine creator of virtual environment, but the env executable ({:?}) is not in the expected location.", executable.as_ref());
        return None;
    }

    let symlink = resolve_symlink(&executable)?;
    if symlink.is_file() {
        Some(symlink)
    } else {
        None
    }
}

/// Use pyvenv.cfg to get the version of the virtual environment in windows.
/// If the creation/modified dates of the pyvenv.cfg and the Scripts/python.exe are in the same period (few minutes apart)
/// Then we can use the pyvenv.cfg to get the version of the virtual environment.
fn get_version_from_pyvenv_if_pyvenv_cfg_and_exe_created_same_time(
    prefix: &Path,
) -> Option<String> {
    let cfg = PyVenvCfg::find(prefix)?;
    let pyvenv_cfg = prefix.join("pyvenv.cfg");
    if !pyvenv_cfg.exists() {
        return None;
    }
    let cfg_metadata = pyvenv_cfg.metadata().ok()?;
    let exe_metadata = prefix.join("Scripts").join("python.exe").metadata().ok()?;
    let cfg_modified = cfg_metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    let exe_modified = exe_metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    // If they are just a few minutes apart,
    // then we can assume the version in the pyvenv.cfg is the version of the virtual environment.
    if cfg_modified.abs_diff(exe_modified) < 60 {
        trace!(
            "Using pyvenv.cfg to get version of virtual environment {:?}",
            prefix
        );
        Some(cfg.version)
    } else {
        None
    }
}
