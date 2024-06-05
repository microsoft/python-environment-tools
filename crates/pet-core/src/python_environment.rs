// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use pet_utils::path::normalize;
use serde::{Deserialize, Serialize};

use crate::{arch::Architecture, manager::EnvManager};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub enum PythonEnvironmentCategory {
    Conda,
    Homebrew,
    Pyenv,
    PyenvVirtualEnv,
    Pipenv,
    System,
    Unknown,
    Venv,
    VirtualEnv,
    VirtualEnvWrapper,
    WindowsStore,
    WindowsRegistry,
}
impl Ord for PythonEnvironmentCategory {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{:?}", self).cmp(&format!("{:?}", other))
    }
}
impl PartialOrd for PythonEnvironmentCategory {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
// Python environment.
// Any item that has information is known to be accurate, if an item is missing it is unknown.
pub struct PythonEnvironment {
    // Display name as provided by the tool, Windows Store & Windows Registry have display names defined in registry.
    pub display_name: Option<String>,
    // The name of the environment. Primarily applies to conda environments.
    pub name: Option<String>,
    // Python executable, can be empty in the case of conda envs that do not have Python installed in them.
    pub executable: Option<PathBuf>,
    pub category: PythonEnvironmentCategory,
    pub version: Option<String>,
    // SysPrefix for the environment.
    pub prefix: Option<PathBuf>,
    pub manager: Option<EnvManager>,
    /**
     * The project path for the Pipenv, VirtualEnvWrapper, Hatch environment & the like.
     * Basically this is the folder that a particular environment is associated with.
     */
    pub project: Option<PathBuf>,
    // Architecture of the environment.
    // E.g. its possible to have a 32bit python in a 64bit OS.
    pub arch: Option<Architecture>,
    // Some of the known symlinks for the environment.
    // E.g. in the case of Homebrew there are a number of symlinks that are created.
    pub symlinks: Option<Vec<PathBuf>>,
}
impl Ord for PythonEnvironment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!(
            "{:?}=>{:?}",
            self.executable.clone().unwrap_or_default(),
            self.prefix.clone().unwrap_or_default()
        )
        .cmp(&format!(
            "{:?}=>{:?}",
            other.executable.clone().unwrap_or_default(),
            other.prefix.clone().unwrap_or_default()
        ))
    }
}
impl PartialOrd for PythonEnvironment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Default for PythonEnvironment {
    fn default() -> Self {
        Self {
            display_name: None,
            name: None,
            executable: None,
            // Sometimes we might not know the env type.
            // Lets never default these to System/Global or others as thats not true.
            // Not knowing does not mean it is a system env.
            category: PythonEnvironmentCategory::Unknown,
            version: None,
            prefix: None,
            manager: None,
            project: None,
            arch: None,
            symlinks: None,
        }
    }
}

impl PythonEnvironment {
    pub fn new(
        executable: Option<PathBuf>,
        category: PythonEnvironmentCategory,
        prefix: Option<PathBuf>,
        manager: Option<EnvManager>,
        version: Option<String>,
    ) -> Self {
        Self {
            executable,
            category,
            version,
            prefix,
            manager,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct PythonEnvironmentBuilder {
    display_name: Option<String>,
    name: Option<String>,
    executable: Option<PathBuf>,
    category: PythonEnvironmentCategory,
    version: Option<String>,
    prefix: Option<PathBuf>,
    manager: Option<EnvManager>,
    project: Option<PathBuf>,
    arch: Option<Architecture>,
    symlinks: Option<Vec<PathBuf>>,
}

impl PythonEnvironmentBuilder {
    pub fn new(category: PythonEnvironmentCategory) -> Self {
        Self {
            category,
            display_name: None,
            name: None,
            executable: None,
            version: None,
            prefix: None,
            manager: None,
            project: None,
            arch: None,
            symlinks: None,
        }
    }

    pub fn display_name(mut self, display_name: Option<String>) -> Self {
        self.display_name = display_name;
        self
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn executable(mut self, executable: Option<PathBuf>) -> Self {
        self.executable.clone_from(&executable);
        if let Some(exe) = executable {
            if let Some(parent) = exe.parent() {
                if let Some(file_name) = exe.file_name() {
                    self.executable = Some(normalize(parent).join(file_name))
                }
            }
        }
        self
    }

    pub fn version(mut self, version: Option<String>) -> Self {
        self.version = version;
        self
    }

    pub fn prefix(mut self, prefix: Option<PathBuf>) -> Self {
        self.prefix.clone_from(&prefix);
        if let Some(resolved) = prefix {
            self.prefix = Some(normalize(resolved))
        }
        self
    }

    pub fn manager(mut self, manager: Option<EnvManager>) -> Self {
        self.manager = manager;
        self
    }

    pub fn project(mut self, project: Option<PathBuf>) -> Self {
        self.project.clone_from(&project);
        if let Some(resolved) = project {
            self.project = Some(normalize(resolved))
        }
        self
    }

    pub fn arch(mut self, arch: Option<Architecture>) -> Self {
        self.arch = arch;
        self
    }

    pub fn symlinks(mut self, symlinks: Option<Vec<PathBuf>>) -> Self {
        self.symlinks = symlinks;
        self
    }

    pub fn build(self) -> PythonEnvironment {
        PythonEnvironment {
            display_name: self.display_name,
            name: self.name,
            executable: self.executable,
            category: self.category,
            version: self.version,
            prefix: self.prefix,
            manager: self.manager,
            project: self.project,
            arch: self.arch,
            symlinks: self.symlinks,
        }
    }
}
