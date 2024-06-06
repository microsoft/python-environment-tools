// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, thread};

use env_variables::EnvVariables;
use lazy_static::lazy_static;
use log::warn;
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    Locator, LocatorResult,
};
use pet_utils::{
    env::PythonEnv, executable::{find_executables, get_shortest_executable}, headers::Headers, path::resolve_symlink, pyvenv_cfg::PyVenvCfg
};
use regex::Regex;

lazy_static! {
    // /Library/Frameworks/Python.framework/Versions/3.10/bin/python3.10
    static ref PYTHON_FRAMEWORK_VERSION: Regex = Regex::new("^python-([\\d+\\.*]*)-.*.json$")
        .expect("error parsing Version regex for Python.Framework Version in Python env Paths");
}

mod env_variables;

pub struct PythonOnPath {
    pub env_vars: EnvVariables,
}

impl PythonOnPath {
    pub fn from(environment: &dyn Environment) -> PythonOnPath {
        PythonOnPath {
            env_vars: EnvVariables::from(environment),
        }
    }
}

impl Locator for PythonOnPath {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        let exe = &env.executable;
        let mut env = PythonEnvironmentBuilder::new(PythonEnvironmentCategory::System)
            .executable(Some(exe.clone()))
            .version(env.version.clone())
            .prefix(env.prefix.clone())
            .build();
        if let Some(symlink) = resolve_symlink(exe) {
            env.symlinks = Some(vec![symlink.clone(), exe.clone()]);
            // Getting version this way is more accurate than the above regex.
            // Sample paths
            // /Library/Frameworks/Python.framework/Versions/3.10/bin/python3.10
            if symlink.starts_with("/Library/Frameworks/Python.framework/Versions") {
                if let Some(captures) = PYTHON_FRAMEWORK_VERSION.captures(symlink.to_str().unwrap())
                {
                    let version = captures.get(1).map_or("", |m| m.as_str());
                    if !version.is_empty() {
                        env.version = Some(version.to_string());
                    }
                }
                // Sample paths
                // /Library/Frameworks/Python.framework/Versions/3.10/bin/python3.10
                if let Some(parent) = symlink.ancestors().nth(2) {
                    if let Some(version) = Headers::get_version(parent) {
                        env.version = Some(version);
                    }
                }

                if let Some(env_path) = symlink.ancestors().nth(2) {
                    env.prefix = Some(env_path.to_path_buf());
                }
            }
        } else if env.prefix.is_some() && env.version.is_none() {
            if let Some(env_path) = env.prefix.as_ref() {
                env.version = Headers::get_version(env_path);
            }
        }
        Some(env)
    }

    fn find(&self) -> Option<LocatorResult> {
        // Exclude files from this folder, as they would have been discovered elsewhere (widows_store)
        // Also the exe is merely a pointer to another file.
        let home = self.env_vars.home.clone()?;
        let apps_path = home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps");

        let items = self
            .env_vars
            .known_global_search_locations
            .clone()
            .into_iter()
            .filter(|p| !p.starts_with(apps_path.clone()))
            .collect::<Vec<PathBuf>>();

        let mut handles: Vec<std::thread::JoinHandle<Vec<PathBuf>>> = vec![];
        for item in items.chunks(5) {
            let lst = item.to_vec();
            let handle = thread::spawn(move || {
                lst.iter()
                    // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
                    // Hence do not just look for files in a bin directory of the path.
                    .flat_map(|p| find_executables(p))
                    .filter(|p| {
                        // Exclude python2 on macOS
                        if std::env::consts::OS == "macos" {
                            return p.to_str().unwrap_or_default() != "/usr/bin/python2";
                        }
                        true
                    })
                    .collect::<Vec<PathBuf>>()
            });
            handles.push(handle);
        }
        let mut python_executables: Vec<PathBuf> = vec![];
        for handle in handles {
            if let Ok(ref mut result) = handle.join() {
                python_executables.append(result)
            }
        }

        // The python executables can contain files like
        // /usr/local/bin/python3.10
        // /usr/local/bin/python3
        // Possible both of the above are symlinks and point to the same file.
        // Hence sort on length of the path.
        // So that we process generic python3 before python3.10
        python_executables.sort();
        python_executables.dedup();
        python_executables.sort_by(|a, b| {
            a.to_str()
                .unwrap_or_default()
                .len()
                .cmp(&b.to_str().unwrap_or_default().len())
        });

        let mut already_found: HashMap<PathBuf, RefCell<PythonEnvironment>> = HashMap::new();
        python_executables.into_iter().for_each(|exe| {
            if let Some(exe_dir) = exe.parent() {
                let mut version = None;
                let symlink = match PyVenvCfg::find(exe_dir) {
                    Some(version_value) => {
                        version = Some(version_value.version);
                        // We got a version from pyvenv.cfg file, that means we're looking at a virtual env.
                        // This should not happen.
                        warn!(
                            "Found a virtual env but identified as global Python: {:?}",
                            exe
                        );
                        // Its already fully resolved as we managed to get the env version from a pyvenv.cfg in current dir.
                        None
                    }
                    None => resolve_symlink(&exe.clone()),
                };
                if let Some(ref symlink) = symlink {
                    if already_found.contains_key(symlink) {
                        // If we have a symlinked file then, ensure the original path is added as symlink.
                        // Possible we only added /usr/local/bin/python3.10 and not /usr/local/bin/python3
                        // This entry is /usr/local/bin/python3
                        if let Some(existing) = already_found.get_mut(&exe) {
                            let mut existing = existing.borrow_mut();
                            if let Some(ref mut symlinks) = existing.symlinks {
                                symlinks.push(exe.clone());
                            } else {
                                existing.symlinks = Some(vec![symlink.clone(), exe.clone()]);
                            }

                            if let Some(shortest_exe) = get_shortest_executable(&existing.symlinks)
                            {
                                existing.executable = Some(shortest_exe);
                            }
                        }
                        return;
                    }
                }

                if let Some(env) = self.from(&PythonEnv::new(exe.clone(), None, version)) {
                    let mut env = env.clone();
                    let mut symlinks: Option<Vec<PathBuf>> = None;
                    if let Some(ref symlink) = symlink {
                        symlinks = Some(vec![symlink.clone(), exe.clone()]);
                    }
                    env.symlinks.clone_from(&symlinks);
                    if let Some(shortest_exe) = get_shortest_executable(&symlinks) {
                        env.executable = Some(shortest_exe);
                    }

                    let env = RefCell::new(env);
                    already_found.insert(exe, env.clone());
                    if let Some(symlinks) = symlinks.clone() {
                        for symlink in symlinks {
                            already_found.insert(symlink.clone(), env.clone());
                        }
                    }
                }
            }
        });

        if already_found.is_empty() {
            None
        } else {
            Some(LocatorResult {
                environments: already_found.values().map(|v| v.borrow().clone()).collect(),
                managers: vec![],
            })
        }
    }
}
