// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use log::error;
use pet_core::{
    arch::Architecture,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::manager::Manager;

// We want to maintain full control over serialization instead of relying on the enums or the like.
// Else its too easy to break the API by changing the enum variants.
fn python_category_to_string(category: &PythonEnvironmentCategory) -> &'static str {
    match category {
        PythonEnvironmentCategory::System => "system",
        PythonEnvironmentCategory::MacCommandLineTools => "mac-command-line-tools",
        PythonEnvironmentCategory::MacPythonOrg => "mac-python-org",
        PythonEnvironmentCategory::Homebrew => "homebrew",
        PythonEnvironmentCategory::Conda => "conda",
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
    pub category: &'static str,
    pub version: Option<String>,
    pub prefix: Option<PathBuf>,
    pub manager: Option<Manager>,
    pub project: Option<PathBuf>,
    pub arch: Option<&'static str>,
    pub symlinks: Option<Vec<PathBuf>>,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Environment ({})", self.category).unwrap_or_default();
        if let Some(name) = &self.display_name {
            writeln!(f, "   Display-Name: {}", name).unwrap_or_default();
        }
        if let Some(name) = &self.name {
            writeln!(f, "   Name        : {}", name).unwrap_or_default();
        }
        if let Some(exe) = &self.executable {
            writeln!(f, "   Executable  : {}", exe.to_str().unwrap_or_default())
                .unwrap_or_default();
        }
        if let Some(version) = &self.version {
            writeln!(f, "   Version     : {}", version).unwrap_or_default();
        }
        if let Some(prefix) = &self.prefix {
            writeln!(
                f,
                "   Prefix      : {}",
                prefix.to_str().unwrap_or_default()
            )
            .unwrap_or_default();
        }
        if let Some(project) = &self.project {
            writeln!(f, "   Project     : {}", project.to_str().unwrap()).unwrap_or_default();
        }
        if let Some(arch) = &self.arch {
            writeln!(f, "   Architecture: {}", arch).unwrap_or_default();
        }
        if let Some(manager) = &self.manager {
            writeln!(
                f,
                "   Manager     : {}, {}",
                manager.tool,
                manager.executable.to_str().unwrap_or_default()
            )
            .unwrap_or_default();
        }
        if let Some(symlinks) = &self.symlinks {
            let mut symlinks = symlinks.clone();
            symlinks.sort_by(|a, b| {
                a.to_str()
                    .unwrap_or_default()
                    .len()
                    .cmp(&b.to_str().unwrap_or_default().len())
            });

            if !symlinks.is_empty() {
                for (i, symlink) in symlinks.iter().enumerate() {
                    if i == 0 {
                        writeln!(f, "   Symlinks    : {:?}", symlink).unwrap_or_default();
                    } else {
                        writeln!(f, "               : {:?}", symlink).unwrap_or_default();
                    }
                }
            }
        }
        Ok(())
    }
}

impl Environment {
    pub fn from(env: &PythonEnvironment) -> Environment {
        Environment {
            display_name: env.display_name.clone(),
            name: env.name.clone(),
            executable: env.executable.clone(),
            category: python_category_to_string(&env.category),
            version: env.version.clone(),
            prefix: env.prefix.clone(),
            manager: match &env.manager {
                Some(manager) => Manager::from(manager).into(),
                None => None,
            },
            project: env.project.clone(),
            arch: env.arch.as_ref().map(architecture_to_string),
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
