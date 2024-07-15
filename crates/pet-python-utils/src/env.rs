// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace};
use pet_core::env::PythonEnv;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::cache::create_cache;

const PYTHON_INFO_JSON_SEPARATOR: &str = "093385e9-59f7-4a16-a604-14bf206256fe";
const PYTHON_INFO_CMD:&str = "import json, sys; print('093385e9-59f7-4a16-a604-14bf206256fe');print(json.dumps({'version': '.'.join(str(n) for n in sys.version_info), 'sys_prefix': sys.prefix, 'executable': sys.executable, 'is64_bit': sys.maxsize > 2**32}))";

#[derive(Debug, Deserialize, Clone)]
pub struct InterpreterInfo {
    pub version: String,
    pub sys_prefix: String,
    pub executable: String,
    pub is64_bit: bool,
}

#[derive(Debug)]
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
        let entry = cache.lock().unwrap();
        if let Some(env) = entry.get() {
            if let (Some(exe), Some(prefix), Some(version)) =
                (env.executable, env.prefix, env.version)
            {
                // Ensure the given exe is in the list of symlinks.
                if env
                    .symlinks
                    .clone()
                    .unwrap_or_default()
                    .contains(&executable.to_path_buf())
                {
                    return Some(ResolvedPythonEnv {
                        executable: exe,
                        is64_bit: false,
                        prefix,
                        symlinks: env.symlinks.clone(),
                        version,
                    });
                }
            }
        }

        if let Some(env) = get_interpreter_details(executable) {
            entry.store(env);
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
    let result = std::process::Command::new(executable)
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
                    Some(ResolvedPythonEnv {
                        executable: PathBuf::from(info.executable.clone()),
                        prefix: PathBuf::from(info.sys_prefix),
                        version: info.version.trim().to_string(),
                        is64_bit: info.is64_bit,
                        symlinks: if info.executable == executable {
                            None
                        } else {
                            Some(vec![PathBuf::from(executable)])
                        },
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
