// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{collections::HashSet, path::PathBuf};

use log::warn;
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    telemetry::{missing_conda_info::MissingCondaEnvironments, TelemetryEvent},
};

use crate::{
    conda_info::CondaInfo, conda_rc::Condarc, env_variables::EnvVariables,
    environments::get_conda_environment_info, manager::CondaManager, utils::is_conda_install,
};

pub fn report_missing_envs(
    reporter: &dyn Reporter,
    env_vars: &EnvVariables,
    possibly_missing_envs: &[PathBuf],
    known_envs: &[PythonEnvironment],
    conda_info: &CondaInfo,
    user_provided_conda_exe: bool,
) -> Option<()> {
    let missing_envs = log_and_find_missing_envs(possibly_missing_envs, known_envs, conda_info)?;
    let known_conda_rcs = get_all_known_conda_rc(env_vars, known_envs);
    let conda_manager_not_found = !known_envs
        .iter()
        .any(|e| e.kind == Some(PythonEnvironmentKind::Conda) && e.manager.is_some());
    let mut discovered_conda_rcs: HashSet<_> = known_conda_rcs
        .iter()
        .flat_map(|rc| rc.files.clone().into_iter())
        .collect();
    let mut discovered_env_dirs: HashSet<_> = known_conda_rcs
        .iter()
        .flat_map(|rc| rc.env_dirs.iter())
        .collect();
    let known_env_prefixes: HashSet<_> =
        known_envs.iter().filter_map(|e| e.prefix.clone()).collect();

    let mut root_prefix_not_found = false;
    let mut conda_prefix_not_found = false;
    if let Some(prefix) = conda_info.root_prefix.as_ref() {
        if !known_env_prefixes.contains(prefix) {
            warn!("Root prefix {:?} not found", prefix);
            root_prefix_not_found = true;
        }
    }
    if let Some(prefix) = conda_info.conda_prefix.as_ref() {
        if !known_env_prefixes.contains(prefix) {
            warn!("Conda prefix {:?} not found", prefix);
            conda_prefix_not_found = true;
        }
    }

    let (sys_conda_rc_not_found, missing_from_sys_rc_env_dirs, missing_env_dirs_from_sys_rc) =
        count_missing_envs(
            &mut discovered_conda_rcs,
            &mut discovered_env_dirs,
            &missing_envs,
            &conda_info
                .sys_rc_path
                .clone()
                .map(|x| [x])
                .unwrap_or_default(),
            "sys",
        );

    let (user_conda_rc_not_found, missing_from_user_rc_env_dirs, missing_env_dirs_from_user_rc) =
        count_missing_envs(
            &mut discovered_conda_rcs,
            &mut discovered_env_dirs,
            &missing_envs,
            &conda_info
                .user_rc_path
                .clone()
                .map(|x| [x])
                .unwrap_or_default(),
            "user",
        );

    let (other_conda_rc_not_found, missing_from_other_rc_env_dirs, missing_env_dirs_from_other_rc) =
        count_missing_envs(
            &mut discovered_conda_rcs,
            &mut discovered_env_dirs,
            &missing_envs,
            &conda_info.config_files,
            "other",
        );

    let missing_info = MissingCondaEnvironments {
        missing: missing_envs.len() as u16,
        user_provided_conda_exe,
        root_prefix_not_found,
        conda_prefix_not_found,
        conda_manager_not_found,
        sys_rc_not_found: if sys_conda_rc_not_found > 0 {
            Some(true)
        } else {
            None
        },
        user_rc_not_found: if user_conda_rc_not_found > 0 {
            Some(true)
        } else {
            None
        },
        other_rc_not_found: if other_conda_rc_not_found > 0 {
            Some(other_conda_rc_not_found)
        } else {
            None
        },
        missing_env_dirs_from_sys_rc: if missing_env_dirs_from_sys_rc > 0 {
            Some(missing_env_dirs_from_sys_rc)
        } else {
            None
        },
        missing_env_dirs_from_user_rc: if missing_env_dirs_from_user_rc > 0 {
            Some(missing_env_dirs_from_user_rc)
        } else {
            None
        },
        missing_env_dirs_from_other_rc: if missing_env_dirs_from_other_rc > 0 {
            Some(missing_env_dirs_from_other_rc)
        } else {
            None
        },
        missing_from_sys_rc_env_dirs: if missing_from_sys_rc_env_dirs > 0 {
            Some(missing_from_sys_rc_env_dirs)
        } else {
            None
        },
        missing_from_user_rc_env_dirs: if missing_from_user_rc_env_dirs > 0 {
            Some(missing_from_user_rc_env_dirs)
        } else {
            None
        },
        missing_from_other_rc_env_dirs: if missing_from_other_rc_env_dirs > 0 {
            Some(missing_from_other_rc_env_dirs)
        } else {
            None
        },
    };

    reporter.report_telemetry(&TelemetryEvent::MissingCondaEnvironments(missing_info));
    Some(())
}

