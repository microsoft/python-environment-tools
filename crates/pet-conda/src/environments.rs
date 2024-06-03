// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use super::{is_conda_env_location, is_conda_install_location, utils::get_conda_package_info};
use crate::{
    messaging::{
        Architecture, EnvManager, PythonEnvironment, PythonEnvironmentBuilder,
        PythonEnvironmentCategory,
    },
    utils::find_python_binary_path,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CondaEnvironment {
    pub env_path: PathBuf,
    pub python_executable_path: Option<PathBuf>,
    pub version: Option<String>,
    pub conda_install_folder: Option<PathBuf>,
    pub arch: Option<Architecture>,
}

impl CondaEnvironment {
    pub fn to_python_environment(
        &self,
        conda_manager: EnvManager,
        conda_manager_dir: &PathBuf,
    ) -> PythonEnvironment {
        #[allow(unused_assignments)]
        let mut name: Option<String> = None;
        if is_conda_install_location(&self.env_path) {
            name = Some("base".to_string());
        } else {
            name = match self.env_path.file_name() {
                Some(name) => Some(name.to_str().unwrap_or_default().to_string()),
                None => None,
            };
        }
        // if the conda install folder is parent of the env folder, then we can use named activation.
        // E.g. conda env is = <conda install>/envs/<env name>
        // Then we can use `<conda install>/bin/conda activate -n <env name>`
        if !self.env_path.starts_with(&conda_manager_dir) {
            name = None;
        }
        // This is a root env.
        let builder = PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Conda)
            .python_executable_path(self.python_executable_path.clone())
            .version(self.version.clone())
            .env_path(Some(self.env_path.clone()))
            .arch(self.arch.clone())
            .env_manager(Some(conda_manager.clone()))
            .python_run_command(get_activation_command(self, &conda_manager, name.clone()));

        if let Some(name) = name {
            builder.name(name).build()
        } else {
            builder.build()
        }
    }
}
pub fn get_conda_environment_info(env_path: &PathBuf) -> Option<CondaEnvironment> {
    if is_conda_env_location(env_path) {
        let conda_install_folder = get_conda_installation_used_to_create_conda_env(env_path);
        let env_path = env_path.clone();
        if let Some(python_binary) = find_python_binary_path(&env_path) {
            if let Some(package_info) = get_conda_package_info(&env_path, "python") {
                return Some(CondaEnvironment {
                    env_path,
                    python_executable_path: Some(python_binary),
                    version: Some(package_info.version),
                    conda_install_folder,
                    arch: package_info.arch,
                });
            } else {
                return Some(CondaEnvironment {
                    env_path,
                    python_executable_path: Some(python_binary),
                    version: None,
                    conda_install_folder,
                    arch: None,
                });
            }
        } else {
            return Some(CondaEnvironment {
                env_path,
                python_executable_path: None,
                version: None,
                conda_install_folder,
                arch: None,
            });
        }
    }

    None
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
    if is_conda_install_location(env_path) {
        return Some(env_path.to_path_buf());
    }

    // If this environment is in a folder named `envs`, then the parent directory of `envs` is the root conda install folder.
    if let Some(parent) = env_path.ancestors().nth(2) {
        if is_conda_install_location(parent) {
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
    if env.python_executable_path.is_none() {
        return None;
    }
    let conda_exe = manager.executable_path.to_str().unwrap().to_string();
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
            env.env_path.to_str().unwrap().to_string(),
            "python".to_string(),
        ])
    }
}
