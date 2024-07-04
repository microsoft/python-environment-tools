// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::error;
use pet_fs::path::norm_case;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{arch::Architecture, manager::EnvManager};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PythonEnvironmentKind {
    Conda,
    Homebrew,
    Pyenv,           // Relates to Python installations in pyenv that are from Python org.
    GlobalPaths,     // Python found in global locations like PATH, /usr/bin etc.
    PyenvVirtualEnv, // Pyenv virtualenvs.
    Pipenv,
    Poetry,
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
impl Ord for PythonEnvironmentKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{self:?}").cmp(&format!("{other:?}"))
    }
}
impl PartialOrd for PythonEnvironmentKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Debug, Default)]
// Python environment.
// Any item that has information is known to be accurate, if an item is missing it is unknown.
pub struct PythonEnvironment {
    // Display name as provided by the tool, Windows Store & Windows Registry have display names defined in registry.
    pub display_name: Option<String>,
    // The name of the environment. Primarily applies to conda environments.
    pub name: Option<String>,
    // Python executable, can be empty in the case of conda envs that do not have Python installed in them.
    pub executable: Option<PathBuf>,
    pub kind: Option<PythonEnvironmentKind>,
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
    /// The folder/path that was searched to find this environment.
    /// Generally applies to workspace folder, and means that the environment was found in this folder or is related to this folder.
    /// Similar in meaqning to `project` but is more of a search path.
    pub search_path: Option<PathBuf>,
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

impl PythonEnvironment {
    pub fn new(
        executable: Option<PathBuf>,
        kind: Option<PythonEnvironmentKind>,
        prefix: Option<PathBuf>,
        manager: Option<EnvManager>,
        version: Option<String>,
    ) -> Self {
        Self {
            executable,
            kind,
            version,
            prefix,
            manager,
            ..Default::default()
        }
    }
}

impl std::fmt::Display for PythonEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Environment ({:?})", self.kind).unwrap_or_default();
        if let Some(name) = &self.display_name {
            writeln!(f, "   Display-Name: {name}").unwrap_or_default();
        }
        if let Some(name) = &self.name {
            writeln!(f, "   Name        : {name}").unwrap_or_default();
        }
        if let Some(exe) = &self.executable {
            writeln!(f, "   Executable  : {}", exe.to_str().unwrap_or_default())
                .unwrap_or_default();
        }
        if let Some(version) = &self.version {
            writeln!(f, "   Version     : {version}").unwrap_or_default();
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
        if let Some(search_path) = &self.search_path {
            writeln!(f, "   Search Path : {}", search_path.to_str().unwrap()).unwrap_or_default();
        }
        if let Some(arch) = &self.arch {
            writeln!(f, "   Architecture: {arch}").unwrap_or_default();
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
                        writeln!(f, "   Symlinks    : {symlink:?}").unwrap_or_default();
                    } else {
                        writeln!(f, "               : {symlink:?}").unwrap_or_default();
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
    kind: Option<PythonEnvironmentKind>,
    version: Option<String>,
    prefix: Option<PathBuf>,
    manager: Option<EnvManager>,
    project: Option<PathBuf>,
    arch: Option<Architecture>,
    symlinks: Option<Vec<PathBuf>>,
    search_path: Option<PathBuf>,
}

impl PythonEnvironmentBuilder {
    pub fn new(kind: Option<PythonEnvironmentKind>) -> Self {
        Self {
            kind,
            display_name: None,
            name: None,
            executable: None,
            version: None,
            prefix: None,
            manager: None,
            project: None,
            arch: None,
            symlinks: None,
            search_path: None,
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

    pub fn search_path(mut self, search_path: Option<PathBuf>) -> Self {
        self.search_path = search_path;
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
            self.executable = Some(
                get_shortest_executable(&self.kind, &Some(all.clone()))
                    .unwrap_or(executable.clone()),
            );
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
        let executable = self.executable.map(|executable| {
            get_shortest_executable(&self.kind, &Some(all.clone())).unwrap_or(executable)
        });

        PythonEnvironment {
            display_name: self.display_name,
            name: self.name,
            executable,
            kind: self.kind,
            version: self.version,
            prefix: self.prefix,
            manager: self.manager,
            project: self.project,
            arch: self.arch,
            search_path: self.search_path,
            symlinks,
        }
    }
}

// Given a list of executables, return the one with the shortest path.
// The shortest path is the most likely to be most user friendly.
fn get_shortest_executable(
    kind: &Option<PythonEnvironmentKind>,
    exes: &Option<Vec<PathBuf>>,
) -> Option<PathBuf> {
    // For windows store, the exe should always be the one in the WindowsApps folder.
    if *kind == Some(PythonEnvironmentKind::WindowsStore) {
        if let Some(exes) = exes {
            if let Some(exe) = exes.iter().find(|e| {
                e.to_string_lossy().contains("AppData")
                    && e.to_string_lossy().contains("Local")
                    && e.to_string_lossy().contains("Microsoft")
                    && e.to_string_lossy().contains("WindowsApps")
            }) {
                return Some(exe.clone());
            }
        }
    }

    // Ensure the executable always points to the shorted path.
    if let Some(mut exes) = exes.clone() {
        exes.sort_by(|a, b| {
            a.to_str()
                .unwrap_or_default()
                .len()
                .cmp(&b.to_str().unwrap_or_default().len())
        });
        if exes.is_empty() {
            return None;
        }
        Some(exes[0].clone())
    } else {
        None
    }
}

pub fn get_environment_key(env: &PythonEnvironment) -> Option<PathBuf> {
    if let Some(exe) = &env.executable {
        Some(exe.clone())
    } else if let Some(prefix) = &env.prefix {
        // If this is a conda env without Python, then the exe will be prefix/bin/python
        if env.kind == Some(PythonEnvironmentKind::Conda) {
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
