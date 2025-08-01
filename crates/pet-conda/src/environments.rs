// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    manager::CondaManager,
    package::{CondaPackageInfo, Package},
    utils::{is_conda_env, is_conda_install},
};
use log::{trace, warn};
use pet_core::{
    arch::Architecture,
    manager::EnvManager,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
};
use pet_fs::path::{norm_case, resolve_symlink};
use pet_python_utils::executable::{find_executable, find_executables};
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
        get_conda_environment_info(path, manager)
    }

    pub fn to_python_environment(&self, conda_manager: Option<EnvManager>) -> PythonEnvironment {
        #[allow(unused_assignments)]
        let name = get_conda_env_name(&self.prefix, &self.prefix, &self.conda_dir);

        // This is a root env.
        let builder = PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Conda))
            .executable(self.executable.clone())
            .version(self.version.clone())
            .prefix(Some(self.prefix.clone()))
            .arch(self.arch.clone())
            .symlinks(Some(find_executables(&self.prefix)))
            .name(name.clone())
            .manager(conda_manager);

        builder.build()
    }
}

pub fn get_conda_environment_info(
    env_path: &Path,
    manager: &Option<CondaManager>,
) -> Option<CondaEnvironment> {
    if !is_conda_env(env_path) {
        // Not a conda environment (neither root nor a separate env).
        return None;
    }
    // If we know the conda install folder, then we can use it.
    let mut conda_install_folder = get_conda_installation_used_to_create_conda_env(env_path)
        .or_else(|| manager.clone().and_then(|m| m.conda_dir));

    if let Some(conda_dir) = &conda_install_folder {
        if conda_dir.exists() {
            trace!(
                "Conda install folder {}, found, & will be used for the Conda Env: {}",
                env_path.display(),
                conda_dir.display()
            );
        } else {
            warn!(
                "Conda install folder {}, does not exist, hence will not be used for the Conda Env: {}",
                env_path.display(),
                conda_dir.display()
            );
            conda_install_folder = None;
        }
    } else {
        trace!("Conda install folder not found for {}", env_path.display());
    }

    if let Some(python_binary) = find_executable(env_path) {
        if let Some(package_info) = CondaPackageInfo::from(env_path, &Package::Python) {
            Some(CondaEnvironment {
                prefix: env_path.into(),
                executable: Some(python_binary),
                version: Some(package_info.version),
                conda_dir: conda_install_folder,
                arch: package_info.arch,
            })
        } else {
            // No python in this environment.
            Some(CondaEnvironment {
                prefix: env_path.into(),
                executable: Some(python_binary),
                version: None,
                conda_dir: conda_install_folder,
                arch: None,
            })
        }
    } else {
        // No python in this environment.
        Some(CondaEnvironment {
            prefix: env_path.into(),
            executable: None,
            version: None,
            conda_dir: conda_install_folder,
            arch: None,
        })
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
pub fn get_conda_installation_used_to_create_conda_env(env_path: &Path) -> Option<PathBuf> {
    // If this environment is in a folder named `envs`, then the parent directory of `envs` is the root conda install folder.
    if let Some(parent) = env_path.ancestors().nth(2) {
        if is_conda_install(parent) {
            return Some(parent.to_path_buf());
        }
    }

    // First look for the conda-meta/history file in the environment folder.
    // This could be a conda envirment (not root) but has `conda` installed in it.
    if let Some(line) = get_conda_creation_line_from_history(env_path) {
        // Sample lines
        // # cmd: <conda install directory>\Scripts\conda-script.py create -n samlpe1
        // # cmd: <conda install directory>\Scripts\conda-script.py create -p <full path>
        // # cmd: /Users/donjayamanne/miniconda3/bin/conda create -n conda1
        if let Some(conda_dir) = get_conda_dir_from_cmd(line) {
            if is_conda_install(&conda_dir) {
                return Some(conda_dir);
            } else {
                // Possible this is a directory such as `C:\Users\donja\miniconda3\Scripts`
                // We try to remove `Scripts` or `bin` from the path in the `get_conda_dir_from_cmd`.
                // However if there are other directories such as `condabin` or others we are not aware of, lets try.
                if let Some(conda_dir) = conda_dir.parent() {
                    if is_conda_install(conda_dir) {
                        return Some(conda_dir.into());
                    }
                }
            }
        }
    }

    // Possible the env_path is the root conda install folder.
    if is_conda_install(env_path) {
        Some(env_path.to_path_buf())
    } else {
        None
    }
}

pub fn get_conda_creation_line_from_history(env_path: &Path) -> Option<String> {
    let conda_meta_history = env_path.join("conda-meta").join("history");
    if let Ok(reader) = std::fs::read_to_string(conda_meta_history.clone()) {
        if let Some(line) = reader.lines().map(|l| l.trim()).find(|l| {
            l.to_lowercase().starts_with("# cmd:") && l.to_lowercase().contains(" create -")
        }) {
            trace!(
                "Conda creation line for {:?} is from history file is {:?}",
                env_path,
                line
            );
            return Some(line.into());
        }
    }

    None
}

fn get_conda_env_name(
    env_path: &Path,
    prefix: &Path,
    conda_dir: &Option<PathBuf>,
) -> Option<String> {
    let mut name: Option<String>;
    if is_conda_install(prefix) {
        name = Some("base".to_string());
    } else {
        name = prefix
            .file_name()
            .map(|name| name.to_str().unwrap_or_default().to_string());
    }
    // if the conda install folder is parent of the env folder, then we can use named activation.
    // E.g. conda env is = <conda install>/envs/<env name>
    // Then we can use `<conda install>/bin/conda activate -n <env name>`
    if let Some(conda_dir) = conda_dir {
        if !prefix.starts_with(conda_dir) {
            name = get_conda_env_name_from_history_file(env_path, prefix);
        }
    }

    name
}

/**
 * The conda-meta/history file in conda environments contain the command used to create the conda environment.
 * And example is `# cmd: <conda install directory>\Scripts\conda-script.py create -n sample``
 * And example is `# cmd: conda create -n sample``
 *
 * This function returns the name of the conda environment.
 */
fn get_conda_env_name_from_history_file(env_path: &Path, prefix: &Path) -> Option<String> {
    let name = prefix
        .file_name()
        .map(|name| name.to_str().unwrap_or_default().to_string());

    if let Some(name) = name {
        if let Some(line) = get_conda_creation_line_from_history(env_path) {
            // Sample lines
            // # cmd: <conda install directory>\Scripts\conda-script.py create -n samlpe1
            // # cmd: <conda install directory>\Scripts\conda-script.py create -p <full path>
            // # cmd: /Users/donjayamanne/miniconda3/bin/conda create -n conda1
            if is_conda_env_name_in_cmd(line, &name) {
                return Some(name);
            }
        }
    }
    None
}

fn get_conda_dir_from_cmd(cmd_line: String) -> Option<PathBuf> {
    // Sample lines
    // # cmd: <conda install directory>\Scripts\conda-script.py create -n samlpe1
    // # cmd: <conda install directory>\Scripts\conda-script.py create -p <full path>
    // # cmd: /Users/donjayamanne/miniconda3/bin/conda create -n conda1
    // # cmd_line: "# cmd: /usr/bin/conda create -p ./prefix-envs/.conda1 python=3.12 -y"
    let start_index = cmd_line.to_lowercase().find("# cmd:")? + "# cmd:".len();
    let end_index = cmd_line.to_lowercase().find(" create -")?;
    let conda_exe = PathBuf::from(cmd_line[start_index..end_index].trim().to_string());
    // Sometimes the path can be as follows, where `/usr/bin/conda` could be a symlink.
    // cmd_line: "# cmd: /usr/bin/conda create -p ./prefix-envs/.conda1 python=3.12 -y"
    let conda_exe = resolve_symlink(&conda_exe).unwrap_or(conda_exe);
    if let Some(cmd_line) = conda_exe.parent() {
        if let Some(conda_dir) = cmd_line.file_name() {
            if conda_dir.to_string_lossy().to_lowercase() == "bin"
                || conda_dir.to_string_lossy().to_lowercase() == "scripts"
                || conda_dir.to_string_lossy().to_lowercase() == "condabin"
            {
                if let Some(conda_dir) = cmd_line.parent() {
                    // Ensure the casing of the paths are correct.
                    // Its possible the actual path is in a different case.
                    // The casing in history might not be same as that on disc
                    // We do not want to have duplicates in different cases.
                    // & we'd like to preserve the case of the original path as on disc.
                    return Some(norm_case(conda_dir).to_path_buf());
                }
            }
            // Sometimes we can have paths like
            // # cmd: C:\Users\donja\miniconda3\lib\site-packages\conda\__main__.py create --yes --prefix .conda python=3.9
            // # cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes --prefix .conda python=3.12

            let mut cmd_line = cmd_line.to_path_buf();
            if cmd_line
                .to_str()
                .unwrap_or_default()
                .contains("site-packages")
                && cmd_line.to_str().unwrap_or_default().contains("lib")
            {
                loop {
                    if cmd_line.to_str().unwrap_or_default().contains("lib")
                        && !cmd_line.to_str().unwrap_or_default().ends_with("lib")
                    {
                        let _ = cmd_line.pop();
                    } else {
                        break;
                    }
                }
                if cmd_line.ends_with("lib") {
                    let _ = cmd_line.pop();
                }
            }
            // Ensure the casing of the paths are correct.
            // Its possible the actual path is in a different case.
            // The casing in history might not be same as that on disc
            // We do not want to have duplicates in different cases.
            // & we'd like to preserve the case of the original path as on disc.
            return Some(norm_case(&cmd_line).to_path_buf());
        }
    }
    None
}

fn is_conda_env_name_in_cmd(cmd_line: String, name: &str) -> bool {
    // Sample lines
    // # cmd: <conda install directory>\Scripts\conda-script.py create -n samlpe1
    // # cmd: <conda install directory>\Scripts\conda-script.py create -p <full path>
    // # cmd: /Users/donjayamanne/miniconda3/bin/conda create -n conda1
    // # cmd_line: "# cmd: /usr/bin/conda create -p ./prefix-envs/.conda1 python=3.12 -y"
    // Look for "-n <name>" in the command line
    cmd_line.contains(format!("-n {name}").as_str())
        || cmd_line.contains(format!("--name {name}").as_str())
}

pub fn get_activation_command(
    env: &CondaEnvironment,
    manager: &EnvManager,
    name: Option<String>,
) -> Option<Vec<String>> {
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
            env.prefix.to_str().unwrap_or_default().to_string(),
            "python".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn parse_cmd_line() {
        let line = "# cmd: C:\\Users\\donja\\miniconda3\\lib\\site-packages\\conda\\__main__.py create --yes --prefix .conda python=3.9";
        let conda_dir = get_conda_dir_from_cmd(line.to_string()).unwrap();

        assert_eq!(conda_dir, PathBuf::from("C:\\Users\\donja\\miniconda3"));

        let line =
            "# cmd: C:\\Users\\donja\\miniconda3\\Scripts\\conda-script.py create -n samlpe1";
        let conda_dir = get_conda_dir_from_cmd(line.to_string()).unwrap();

        assert_eq!(conda_dir, PathBuf::from("C:\\Users\\donja\\miniconda3"));

        // From root install folder
        let line = "# cmd: build.py --product miniconda --python 3.9 --installer-type exe --output-dir C:\\ci\\containers\\000029l07m4\\tmp\\build\\dd3144c1\\output-installer/220421/ --standalone C:\\ci\\containers\\000029l07m4\\tmp\\build\\dd3144c1\\mc/standalone_conda/conda.exe";
        let conda_dir = get_conda_dir_from_cmd(line.to_string());

        assert!(conda_dir.is_none());
    }

    #[test]
    #[cfg(unix)]
    fn parse_cmd_line() {
        let line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes --prefix .conda python=3.12";
        let conda_dir = get_conda_dir_from_cmd(line.to_string()).unwrap();

        assert_eq!(
            conda_dir,
            PathBuf::from("/Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3")
        );
    }

    #[test]
    #[cfg(unix)]
    fn verify_conda_env_name() {
        let line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes --name .conda python=3.12";
        assert!(is_conda_env_name_in_cmd(line.to_string(), ".conda"));

        let mut line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes -n .conda python=3.12";
        assert!(is_conda_env_name_in_cmd(line.to_string(), ".conda"));

        line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes --name .conda python=3.12";
        assert!(!is_conda_env_name_in_cmd(line.to_string(), "base"));

        line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes -p .conda python=3.12";
        assert!(!is_conda_env_name_in_cmd(line.to_string(), "base"));

        line = "# cmd: /Users/donjayamanne/.pyenv/versions/mambaforge-22.11.1-3/lib/python3.10/site-packages/conda/__main__.py create --yes -p .conda python=3.12";
        assert!(!is_conda_env_name_in_cmd(line.to_string(), ".conda"));
    }
}
