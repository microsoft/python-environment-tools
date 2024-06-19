// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::manager::Manager;
use log::error;
use pet_core::{
    arch::Architecture,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// We want to maintain full control over serialization instead of relying on the enums or the like.
// Else its too easy to break the API by changing the enum variants.
fn python_category_to_string(category: &PythonEnvironmentCategory) -> &'static str {
    match category {
        PythonEnvironmentCategory::System => "system",
        PythonEnvironmentCategory::MacCommandLineTools => "mac-command-line-tools",
        PythonEnvironmentCategory::MacXCode => "mac-xcode",
        PythonEnvironmentCategory::MacPythonOrg => "mac-python-org",
        PythonEnvironmentCategory::GlobalPaths => "global-paths",
        PythonEnvironmentCategory::Homebrew => "homebrew",
        PythonEnvironmentCategory::Conda => "conda",
        PythonEnvironmentCategory::LinuxGlobal => "linux-global",
        PythonEnvironmentCategory::Pyenv => "pyenv",
        PythonEnvironmentCategory::PyenvVirtualEnv => "pyenv-virtualenv",
        PythonEnvironmentCategory::PyenvOther => "pyenv-other",
        PythonEnvironmentCategory::WindowsStore => "windows-store",
        PythonEnvironmentCategory::WindowsRegistry => "windows-registry",
        PythonEnvironmentCategory::Pipenv => "pipenv",
        PythonEnvironmentCategory::VirtualEnvWrapper => "virtualenvwrapper",
        PythonEnvironmentCategory::Venv => "venv",
        PythonEnvironmentCategory::VirtualEnv => "virtualenv",
        PythonEnvironmentCategory::Unknown => "unknown",
    }
}

// We want to maintain full control over serialization instead of relying on the enums or the like.
// Else its too easy to break the API by changing the enum variants.
fn architecture_to_string(arch: &Architecture) -> &'static str {
    match arch {
        Architecture::X64 => "x64",
        Architecture::X86 => "x86",
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct Environment {
    pub display_name: Option<String>,
    pub name: Option<String>,
    pub executable: Option<PathBuf>,
    pub category: String,
    pub version: Option<String>,
    pub prefix: Option<PathBuf>,
    pub manager: Option<Manager>,
    pub project: Option<PathBuf>,
    pub arch: Option<String>,
    pub symlinks: Option<Vec<PathBuf>>,
}

impl Environment {
    pub fn from(env: &PythonEnvironment) -> Environment {
        Environment {
            display_name: env.display_name.clone(),
            name: env.name.clone(),
            executable: env.executable.clone(),
            category: python_category_to_string(&env.category).to_string(),
            version: env.version.clone(),
            prefix: env.prefix.clone(),
            manager: match &env.manager {
                Some(manager) => Manager::from(manager).into(),
                None => None,
            },
            project: env.project.clone(),
            arch: env
                .arch
                .as_ref()
                .map(architecture_to_string)
                .map(|s| s.to_string()),
            symlinks: env.symlinks.clone(),
        }
    }
}

pub fn get_environment_key(env: &PythonEnvironment) -> Option<PathBuf> {
    if let Some(exe) = &env.executable {
        Some(exe.clone())
    } else if let Some(prefix) = &env.prefix {
        // If this is a conda env without Python, then the exe will be prefix/bin/python
        if env.category == PythonEnvironmentCategory::Conda {
            Some(prefix.join("bin").join(if cfg!(windows) {
                "python.exe"
            } else {
                "python"
            }))
        } else {
            Some(prefix.clone())
        }
    } else {
        error!(
            "Failed to report environment due to lack of exe & prefix: {:?}",
            env
        );
        None
    }
}
