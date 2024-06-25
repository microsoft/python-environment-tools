// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use conda_info::CondaInfo;
use env_variables::EnvVariables;
use environment_locations::{get_conda_environment_paths, get_environments};
use environments::{get_conda_environment_info, CondaEnvironment};
use log::{error, info, warn};
use manager::CondaManager;
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator, LocatorResult,
};
use pet_python_utils::env::PythonEnv;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};
use utils::{is_conda_env, is_conda_install};

mod conda_info;
pub mod conda_rc;
pub mod env_variables;
pub mod environment_locations;
pub mod environments;
pub mod manager;
pub mod package;
pub mod utils;

pub trait CondaLocator: Send + Sync {
    fn find_in(&self, path: &Path) -> Option<LocatorResult>;
    fn find_with_conda_executable(&self, conda_executable: Option<PathBuf>) -> Option<()>;
}
pub struct Conda {
    /// Directories where conda environments are found (env_dirs returned from `conda info --json`)
    pub env_dirs: Arc<Mutex<Vec<PathBuf>>>,
    pub environments: Arc<Mutex<HashMap<PathBuf, PythonEnvironment>>>,
    pub managers: Arc<Mutex<HashMap<PathBuf, CondaManager>>>,
    pub env_vars: EnvVariables,
}

impl Conda {
    pub fn from(env: &dyn Environment) -> Conda {
        Conda {
            env_dirs: Arc::new(Mutex::new(vec![])),
            environments: Arc::new(Mutex::new(HashMap::new())),
            managers: Arc::new(Mutex::new(HashMap::new())),
            env_vars: EnvVariables::from(env),
        }
    }
}

impl CondaLocator for Conda {
    fn find_with_conda_executable(&self, conda_executable: Option<PathBuf>) -> Option<()> {
        let info = CondaInfo::from(conda_executable)?;
        // Have we seen these envs, if yes, then nothing to do
        let environments = self.environments.lock().unwrap().clone();
        let mut new_envs = info
            .envs
            .clone()
            .into_iter()
            .filter(|p| !environments.contains_key(p))
            .collect::<Vec<PathBuf>>();
        if new_envs.is_empty() {
            return None;
        }

        // Oh oh, we have new envs, lets find them.
        let manager = CondaManager::from_info(&info.executable, &info)?;
        for path in new_envs.iter() {
            let mgr = manager.clone();
            if let Some(env) = get_conda_environment_info(path, &Some(mgr.clone())) {
                warn!(
                    "Found a conda env {:?} using the conda exe {:?}",
                    env.prefix, info.executable
                );
            }
        }

        // Also keep track of these directories for next time
        let mut env_dirs = self.env_dirs.lock().unwrap();
        env_dirs.append(&mut new_envs);

        // Send telemetry

        Some(())
    }
    fn find_in(&self, conda_dir: &Path) -> Option<LocatorResult> {
        if !is_conda_install(conda_dir) {
            return None;
        }
        if let Some(manager) = CondaManager::from(conda_dir) {
            let conda_dir = manager.conda_dir.clone();
            // Keep track to search again later.
            // Possible we'll find environments in other directories created using this manager
            let mut managers = self.managers.lock().unwrap();
            // Keep track to search again later.
            // Possible we'll find environments in other directories created using this manager
            managers.insert(conda_dir.clone(), manager.clone());
            drop(managers);

            let mut new_environments = vec![];
            info!("ENUMERATE IN {:?}", conda_dir);
            // Find all the environments in the conda install folder. (under `envs` folder)
            for conda_env in
                get_conda_environments(&get_environments(&conda_dir), &manager.clone().into())
            {
                let mut environments = self.environments.lock().unwrap();
                if environments.contains_key(&conda_env.prefix) {
                    continue;
                }
                let env = conda_env
                    .to_python_environment(Some(conda_dir.clone()), Some(manager.to_manager()));
                environments.insert(conda_env.prefix.clone(), env.clone());
                new_environments.push(env);
            }

            return Some(LocatorResult {
                environments: new_environments,
                managers: vec![manager.to_manager()],
            });
        }
        None
    }
}

impl Conda {
    fn get_manager(&self, conda_dir: &Path) -> Option<CondaManager> {
        let mut managers = self.managers.lock().unwrap();
        // If we have a conda install folder, then use that to get the manager.
        if let Some(mgr) = managers.get(conda_dir) {
            return Some(mgr.clone());
        }

        if let Some(manager) = CondaManager::from(conda_dir) {
            managers.insert(conda_dir.into(), manager.clone());
            Some(manager)
        } else {
            None
        }
    }
}

impl Locator for Conda {
    fn get_name(&self) -> &'static str {
        "Conda"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentCategory> {
        vec![PythonEnvironmentCategory::Conda]
    }
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Possible we do not have the prefix, but this exe is in the bin directory and its a conda env or root conda install.
        let mut prefix = env.prefix.clone();
        if prefix.is_none() {
            if let Some(parent_dir) = &env.executable.parent() {
                if is_conda_env(parent_dir) {
                    // This is a conda env (most likely root conda env as the exe is in the same directory (generally on windows))
                    prefix = Some(parent_dir.to_path_buf());
                } else if parent_dir.ends_with("bin") || parent_dir.ends_with("Scripts") {
                    if let Some(parent_dir) = parent_dir.parent() {
                        if is_conda_env(parent_dir) {
                            // This is a conda env
                            prefix = Some(parent_dir.to_path_buf());
                        }
                    }
                }
            }
        }

