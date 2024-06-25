// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{fs, path::PathBuf};

use log::error;
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::{env::PythonEnv, executable::find_executables};
use pet_virtualenv::is_virtualenv;

pub struct LinuxGlobalPython {}

impl LinuxGlobalPython {
    pub fn new() -> LinuxGlobalPython {
        LinuxGlobalPython {}
    }
}
impl Default for LinuxGlobalPython {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for LinuxGlobalPython {
    fn get_name(&self) -> &'static str {
        "LinuxGlobalPython"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentCategory> {
        vec![PythonEnvironmentCategory::LinuxGlobal]
    }

    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if std::env::consts::OS == "macos" || std::env::consts::OS == "windows" {
            return None;
        }
        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }

        if let (Some(prefix), Some(_)) = (env.prefix.clone(), env.version.clone()) {
            let executable = env.executable.clone();

            // If prefix or version is not available, then we cannot use this method.
            // 1. For files in /bin or /usr/bin, the prefix is always /usr
            // 2. For files in /usr/local/bin, the prefix is always /usr/local
            if !executable.starts_with("/bin")
                && !executable.starts_with("/usr/bin")
                && !executable.starts_with("/usr/local/bin")
                && !prefix.starts_with("/usr")
                && !prefix.starts_with("/usr/local")
            {
                return None;
            }

            // All known global linux are always installed in `/bin` or `/usr/bin`
            if executable.starts_with("/bin") || executable.starts_with("/usr/bin") {
                get_python_in_usr_bin(env)
            } else if executable.starts_with("/usr/local/bin") {
                get_python_in_usr_local_bin(env)
            } else {
                error!(
                    "Invalid state, ex ({:?}) is not in any of /bin, /usr/bin, /usr/local/bin",
                    executable
                );
                None
            }
        } else {
            None
        }
    }

    fn find(&self, _reporter: &dyn Reporter) {
        // No point looking in /usr/bin or /bin folder.
        // We will end up looking in these global locations and spawning them in other parts.
        // Here we cannot assume that anything in /usr/bin is a global Python, it could be a symlink or other.
        // Safer approach is to just spawn it which we need to do to get the `sys.prefix`
    }
}

fn get_python_in_usr_bin(env: &PythonEnv) -> Option<PythonEnvironment> {
    // If we do not have the prefix, then do not try
    // This method will be called with resolved Python where prefix & version is available.
    if env.version.clone().is_none() || env.prefix.clone().is_none() {
        return None;
    }
    let executable = env.executable.clone();
    let mut symlinks = env.symlinks.clone().unwrap_or_default();
    symlinks.push(executable.clone());

    let bin = PathBuf::from("/bin");
    let usr_bin = PathBuf::from("/usr/bin");

    // Possible this exe is a symlink to another file in the same directory.
    // E.g. /usr/bin/python3 is a symlink to /usr/bin/python3.12
    // let bin = executable.parent()?;
    // We use canonicalize to get the real path of the symlink.
    // Only used in this case, see notes for resolve_symlink.
    if let Some(symlink) = resolve_symlink(&executable).or(fs::canonicalize(&executable).ok()) {
        // Ensure this is a symlink in the bin or usr/bin directory.
        if symlink.starts_with(&bin) || symlink.starts_with(&usr_bin) {
            symlinks.push(symlink);
        }
    }

    // Look for other symlinks in /usr/bin and /bin folder
    // https://stackoverflow.com/questions/68728225/what-is-the-difference-between-usr-bin-python3-and-bin-python3
    // We know that on linux there are symlinks in both places.
    // & they all point to one exe and have the same version and same prefix.
    for possible_symlink in [find_executables(&bin), find_executables(&usr_bin)].concat() {
        if let Some(symlink) =
            resolve_symlink(&possible_symlink).or(fs::canonicalize(&possible_symlink).ok())
        {
            // the file /bin/python3 is a symlink to /usr/bin/python3.12
            // the file /bin/python3.12 is a symlink to /usr/bin/python3.12
            // the file /usr/bin/python3 is a symlink to /usr/bin/python3.12
            // Thus we have 3 symlinks pointing to the same exe /usr/bin/python3.12
            if symlinks.contains(&symlink) {
                symlinks.push(possible_symlink);
            }
        }
    }
    symlinks.sort();
    symlinks.dedup();

    Some(
        PythonEnvironmentBuilder::new(PythonEnvironmentCategory::LinuxGlobal)
            .executable(Some(executable))
            .version(env.version.clone())
            .prefix(env.prefix.clone())
            .symlinks(Some(symlinks))
            .build(),
    )
}

fn get_python_in_usr_local_bin(env: &PythonEnv) -> Option<PythonEnvironment> {
    // If we do not have the prefix, then do not try
    // This method will be called with resolved Python where prefix & version is available.
    if env.version.clone().is_none() || env.prefix.clone().is_none() {
        return None;
    }
    let executable = env.executable.clone();
    let mut symlinks = env.symlinks.clone().unwrap_or_default();
    symlinks.push(executable.clone());

    let usr_local_bin = PathBuf::from("/usr/local/bin");

    // Possible this exe is a symlink to another file in the same directory.
    // E.g. /usr/local/bin/python3 could be a symlink to /usr/local/bin/python3.12
    // let bin = executable.parent()?;
    // We use canonicalize to get the real path of the symlink.
    // Only used in this case, see notes for resolve_symlink.
    if let Some(symlink) = resolve_symlink(&executable).or(fs::canonicalize(&executable).ok()) {
        // Ensure this is a symlink in the bin or usr/local/bin directory.
        if symlink.starts_with(&usr_local_bin) {
            symlinks.push(symlink);
        }
    }

    // Look for other symlinks in this same folder
    for possible_symlink in find_executables(&usr_local_bin) {
        if let Some(symlink) =
            resolve_symlink(&possible_symlink).or(fs::canonicalize(&possible_symlink).ok())
        {
            // the file /bin/python3 is a symlink to /usr/bin/python3.12
            // the file /bin/python3.12 is a symlink to /usr/bin/python3.12
            // the file /usr/bin/python3 is a symlink to /usr/bin/python3.12
            // Thus we have 3 symlinks pointing to the same exe /usr/bin/python3.12
            if symlinks.contains(&symlink) {
                symlinks.push(possible_symlink);
            }
        }
    }
    symlinks.sort();
    symlinks.dedup();

    Some(
        PythonEnvironmentBuilder::new(PythonEnvironmentCategory::LinuxGlobal)
            .executable(Some(executable))
            .version(env.version.clone())
            .prefix(env.prefix.clone())
            .symlinks(Some(symlinks))
            .build(),
    )
}
