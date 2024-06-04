// // Copyright (c) Microsoft Corporation. All rights reserved.
// // Licensed under the MIT License.

// mod environment_locations;
// mod environments;
// mod lib;
// mod manager;
// mod utils;

// use crate::known::Environment;
// use crate::locator::Locator;
// use crate::locator::LocatorResult;
// use crate::messaging::EnvManager;
// use crate::messaging::PythonEnvironment;
// use crate::messaging::PythonEnvironmentBuilder;
// use crate::messaging::PythonEnvironmentCategory;
// use crate::utils::PythonEnv;
// use environment_locations::get_conda_environment_paths;
// #[allow(unused_imports)]
// pub use environment_locations::get_conda_environment_paths_from_conda_rc;
// use environment_locations::get_environments_in_conda_dir;
// use environments::get_activation_command;
// use environments::get_conda_environment_info;
// use environments::get_conda_installation_used_to_create_conda_env;
// use environments::CondaEnvironment;
// use log::error;
// use manager::get_conda_manager;
// use manager::CondaManager;
// use std::collections::HashMap;
// use std::collections::HashSet;
// use std::path::{Path, PathBuf};
// use std::sync::Mutex;
// #[allow(unused_imports)]
// pub use utils::get_conda_package_info;
// pub use utils::is_conda_env_location;
// pub use utils::is_conda_install_location;
// #[allow(unused_imports)]
// pub use utils::CondaPackage;

// fn get_conda_manager_from_env(env_path: &Path) -> Option<CondaManager> {
//     // Lets see if we've been given the base env.
//     if let Some(manager) = get_conda_manager(env_path) {
//         return Some(manager);
//     }

//     // Possible we've been given an env thats in the `<conda insta..>/envs` folder.
//     if let Some(parent) = env_path.parent() {
//         if parent.file_name().unwrap_or_default() == "envs" {
//             return get_conda_manager(parent.parent()?);
//         }
//     }

//     // We've been given an env thats been created using the -p flag.
//     // Get the conda install folder from the history file.
//     if let Some(conda_install_folder) = get_conda_installation_used_to_create_conda_env(env_path) {
//         return get_conda_manager(&conda_install_folder);
//     }
//     None
// }

// fn get_known_conda_envs_from_various_locations(
//     environment: &dyn Environment,
// ) -> Vec<CondaEnvironment> {
//     get_conda_environment_paths(environment)
//         .iter()
//         .map(|path| get_conda_environment_info(&path))
//         .filter(Option::is_some)
//         .map(Option::unwrap)
//         .into_iter()
//         .collect::<Vec<CondaEnvironment>>()
// }

// pub struct Conda<'a> {
//     pub environments: Mutex<HashMap<PathBuf, PythonEnvironment>>,
//     pub managers: Mutex<HashMap<PathBuf, CondaManager>>,
//     pub environment: &'a dyn Environment,
// }

// pub trait CondaLocator {
//     fn find_in(&mut self, possible_conda_folder: &Path) -> Option<LocatorResult>;
// }

// impl Conda<'_> {
//     pub fn with<'a>(environment: &'a impl Environment) -> Conda {
//         Conda {
//             environment,
//             environments: Mutex::new(HashMap::new()),
//             managers: Mutex::new(HashMap::new()),
//         }
//     }
// }

// impl CondaLocator for Conda<'_> {
//     fn find_in(&mut self, conda_dir: &Path) -> Option<LocatorResult> {
//         if !is_conda_install_location(conda_dir) {
//             return None;
//         }
//         if let Some(manager) = get_conda_manager(&conda_dir) {
//             let mut managers = self.managers.lock().unwrap();
//             let mut environments = self.environments.lock().unwrap();

//             // Keep track to search again later.
//             // Possible we'll find environments in other directories created using this manager
//             managers.insert(conda_dir.to_path_buf(), manager.clone());

//             let mut new_environments = vec![];

//             // Find all the environments in the conda install folder. (under `envs` folder)
//             get_environments_in_conda_dir(conda_dir)
//                 .iter()
//                 .map(|path| get_conda_environment_info(path))
//                 .filter(Option::is_some)
//                 .map(Option::unwrap)
//                 .for_each(|env| {
//                     let env = env.to_python_environment(manager.to_manager(), &manager.conda_dir);
//                     if let Some(path) = env.env_path.clone() {
//                         if environments.contains_key(&path) {
//                             return;
//                         }
//                         environments.insert(path, env.clone());
//                         new_environments.push(env);
//                     }
//                 });

