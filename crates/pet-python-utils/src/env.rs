// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace};
use pet_core::{arch::Architecture, env::PythonEnv, python_environment::PythonEnvironment};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{cache::create_cache, executable::new_silent_command};

const PYTHON_INFO_JSON_SEPARATOR: &str = "093385e9-59f7-4a16-a604-14bf206256fe";
const PYTHON_INFO_CMD:&str = "import json, sys; print('093385e9-59f7-4a16-a604-14bf206256fe');print(json.dumps({'version': '.'.join(str(n) for n in sys.version_info), 'sys_prefix': sys.prefix, 'executable': sys.executable, 'is64_bit': sys.maxsize > 2**32}))";

#[derive(Debug, Deserialize, Clone)]
pub struct InterpreterInfo {
    pub version: String,
    pub sys_prefix: String,
    pub executable: String,
    pub is64_bit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPythonEnv {
    pub executable: PathBuf,
    pub prefix: PathBuf,
    pub version: String,
    pub is64_bit: bool,
    pub symlinks: Option<Vec<PathBuf>>,
}

impl ResolvedPythonEnv {
    pub fn to_python_env(&self) -> PythonEnv {
        let mut env = PythonEnv::new(
            self.executable.clone(),
            Some(self.prefix.clone()),
            Some(self.version.clone()),
        );
        env.symlinks.clone_from(&self.symlinks);
        env
    }
    pub fn add_to_cache(&self, environment: PythonEnvironment) {
        // Verify whether we have been given the right exe.
        let arch = Some(if self.is64_bit {
            Architecture::X64
        } else {
            Architecture::X86
        });
        let symlinks = environment.symlinks.clone().unwrap_or_default();
        if symlinks.contains(&self.executable)
            && environment.version.clone().unwrap_or_default() == self.version
            && environment.prefix.clone().unwrap_or_default() == self.prefix
            && environment.arch == arch
        {
            let cache = create_cache(self.executable.clone());
            let entry = cache.lock().expect("cache mutex poisoned");
            entry.track_symlinks(symlinks)
        } else {
            error!(
                "Invalid Python environment being cached: {:?} expected {:?}",
                environment, self
            );
        }
    }
    /// Given the executable path, resolve the python environment by spawning python.
    /// If we had previously spawned Python and we have the symlinks to this as well,
    /// & all of them are the same as when this exe was previously spawned,
    /// & mtime & ctimes of none of the exes (symlinks) have changed, then we can use the cached info.
    pub fn from(
        executable: &Path,
        // known_symlinks: &Vec<PathBuf>,
        // cache: &dyn Cache,
    ) -> Option<Self> {
        let cache = create_cache(executable.to_path_buf());
        let entry = cache.lock().expect("cache mutex poisoned");
        if let Some(env) = entry.get() {
            Some(env)
        } else if let Some(env) = get_interpreter_details(executable) {
            entry.store(env.clone());
            Some(env)
        } else {
            None
        }
    }
}

fn get_interpreter_details(executable: &Path) -> Option<ResolvedPythonEnv> {
    // Spawn the python exe and get the version, sys.prefix and sys.executable.
    let executable = executable.to_str()?;
    let start = SystemTime::now();
    trace!("Executing Python: {} -c {}", executable, PYTHON_INFO_CMD);
    let result = new_silent_command(executable)
        .args(["-c", PYTHON_INFO_CMD])
        .output();
    match result {
        Ok(output) => {
            let output = String::from_utf8(output.stdout).unwrap().trim().to_string();
            trace!(
                "Executed Python {:?} in {:?} & produced an output {:?}",
                executable,
                start.elapsed(),
                output
            );
            if let Some((_, output)) = output.split_once(PYTHON_INFO_JSON_SEPARATOR) {
                if let Ok(info) = serde_json::from_str::<InterpreterInfo>(output) {
                    let mut symlinks = vec![
                        PathBuf::from(executable),
                        PathBuf::from(info.executable.clone()),
                    ];
                    symlinks.sort();
                    symlinks.dedup();
                    Some(ResolvedPythonEnv {
                        executable: PathBuf::from(info.executable.clone()),
                        prefix: PathBuf::from(info.sys_prefix),
                        version: info.version.trim().to_string(),
                        is64_bit: info.is64_bit,
                        symlinks: Some(symlinks),
                    })
                } else {
                    error!(
                            "Python Execution for {:?} produced an output {:?} that could not be parsed as JSON",
                            executable, output,
                        );
                    None
                }
            } else {
                error!(
                    "Python Execution for {:?} produced an output {:?} without a separator",
                    executable, output,
                );
                None
            }
        }
        Err(err) => {
            error!(
                "Failed to execute Python to resolve info {:?}: {}",
                executable, err
            );
            None
        }
    }
}
