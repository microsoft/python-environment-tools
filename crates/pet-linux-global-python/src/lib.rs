// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::fs;

use log::warn;
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

        // If we do not have a version, then we cannot use this method.
        // Without version means we have not spawned the Python exe, thus do not have the real info.
        env.version.clone()?;
        let prefix = env.prefix.clone()?;
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

        // All known global linux are always installed in `/bin` or `/usr/bin` or `/usr/local/bin`
        if executable.starts_with("/bin")
            || executable.starts_with("/usr/bin")
            || executable.starts_with("/usr/local/bin")
        {
            get_python_in_bin(env)
        } else {
            warn!(
                    "Unknown Python exe ({:?}), not in any of the known locations /bin, /usr/bin, /usr/local/bin",
                    executable
                );
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

fn get_python_in_bin(env: &PythonEnv) -> Option<PythonEnvironment> {
    // If we do not have the prefix, then do not try
    // This method will be called with resolved Python where prefix & version is available.
    if env.version.clone().is_none() || env.prefix.clone().is_none() {
        return None;
    }
    let executable = env.executable.clone();
    let mut symlinks = env.symlinks.clone().unwrap_or_default();
    symlinks.push(executable.clone());

    let bin = executable.parent()?;

    // Keep track of what the exe resolves to.
    // Will have a value only if the exe is in another dir
    // E.g. /bin/python3 might be a symlink to /usr/bin/python3.12
    // However due to legacy reasons we'll be treating these two as separate exes.
    // Hence they will be separate Python environments.
    let mut resolved_exe_is_from_another_dir = None;

    // Possible this exe is a symlink to another file in the same directory.
    // E.g. Generally /usr/bin/python3 is a symlink to /usr/bin/python3.12
    // E.g. Generally /usr/local/bin/python3 is a symlink to /usr/local/bin/python3.12
    // E.g. Generally /bin/python3 is a symlink to /bin/python3.12
    // let bin = executable.parent()?;
    // We use canonicalize to get the real path of the symlink.
    // Only used in this case, see notes for resolve_symlink.
    if let Some(symlink) = resolve_symlink(&executable).or(fs::canonicalize(&executable).ok()) {
        // Ensure this is a symlink in the bin or usr/bin directory.
        if symlink.starts_with(bin) {
            symlinks.push(symlink);
        } else {
            resolved_exe_is_from_another_dir = Some(symlink);
        }
    }

    // Look for other symlinks in the same folder
    // We know that on linux there are sym links in the same folder as the exe.
    // & they all point to one exe and have the same version and same prefix.
    for possible_symlink in find_executables(bin).iter() {
        if let Some(ref symlink) =
            resolve_symlink(&possible_symlink).or(fs::canonicalize(&possible_symlink).ok())
        {
            // Generally the file /bin/python3 is a symlink to /usr/bin/python3.12
            // Generally the file /bin/python3.12 is a symlink to /usr/bin/python3.12
            // Generally the file /usr/bin/python3 is a symlink to /usr/bin/python3.12
            // HOWEVER, we will be treating the files in /bin and /usr/bin as different.
            // Hence check whether the resolve symlink is in the same directory.
            if symlink.starts_with(bin) & symlinks.contains(&symlink) {
                symlinks.push(possible_symlink.to_owned());
            }

            // Possible the env.exevutable = /bin/python3
            // And the possible_symlink = /bin/python3.12
            // & possible that both of the above are symlinks and point to /usr/bin/python3.12
            // In this case /bin/python3 === /bin/python.3.12
            // However as mentioned earlier we will not be treating these the same as /usr/bin/python3.12
            if resolved_exe_is_from_another_dir == Some(symlink.to_owned()) {
                symlinks.push(possible_symlink.to_owned());
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
