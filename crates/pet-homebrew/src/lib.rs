// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::get_homebrew_prefix_bin;
use environments::get_python_info;
use pet_core::{
    os_environment::Environment, python_environment::PythonEnvironment, Locator, LocatorResult,
};
use pet_utils::{env::PythonEnv, executable::find_executables, path::resolve_symlink};
use std::{collections::HashSet, path::PathBuf};

mod env_variables;
mod environment_locations;
mod environments;
mod sym_links;

pub struct Homebrew {
    environment: EnvVariables,
}

impl Homebrew {
    pub fn from(environment: &dyn Environment) -> Homebrew {
        Homebrew {
            environment: EnvVariables::from(environment),
        }
    }
}

fn resolve(env: &PythonEnv, reported: &mut HashSet<String>) -> Option<PythonEnvironment> {
    let exe = env.executable.clone();
    let exe_file_name = exe.file_name()?;
    let resolved_file = resolve_symlink(&exe).unwrap_or(exe.clone());
    if resolved_file.starts_with("/opt/homebrew/Cellar") {
        // Symlink  - /opt/homebrew/bin/python3.12
        // Symlink  - /opt/homebrew/opt/python3/bin/python3.12
        // Symlink  - /opt/homebrew/Cellar/python@3.12/3.12.3/bin/python3.12
        // Symlink  - /opt/homebrew/opt/python@3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // Symlink  - /opt/homebrew/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // Real exe - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // SysPrefix- /opt/homebrew/opt/python@3.12/Frameworks/Python.framework/Versions/3.12
        get_python_info(
            &PathBuf::from("/opt/homebrew/bin").join(exe_file_name),
            reported,
            &resolved_file,
        )
    } else if resolved_file.starts_with("/home/linuxbrew/.linuxbrew/Cellar") {
        // Symlink  - /usr/local/bin/python3.12
        // Symlink  - /home/linuxbrew/.linuxbrew/bin/python3.12
        // Symlink  - /home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3.12
        // Real exe - /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3/bin/python3.12
        // SysPrefix- /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3
        get_python_info(
            &PathBuf::from("/usr/local/bin").join(exe_file_name),
            reported,
            &resolved_file,
        )
    } else if resolved_file.starts_with("/usr/local/Cellar") {
        // Symlink  - /usr/local/bin/python3.8
        // Symlink  - /usr/local/opt/python@3.8/bin/python3.8
        // Symlink  - /usr/local/Cellar/python@3.8/3.8.19/bin/python3.8
        // Real exe - /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8
        // SysPrefix- /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8
        get_python_info(
            &PathBuf::from("/usr/local/bin").join(exe_file_name),
            reported,
            &resolved_file,
        )
    } else {
        None
    }
}

impl Locator for Homebrew {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        let mut reported: HashSet<String> = HashSet::new();
        resolve(env, &mut reported)
    }

    fn find(&self) -> Option<LocatorResult> {
        let mut reported: HashSet<String> = HashSet::new();
        let mut environments: Vec<PythonEnvironment> = vec![];
        for homebrew_prefix_bin in get_homebrew_prefix_bin(&self.environment) {
            for file in find_executables(&homebrew_prefix_bin).iter().filter(|f| {
                let file_name = f
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .to_lowercase();
                file_name.starts_with("python")
                // If this file name is `python3`, then ignore this for now.
                    // We would prefer to use `python3.x` instead of `python3`.
                    // That way its more consistent and future proof
                        && file_name != "python3"
                        && file_name != "python"
            }) {
                // Sometimes we end up with other python installs in the Homebrew bin directory.
                // E.g. /usr/local/bin is treated as a location where homebrew can be found (homebrew bin)
                // However this is a very generic location, and we might end up with other python installs here.
                // Hence call `resolve` to correctly identify homebrew python installs.
                let env_to_resolve = PythonEnv::new(file.clone(), None, None);
                if let Some(env) = resolve(&env_to_resolve, &mut reported) {
                    environments.push(env);
                }
            }

            // Possible we do not have python3.12 or the like in bin directory
            // & we have only python3, in that case we should add python3 to the list
            let file = homebrew_prefix_bin.join("python3");
            let env_to_resolve = PythonEnv::new(file, None, None);
            if let Some(env) = resolve(&env_to_resolve, &mut reported) {
                environments.push(env);
            }
        }

        if environments.is_empty() {
            None
        } else {
            Some(LocatorResult {
                managers: vec![],
                environments,
            })
        }
    }
}