fn log_and_find_missing_envs(
    possibly_missing_envs: &[PathBuf],
    known_envs: &[PythonEnvironment],
    conda_info: &CondaInfo,
) -> Option<Vec<PathBuf>> {
    let mut missing_envs = possibly_missing_envs.to_vec();
    if missing_envs.is_empty() {
        return None;
    }

    let known_env_prefixes = known_envs
        .iter()
        .filter_map(|e| e.prefix.clone())
        .collect::<Vec<_>>();

    // Oh oh, we have new envs, lets see what they are.
    let manager = CondaManager::from_info(&conda_info.executable, conda_info)?;
    for path in missing_envs
        .clone()
        .iter()
        .filter(|p| !known_env_prefixes.contains(p))
    {
        let mgr = manager.clone();
        if let Some(env) = get_conda_environment_info(path, &Some(mgr.clone())) {
            warn!(
                "Failed to find conda env {:?} without spawning conda {:?}",
                env.prefix, conda_info.executable
            );
        } else {
            missing_envs.retain(|p| p != path);
        }
    }

    if missing_envs.is_empty() {
        None
    } else {
        Some(missing_envs)
    }
}

fn get_all_known_conda_rc(
    env_vars: &EnvVariables,
    known_envs: &[PythonEnvironment],
) -> Vec<Condarc> {
    let mut conda_rcs = vec![];
    if let Some(rc) = Condarc::from(env_vars) {
        conda_rcs.push(rc);
    }
    for env in known_envs.iter() {
        if let Some(prefix) = env.prefix.as_ref() {
            if !is_conda_install(prefix) {
                continue;
            }
            if let Some(rc) = Condarc::from_path(prefix) {
                conda_rcs.push(rc);
            }
        }
    }
    conda_rcs
}

fn count_missing_envs(
    discovered_conda_rcs: &mut HashSet<PathBuf>,
    discovered_env_dirs: &mut HashSet<&PathBuf>,
    missing_envs: &[PathBuf],
    config_files: &[PathBuf],
    config_type: &str,
) -> (u16, u16, u16) {
    let mut conda_rc_not_found = 0;
    let mut missing_from_rc_env_dirs = 0;
    let mut missing_env_dirs_from_rc = 0;

    for rc in config_files.iter() {
        // We are not interested in the rc if it does not exist.
        if !rc.exists() {
            continue;
        }

        if !discovered_conda_rcs.contains(rc) {
            discovered_conda_rcs.insert(rc.clone());
            conda_rc_not_found += 1;
            warn!("{} Conda condarc not found: {:?}", config_type, rc);

            if let Some(cfg) = Condarc::from_path(rc) {
                for env_dir in cfg.env_dirs.iter().filter(|d| d.exists()) {
                    if !discovered_env_dirs.contains(env_dir) {
                        missing_env_dirs_from_rc += 1;
                        warn!(
                            "Environment dir {:?} is missing from {} rc env dirs",
                            env_dir, config_type
                        );
                    }
                    for env in missing_envs.iter() {
                        if env.starts_with(env_dir) {
                            missing_from_rc_env_dirs += 1;
                            warn!(
                                "Environment {:?} is missing from {} rc env dirs",
                                env, config_type
                            );
                        }
                    }
                }
            }
        }
    }

    (
        conda_rc_not_found,
        missing_from_rc_env_dirs,
        missing_env_dirs_from_rc,
    )
}