        if let Some(ref path) = prefix {
            if !is_conda_env(path) {
                return None;
            }

            let mut environments = self.environments.lock().unwrap();

            // Do we already have an env for this.
            if let Some(env) = environments.get(path) {
                return Some(env.clone());
            }
            if let Some(env) = get_conda_environment_info(path, &None) {
                if let Some(conda_dir) = &env.conda_dir {
                    if let Some(manager) = self.get_manager(conda_dir) {
                        let env = env.to_python_environment(
                            Some(conda_dir.clone()),
                            Some(manager.to_manager()),
                        );
                        environments.insert(path.clone(), env.clone());
                        return Some(env);
                    } else {
                        // We will still return the conda env even though we do not have the manager.
                        // This might seem incorrect, however the tool is about discovering environments.
                        // The client can activate this env either using another conda manager or using the activation scripts
                        error!("Unable to find Conda Manager for env (even though we have a conda_dir): {:?}", env);
                        let env = env.to_python_environment(Some(conda_dir.clone()), None);
                        environments.insert(path.clone(), env.clone());
                        return Some(env);
                    }
                } else {
                    // We will still return the conda env even though we do not have the manager.
                    // This might seem incorrect, however the tool is about discovering environments.
                    // The client can activate this env either using another conda manager or using the activation scripts
                    error!("Unable to find Conda Manager for env: {:?}", env);
                    let env = env.to_python_environment(None, None);
                    environments.insert(path.clone(), env.clone());
                    return Some(env);
                }
            }
        }
        None
    }

    fn find(&self, reporter: &dyn Reporter) {
        // if we're calling this again, then clear what ever cache we have.
        let mut environments = self.environments.lock().unwrap();
        environments.clear();
        drop(environments);

        let env_vars = self.env_vars.clone();
        let additional_paths = self.env_dirs.lock().unwrap().clone();
        thread::scope(|s| {
            // 1. Get a list of all know conda environments file paths
            let possible_conda_envs = get_conda_environment_paths(&env_vars, &additional_paths);
            for path in possible_conda_envs {
                s.spawn(move || {
                    // 2. Get the details of the conda environment
                    // This we do not get any details, then its not a conda environment
                    let env = get_conda_environment_info(&path, &None)?;

                    // 3. If we have a conda environment without a conda_dir
                    // Then we will not be able to get the manager.
                    // Either way report this environment
                    if env.conda_dir.is_none(){
                        // We will still return the conda env even though we do not have the manager.
                        // This might seem incorrect, however the tool is about discovering environments.
                        // The client can activate this env either using another conda manager or using the activation scripts
                        error!("Unable to find Conda Manager for the Conda env: {:?}", env);
                        let prefix = env.prefix.clone();
                        let env = env.to_python_environment(None, None);
                        let mut environments = self.environments.lock().unwrap();
                        environments.insert(prefix, env.clone());
                        reporter.report_environment(&env);
                        return None;
                    }

                    // 3. We have a conda environment with a conda_dir (above we handled the case when its not found)
                    // We will try to get the manager for this conda_dir
                    let prefix = env.clone().prefix.clone();

                    {
                        // 3.1 Check if we have already reported this environment.
                        // Closure to quickly release lock
                        let environments = self.environments.lock().unwrap();
                        if environments.contains_key(&env.prefix) {
                            return None;
                        }
                    }


                    // 4 Get the manager for this env.
                    let conda_dir = &env.conda_dir.clone()?;
                    let managers = self.managers.lock().unwrap();
                    let mut manager = managers.get(conda_dir).cloned();
                    drop(managers);

                    if manager.is_none() {
                        // 4.1 Build the manager from the conda dir if we do not have it.
                        if let Some(conda_manager) = CondaManager::from(conda_dir) {
                            reporter.report_manager(&conda_manager.to_manager());
                            let mut managers = self.managers.lock().unwrap();
                            managers.insert(conda_dir.to_path_buf().clone(), conda_manager.clone());
                            manager = Some(conda_manager);
                        }
                    }

                    // 5. Report this env.
                    if let Some(manager) = manager {
                        let env = env.to_python_environment(
                            Some(manager.conda_dir.clone()),
                            Some(manager.to_manager()),
                        );
                        let mut environments = self.environments.lock().unwrap();
                        environments.insert(prefix.clone(), env.clone());
                        reporter.report_environment(&env);
                    } else {
                        // We will still return the conda env even though we do not have the manager.
                        // This might seem incorrect, however the tool is about discovering environments.
                        // The client can activate this env either using another conda manager or using the activation scripts
                        error!("Unable to find Conda Manager for Conda env (even though we have a conda_dir {:?}): Env Details = {:?}", conda_dir, env);
                        let env = env.to_python_environment(Some(conda_dir.clone()), None);
                        let mut environments = self.environments.lock().unwrap();
                        environments.insert(prefix.clone(), env.clone());
                        reporter.report_environment(&env);
                    }
                    Option::<()>::Some(())
                });
            }
        });
    }
}

fn get_conda_environments(
    paths: &Vec<PathBuf>,
    manager: &Option<CondaManager>,
) -> Vec<CondaEnvironment> {
    let mut threads = vec![];
    for path in paths {
        let path = path.clone();
        let mgr = manager.clone();
        threads.push(thread::spawn(move || {
            if let Some(env) = get_conda_environment_info(&path, &mgr) {
                vec![env]
            } else {
                vec![]
            }
        }));
    }

    let mut envs: Vec<CondaEnvironment> = vec![];
    for thread in threads {
        if let Ok(mut result) = thread.join() {
            envs.append(&mut result);
        }
    }
    envs
}
