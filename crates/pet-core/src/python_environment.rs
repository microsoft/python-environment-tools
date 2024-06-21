// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::error;
use pet_fs::path::norm_case;
use pet_python_utils::executable::get_shortest_executable;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{arch::Architecture, manager::EnvManager};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Debug, Hash)]
pub enum PythonEnvironmentCategory {
    Conda,
    Homebrew,
    Pyenv,           // Relates to Python installations in pyenv that are from Python org.
    GlobalPaths,     // Python found in global locations like PATH, /usr/bin etc.
    PyenvVirtualEnv, // Pyenv virtualenvs.
    PyenvOther,      // Such as pyston, stackless, nogil, etc.
    Pipenv,
    Poetry,
    System,
    MacPythonOrg,
    MacCommandLineTools,
    LinuxGlobal,
    MacXCode,
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

impl std::fmt::Display for PythonEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Environment ({:?})", self.category).unwrap_or_default();
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
                "   Manager     : {:?}, {}",
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
                    self.executable = Some(norm_case(parent).join(file_name))
                }
            }
        }
        self.update_symlinks_and_exe(self.symlinks.clone());
        self
    }

    pub fn version(mut self, version: Option<String>) -> Self {
        self.version = version;
        self
    }

    pub fn prefix(mut self, prefix: Option<PathBuf>) -> Self {
        self.prefix.clone_from(&prefix);
        if let Some(resolved) = prefix {
            self.prefix = Some(norm_case(resolved))
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
            self.project = Some(norm_case(resolved))
        }
        self
    }

    pub fn arch(mut self, arch: Option<Architecture>) -> Self {
        self.arch = arch;
        self
    }

    pub fn symlinks(mut self, symlinks: Option<Vec<PathBuf>>) -> Self {
        self.update_symlinks_and_exe(symlinks);
        self
    }

    fn update_symlinks_and_exe(&mut self, symlinks: Option<Vec<PathBuf>>) {
        let mut all = vec![];
        if let Some(ref exe) = self.executable {
            all.push(exe.clone());
        }
        if let Some(symlinks) = symlinks {
            all.extend(symlinks);
        }
        all.sort();
        all.dedup();

        self.symlinks = if all.is_empty() {
            None
        } else {
            Some(all.clone())
        };
        if let Some(executable) = &self.executable {
            self.executable =
                Some(get_shortest_executable(&Some(all.clone())).unwrap_or(executable.clone()));
        }
    }

    pub fn build(self) -> PythonEnvironment {
        let mut all = vec![];
        if let Some(ref exe) = self.executable {
            all.push(exe.clone());
        }
        if let Some(symlinks) = self.symlinks {
            all.extend(symlinks);
        }
        all.sort();
        all.dedup();

        let symlinks = if all.is_empty() {
            None
        } else {
            Some(all.clone())
        };
        let executable = self
            .executable
            .map(|executable| get_shortest_executable(&Some(all.clone())).unwrap_or(executable));

        PythonEnvironment {
            display_name: self.display_name,
            name: self.name,
            executable,
            category: self.category,
            version: self.version,
            prefix: self.prefix,
            manager: self.manager,
            project: self.project,
            arch: self.arch,
            symlinks,
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
