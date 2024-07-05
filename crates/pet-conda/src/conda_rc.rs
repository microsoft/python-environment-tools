// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::env_variables::EnvVariables;
use log::trace;
use std::{
    fs,
    path::{Path, PathBuf},
};
use yaml_rust2::YamlLoader;

#[derive(Debug)]
pub struct Condarc {
    pub files: Vec<PathBuf>,
    pub env_dirs: Vec<PathBuf>,
}

impl Condarc {
    pub fn from(env_vars: &EnvVariables) -> Option<Condarc> {
        get_conda_conda_rc(env_vars)
    }
    pub fn from_path(path: &Path) -> Option<Condarc> {
        get_conda_conda_rc_from_path(&path.to_path_buf())
    }
}

// Search paths documented here
// https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
// https://github.com/conda/conda/blob/3ae5d7cf6cbe2b0ff9532359456b7244ae1ea5ef/conda/base/constants.py#L28
fn get_conda_rc_search_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    use crate::utils::change_root_of_path;

    let mut search_paths: Vec<PathBuf> = vec![];

    if std::env::consts::OS == "windows" {
        search_paths.append(
            &mut [
                "C:\\ProgramData\\conda\\.condarc",
                "C:\\ProgramData\\conda\\condarc",
                "C:\\ProgramData\\conda\\condarc.d",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        );
    } else {
        search_paths.append(
            &mut [
                "/etc/conda/.condarc",
                "/etc/conda/condarc",
                "/etc/conda/condarc.d",
                "/var/lib/conda/.condarc",
                "/var/lib/conda/condarc",
                "/var/lib/conda/condarc.d",
            ]
            .iter()
            .map(PathBuf::from)
            // This is done only for testing purposes, hacky, but we need some tests with different paths.
            .map(|p| change_root_of_path(&p, &env_vars.root))
            .collect(),
        );
    }
    if let Some(ref conda_root) = env_vars.conda_root {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref xdg_config_home) = env_vars.xdg_config_home {
        search_paths.append(&mut vec![
            PathBuf::from(xdg_config_home.clone()).join(".condarc"),
            PathBuf::from(xdg_config_home.clone()).join("condarc"),
            PathBuf::from(xdg_config_home.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref home) = env_vars.home {
        search_paths.append(&mut vec![
            home.join(".config").join("conda").join(".condarc"),
            home.join(".config").join("conda").join("condarc"),
            home.join(".config").join("conda").join("condarc.d"),
            home.join(".conda").join(".condarc"),
            home.join(".conda").join("condarc"),
            home.join(".conda").join("condarc.d"),
            home.join(".condarc"),
        ]);
    }
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref condarc) = env_vars.condarc {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    search_paths
}

// https://github.com/conda/conda/blob/3ae5d7cf6cbe2b0ff9532359456b7244ae1ea5ef/conda/common/configuration.py#L1315
static POSSIBLE_CONDA_RC_FILES: &[&str] = &[".condarc", "condarc", ".condarc.d"];
static SUPPORTED_EXTENSIONS: &[&str] = &["yaml", "yml"];

/**
 * The .condarc file contains a list of directories where conda environments are created.
 * https://conda.io/projects/conda/en/latest/configuration.html#envs-dirs
 *
 * More info here
 * https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
 * https://github.com/conda/conda/blob/3ae5d7cf6cbe2b0ff9532359456b7244ae1ea5ef/conda/base/constants.py#L28
 */
fn get_conda_conda_rc(env_vars: &EnvVariables) -> Option<Condarc> {
    let mut env_dirs = vec![];
    let mut files = vec![];
    for conda_rc in get_conda_rc_search_paths(env_vars).into_iter() {
        if let Some(ref mut cfg) = get_conda_conda_rc_from_path(&conda_rc) {
            env_dirs.append(&mut cfg.env_dirs);
            files.append(&mut cfg.files);
        }
    }

    if env_dirs.is_empty() && files.is_empty() {
        None
    } else {
        Some(Condarc { env_dirs, files })
    }
}

fn get_conda_conda_rc_from_path(conda_rc: &PathBuf) -> Option<Condarc> {
    let mut env_dirs = vec![];
    let mut files = vec![];
    if conda_rc.is_file() {
        if let Some(ref mut cfg) = parse_conda_rc(conda_rc) {
            env_dirs.append(&mut cfg.env_dirs);
            files.push(conda_rc.clone());
        }
    } else if conda_rc.is_dir() {
        // There can be different types of conda rc files in the directory.
        // .condarc, condarc, .condarc.yml, condarc.yaml, etc.
        // https://github.com/conda/conda/blob/3ae5d7cf6cbe2b0ff9532359456b7244ae1ea5ef/conda/common/configuration.py#L1315
        // https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html
        if let Ok(reader) = fs::read_dir(conda_rc) {
            for path in reader
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file())
            {
                let file_name = path.file_name().unwrap().to_str().unwrap_or_default();
                let extension = path
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default();

                if POSSIBLE_CONDA_RC_FILES.contains(&file_name)
                    || SUPPORTED_EXTENSIONS.contains(&extension)
                    || file_name.contains("condarc")
                {
                    if let Some(ref mut cfg) = parse_conda_rc(&path) {
                        env_dirs.append(&mut cfg.env_dirs);
                        files.push(path);
                    }
                }
            }
        }
    }

    if env_dirs.is_empty() && files.is_empty() {
        None
    } else {
        Some(Condarc { env_dirs, files })
    }
}

fn parse_conda_rc(conda_rc: &Path) -> Option<Condarc> {
    let reader = fs::read_to_string(conda_rc).ok()?;
    trace!("Possible conda_rc: {:?}", conda_rc);
    if let Some(cfg) = parse_conda_rc_contents(&reader, None) {
        Some(Condarc {
            env_dirs: cfg.env_dirs,
            files: vec![conda_rc.to_path_buf()],
        })
    } else {
        Some(Condarc {
            env_dirs: vec![],
            files: vec![conda_rc.to_path_buf()],
        })
    }
}

fn parse_conda_rc_contents(contents: &str, home: Option<PathBuf>) -> Option<Condarc> {
    let mut env_dirs = vec![];

    if let Ok(docs) = YamlLoader::load_from_str(contents) {
        if docs.is_empty() {
            return Some(Condarc {
                env_dirs: vec![],
                files: vec![],
            });
        }
        let doc = &docs[0];
        // Expland variables in some of these
        // https://docs.conda.io/projects/conda/en/4.13.x/user-guide/configuration/use-condarc.html#expansion-of-environment-variables

        if let Some(items) = doc["envs_dirs"].as_vec() {
            for item in items {
                let item_str = item.as_str().unwrap().to_string();
                if item_str.is_empty() {
                    continue;
                }
                let item = PathBuf::from(item_str.trim());
                if item.starts_with("~") {
                    if let Some(ref home) = home {
                        if let Ok(item) = item.strip_prefix("~") {
                            let item = home.join(item);
                            env_dirs.push(item);
                        } else {
                            env_dirs.push(item);
                        }
                    }
                } else {
                    env_dirs.push(item);
                }
            }
        }
        if let Some(items) = doc["envs_path"].as_vec() {
            for item in items {
                let item_str = item.as_str().unwrap().to_string();
                if item_str.is_empty() {
                    continue;
                }
                let item = PathBuf::from(item_str.trim());
                if item.starts_with("~") {
                    if let Some(ref home) = home {
                        if let Ok(item) = item.strip_prefix("~") {
                            let item = home.join(item);
                            env_dirs.push(item);
                        } else {
                            env_dirs.push(item);
                        }
                    }
                } else {
                    env_dirs.push(item);
                }
            }
        }
    }
    Some(Condarc {
        env_dirs,
        files: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_conda_rc() {
        let cfg = r#"
channels:
  - conda-forge
  - nodefaults
channel_priority: strict
envs_dirs:
  - /Users/username/dev/envs # Testing 1,2,3
  - /opt/conda/envs
envs_path:
  - /opt/somep lace/envs
  - ~/dev/envs2 # Testing 1,2,3
"#;

        assert_eq!(
            parse_conda_rc_contents(&cfg, Some(PathBuf::from("/Users/username2")))
                .unwrap()
                .env_dirs,
            [
                "/Users/username/dev/envs",
                "/opt/conda/envs",
                "/opt/somep lace/envs",
                "/Users/username2/dev/envs2"
            ]
            .map(PathBuf::from)
        );

        let cfg = r#"
channels:
  - conda-forge
  - nodefaults
channel_priority: strict
envs_dirs:
  - /Users/username/dev/envs # Testing 1,2,3
  - /opt/conda/envs
"#;

        assert_eq!(
            parse_conda_rc_contents(&cfg, Some(PathBuf::from("/Users/username2")))
                .unwrap()
                .env_dirs,
            ["/Users/username/dev/envs", "/opt/conda/envs",].map(PathBuf::from)
        );

        let cfg = r#"
channels:
  - conda-forge
  - nodefaults
channel_priority: strict
envs_path:
  - /opt/somep lace/envs
  - ~/dev/envs2 # Testing 1,2,3
"#;

        assert_eq!(
            parse_conda_rc_contents(&cfg, Some(PathBuf::from("/Users/username2")))
                .unwrap()
                .env_dirs,
            ["/opt/somep lace/envs", "/Users/username2/dev/envs2"].map(PathBuf::from)
        );

        let cfg = r#"
channels:
  - conda-forge
  - nodefaults
channel_priority: strict
"#;

        assert!(parse_conda_rc_contents(&cfg, Some(PathBuf::from("/Users/username2"))).is_none(),);
    }
}
