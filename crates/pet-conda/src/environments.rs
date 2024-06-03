// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    manager::CondaManager,
    package::{CondaPackageInfo, Package},
    utils::{is_conda_env, is_conda_install},
};
use pet_core::{
    arch::Architecture,
    manager::EnvManager,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
};
use pet_utils::executable::find_executable;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CondaEnvironment {
    pub prefix: PathBuf,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub conda_dir: Option<PathBuf>,
    pub arch: Option<Architecture>,
}

impl CondaEnvironment {
    pub fn from(path: &Path, manager: &Option<CondaManager>) -> Option<Self> {
        get_conda_environment_info(&path.into(), manager)
    }

    pub fn to_python_environment(
        &self,
        conda_manager: EnvManager,
        conda_dir: &PathBuf,
    ) -> PythonEnvironment {
        #[allow(unused_assignments)]
        let mut name: Option<String> = None;
        if is_conda_install(&self.prefix) {
            name = Some("base".to_string());
        } else {
            name = match self.prefix.file_name() {
                Some(name) => Some(name.to_str().unwrap_or_default().to_string()),
                None => None,
            };
        }
        // if the conda install folder is parent of the env folder, then we can use named activation.
        // E.g. conda env is = <conda install>/envs/<env name>
        // Then we can use `<conda install>/bin/conda activate -n <env name>`
        if !self.prefix.starts_with(&conda_dir) {
            name = None;
        }
        // This is a root env.
        let builder = PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Conda)
            .executable(self.executable.clone())
            .version(self.version.clone())
            .prefix(Some(self.prefix.clone()))
            .arch(self.arch.clone())
            .name(name.clone())
            .manager(Some(conda_manager.clone()));

        builder.build()
    }
}
fn get_conda_environment_info(
    env_path: &PathBuf,
    manager: &Option<CondaManager>,
) -> Option<CondaEnvironment> {
    if !is_conda_env(env_path) {
        // Not a conda environment (neither root nor a separate env).
        return None;
    }
    // If we know the conda install folder, then we can use it.
    let conda_install_folder = match manager {
        Some(manager) => Some(manager.conda_dir.clone()),
        None => get_conda_installation_used_to_create_conda_env(env_path),
    };
    let env_path = env_path.clone();
    if let Some(python_binary) = find_executable(&env_path) {
        if let Some(package_info) = CondaPackageInfo::from(&env_path, &Package::Python) {
            return Some(CondaEnvironment {
                prefix: env_path,
                executable: Some(python_binary),
                version: Some(package_info.version),
                conda_dir: conda_install_folder,
                arch: package_info.arch,
            });
        } else {
            // No python in this environment.
            return Some(CondaEnvironment {
                prefix: env_path,
                executable: Some(python_binary),
                version: None,
                conda_dir: conda_install_folder,
                arch: None,
            });
        }
    } else {
        // No python in this environment.
        return Some(CondaEnvironment {
            prefix: env_path,
            executable: None,
            version: None,
            conda_dir: conda_install_folder,
            arch: None,
        });
    }
}

/**
 * The conda-meta/history file in conda environments contain the command used to create the conda environment.
 * And example is `# cmd: <conda install directory>\Scripts\conda-script.py create -n sample``
 * And example is `# cmd: conda create -n sample``
 *
 * Sometimes the cmd line contains the fully qualified path to the conda install folder.
 * This function returns the path to the conda installation that was used to create the environment.
 */
pub fn get_conda_installation_used_to_create_conda_env(env_path: &PathBuf) -> Option<PathBuf> {
    // Possible the env_path is the root conda install folder.
    if is_conda_install(env_path) {
        return Some(env_path.to_path_buf());
    }

    // If this environment is in a folder named `envs`, then the parent directory of `envs` is the root conda install folder.
    if let Some(parent) = env_path.ancestors().nth(2) {
        if is_conda_install(parent) {
            return Some(parent.to_path_buf());
        }
    }

    let conda_meta_history = env_path.join("conda-meta").join("history");
    if let Ok(reader) = std::fs::read_to_string(conda_meta_history.clone()) {
        if let Some(line) = reader.lines().map(|l| l.trim()).find(|l| {
            l.to_lowercase().starts_with("# cmd:") && l.to_lowercase().contains(" create -")
        }) {
            // Sample lines
            // # cmd: <conda install directory>\Scripts\conda-script.py create -n samlpe1
            // # cmd: <conda install directory>\Scripts\conda-script.py create -p <full path>
            // # cmd: /Users/donjayamanne/miniconda3/bin/conda create -n conda1
            let start_index = line.to_lowercase().find("# cmd:")? + "# cmd:".len();
            let end_index = line.to_lowercase().find(" create -")?;
            let cmd_line = PathBuf::from(line[start_index..end_index].trim().to_string());
            if let Some(cmd_line) = cmd_line.parent() {
                if let Some(conda_dir) = cmd_line.file_name() {
                    if conda_dir.to_ascii_lowercase() == "bin"
                        || conda_dir.to_ascii_lowercase() == "scripts"
                    {
                        if let Some(conda_dir) = cmd_line.parent() {
                            return Some(conda_dir.to_path_buf());
                        }
                    }
                    return Some(cmd_line.to_path_buf());
                }
            }
        }
    }

    None
}

pub fn get_activation_command(
    env: &CondaEnvironment,
    manager: &EnvManager,
    name: Option<String>,
) -> Option<Vec<String>> {
    if env.executable.is_none() {
        return None;
    }
    let conda_exe = manager.executable.to_str().unwrap_or_default().to_string();
    if let Some(name) = name {
        Some(vec![
            conda_exe,
            "run".to_string(),
            "-n".to_string(),
            name,
            "python".to_string(),
        ])
    } else {
        Some(vec![
            conda_exe,
            "run".to_string(),
            "-p".to_string(),
            env.prefix.to_str().unwrap().to_string(),
            "python".to_string(),
        ])
    }
}
