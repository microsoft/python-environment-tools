// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace};
use pet_fs::path::norm_case;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::pyvenv_cfg::PyVenvCfg;

#[derive(Debug)]
pub struct PythonEnv {
    /// Executable of the Python environment.
    ///
    /// Can be `/usr/bin/python` or `/opt/homebrew/bin/python3.12`.
    /// Or even the fully (resolved &) qualified path to the python executable such as `/opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12`.
    ///
    /// Note: This can be a symlink as well.
    pub executable: PathBuf,
    /// Environment prefix
    pub prefix: Option<PathBuf>,
    /// Version of the Python environment.
    pub version: Option<String>,
    /// Possible symlink (or known alternative link).
    /// For instance:
    ///
    /// If `executable` is `/opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12`,
    /// then `symlink`` can be `/opt/homebrew/bin/python3.12` (or vice versa).
    pub symlinks: Option<Vec<PathBuf>>,
}

impl PythonEnv {
    pub fn new(executable: PathBuf, prefix: Option<PathBuf>, version: Option<String>) -> Self {
        let mut prefix = prefix.clone();
        if let Some(value) = prefix {
            prefix = norm_case(value).into();
        }
        // if the prefix is not defined, try to get this.
        // For instance, if the file is bin/python or Scripts/python
        // And we have a pyvenv.cfg file in the parent directory, then we can get the prefix.
        if prefix.is_none() {
            let mut exe = executable.clone();
            exe.pop();
            if exe.ends_with("Scripts") || exe.ends_with("bin") {
                exe.pop();
                if PyVenvCfg::find(&exe).is_some() {
                    prefix = Some(exe);
                }
            }
        }
        Self {
            executable: norm_case(executable),
            prefix,
            version,
            symlinks: None,
        }
    }
}

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
    pub symlink: Option<Vec<PathBuf>>,
}

impl ResolvedPythonEnv {
    pub fn to_python_env(&self) -> PythonEnv {
        let mut env = PythonEnv::new(
            self.executable.clone(),
            Some(self.prefix.clone()),
            Some(self.version.clone()),
        );
        env.symlinks.clone_from(&self.symlink);
        env
    }
    pub fn from(executable: &Path) -> Option<Self> {
        // Spawn the python exe and get the version, sys.prefix and sys.executable.
        let executable = executable.to_str()?;
        trace!("Executing Python: {} -c {}", executable, PYTHON_INFO_CMD);
        let result = std::process::Command::new(executable)
            .args(["-c", PYTHON_INFO_CMD])
            .output();
        match result {
            Ok(output) => {
                let output = String::from_utf8(output.stdout).unwrap().trim().to_string();
                trace!(
                    "Python Execution for {:?} produced an output {:?}",
                    executable,
                    output
                );
                if let Some((_, output)) = output.split_once(PYTHON_INFO_JSON_SEPARATOR) {
                    if let Ok(info) = serde_json::from_str::<InterpreterInfo>(output) {
                        Some(Self {
                            executable: PathBuf::from(info.executable.clone()),
                            prefix: PathBuf::from(info.sys_prefix),
                            version: info.version.trim().to_string(),
                            is64_bit: info.is64_bit,
                            symlink: if info.executable == executable {
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
}
