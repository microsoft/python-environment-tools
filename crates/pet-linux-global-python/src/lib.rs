// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use pet_core::{
    arch::Architecture,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::{
    env::{PythonEnv, ResolvedPythonEnv},
    executable::find_executables,
};
use pet_virtualenv::is_virtualenv;

pub struct LinuxGlobalPython {
    reported_executables: Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
}

impl LinuxGlobalPython {
    pub fn new() -> LinuxGlobalPython {
        LinuxGlobalPython {
            reported_executables: Arc::new(
                Mutex::new(HashMap::<PathBuf, PythonEnvironment>::new()),
            ),
        }
    }

    fn find_cached(&self, reporter: Option<&dyn Reporter>) {
        if std::env::consts::OS == "macos" || std::env::consts::OS == "windows" {
            return;
        }
        // Look through the /bin, /usr/bin, /usr/local/bin directories
        thread::scope(|s| {
            for bin in ["/bin", "/usr/bin", "/usr/local/bin"] {
                s.spawn(move || {
                    find_and_report_global_pythons_in(bin, reporter, &self.reported_executables);
                });
            }
        });
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
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::LinuxGlobal]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
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
        let executable = env.executable.clone();

        self.find_cached(None);

        // We only support python environments in /bin, /usr/bin, /usr/local/bin
        if !executable.starts_with("/bin")
            && !executable.starts_with("/usr/bin")
            && !executable.starts_with("/usr/local/bin")
        {
            return None;
        }

        self.reported_executables
            .lock()
            .unwrap()
            .get(&executable)
            .cloned()
    }

    fn find(&self, reporter: &dyn Reporter) {
        if std::env::consts::OS == "macos" || std::env::consts::OS == "windows" {
            return;
        }
        self.reported_executables.lock().unwrap().clear();
        self.find_cached(Some(reporter))
    }
}

fn find_and_report_global_pythons_in(
    bin: &str,
    reporter: Option<&dyn Reporter>,
    reported_executables: &Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
) {
    let python_executables = find_executables(Path::new(bin));

    for exe in python_executables.clone().iter() {
        if reported_executables.lock().unwrap().contains_key(exe) {
            continue;
        }
        if let Some(resolved) = ResolvedPythonEnv::from(exe) {
            if let Some(env) = get_python_in_bin(&resolved.to_python_env(), resolved.is64_bit) {
                let mut reported_executables = reported_executables.lock().unwrap();
                // env.symlinks = Some([symlinks, env.symlinks.clone().unwrap_or_default()].concat());
                if let Some(symlinks) = &env.symlinks {
                    for symlink in symlinks {
                        reported_executables.insert(symlink.clone(), env.clone());
                    }
                }
                if let Some(exe) = env.executable.clone() {
                    reported_executables.insert(exe, env.clone());
                }
                if let Some(reporter) = reporter {
                    reporter.report_environment(&env);
                }
            }
        }
    }
}

fn get_python_in_bin(env: &PythonEnv, is_64bit: bool) -> Option<PythonEnvironment> {
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
    // Similarly /usr/local/python/current/bin/python might point to something like /usr/local/python/3.10.13/bin/python3.10
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
    if let Ok(symlink) = fs::canonicalize(&executable) {
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
            resolve_symlink(&possible_symlink).or(fs::canonicalize(possible_symlink).ok())
        {
            // Generally the file /bin/python3 is a symlink to /usr/bin/python3.12
            // Generally the file /bin/python3.12 is a symlink to /usr/bin/python3.12
            // Generally the file /usr/bin/python3 is a symlink to /usr/bin/python3.12
            // HOWEVER, we will be treating the files in /bin and /usr/bin as different.
            // Hence check whether the resolve symlink is in the same directory.
            if symlink.starts_with(bin) & symlinks.contains(symlink) {
                symlinks.push(possible_symlink.to_owned());
            }

            // Possible the env.executable = /bin/python3
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
        PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::LinuxGlobal))
            .executable(Some(executable))
            .version(env.version.clone())
            .arch(if is_64bit {
                Some(Architecture::X64)
            } else {
                Some(Architecture::X86)
            })
            .prefix(env.prefix.clone())
            .symlinks(Some(symlinks))
            .build(),
    )
}