//             return Some(LocatorResult {
//                 environments: new_environments,
//                 managers: vec![manager.to_manager()],
//             });
//         }
//         return None;
//     }
// }

// impl Locator for Conda<'_> {
//     fn resolve(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
//         if let Some(ref path) = env.path {
//             let mut managers = self.managers.lock().unwrap();
//             let mut environments = self.environments.lock().unwrap();

//             // Do we already have an env for this.
//             if let Some(env) = environments.get(path) {
//                 return Some(env.clone());
//             }
//             if let Some(env) = get_conda_environment_info(path) {
//                 if let Some(conda_dir) = &env.conda_install_folder {
//                     // Use existing manager if we have one.
//                     if let Some(manager) = managers.get(conda_dir) {
//                         let env = env.to_python_environment(manager.to_manager(), &conda_dir);
//                         environments.insert(path.clone(), env.clone());
//                         return Some(env);
//                     }

//                     if let Some(manager) = get_conda_manager(conda_dir) {
//                         let env = env.to_python_environment(manager.to_manager(), &conda_dir);
//                         managers.insert(path.clone(), manager.clone());
//                         environments.insert(path.clone(), env.clone());
//                         return Some(env);
//                     }
//                 } else {
//                     error!(
//                         "Unable to find conda Install folder conda install folder env: {:?}",
//                         env
//                     );
//                 }
//             }
//         }
//         None
//     }

//     fn find(&mut self) -> Option<LocatorResult> {
//         let mut managers = self.managers.lock().unwrap();
//         let mut environments = self.environments.lock().unwrap();

//         let mut discovered_environments: Vec<PythonEnvironment> = vec![];
//         // 1. Get a list of all know conda environments
//         let known_conda_envs = get_known_conda_envs_from_various_locations(self.environment);

//         // 2. Go through all conda dirs and build the conda managers.
//         for env in &known_conda_envs {
//             if let Some(conda_dir) = &env.conda_install_folder {
//                 if managers.contains_key(conda_dir) {
//                     continue;
//                 }
//                 if let Some(manager) = get_conda_manager(&conda_dir) {
//                     managers.insert(conda_dir.clone(), manager);
//                 }
//             }
//         }

//         fn get_manager(
//             known_env: &CondaEnvironment,
//             discovered_managers: &mut HashMap<PathBuf, CondaManager>,
//         ) -> Option<CondaManager> {
//             if let Some(ref path) = known_env.conda_install_folder {
//                 return discovered_managers.get(path).cloned();
//             }
//             // If we have a conda install folder, then use that to get the manager.
//             if let Some(ref conda_dir) = known_env.conda_install_folder {
//                 if let Some(mgr) = discovered_managers.get(conda_dir) {
//                     return Some(mgr.clone());
//                 }
//                 if let Some(manager) = get_conda_manager(&conda_dir) {
//                     discovered_managers.insert(conda_dir.clone(), manager.clone());
//                     return Some(manager);
//                 }

//                 // We could not find the manager, this is an error.
//                 error!(
//                     "Manager not found for conda env: {:?}, known managers include {:?}",
//                     known_env,
//                     discovered_managers.values()
//                 );
//             }
//             // If we do not have the conda install folder, then use the env path to get the manager.
//             if let Some(mgr) = discovered_managers.values().next() {
//                 return Some(mgr.clone());
//             } else {
//                 error!("No conda manager, hence unable to report any environment");
//                 return None;
//             }
//         }

//         // 5. Go through each environment we know of and build the python environments.
//         for known_env in &known_conda_envs {
//             // We should not hit this condition, see above.
//             if let Some(manager) = get_manager(known_env, &mut managers) {
//                 let env = known_env.to_python_environment(manager.to_manager(), &manager.conda_dir);
//                 environments.insert(known_env.env_path.clone(), env.clone());
//                 discovered_environments.push(env);
//             }
//         }

//         if managers.is_empty() && discovered_environments.is_empty() {
//             return None;
//         }

//         Some(LocatorResult {
//             managers: managers
//                 .values()
//                 .into_iter()
//                 .map(|m| m.to_manager())
//                 .collect::<Vec<EnvManager>>(),
//             environments: discovered_environments,
//         })
//     }
// }
