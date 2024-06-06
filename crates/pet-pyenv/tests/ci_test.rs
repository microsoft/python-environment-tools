// // Copyright (c) Microsoft Corporation.
// // Licensed under the MIT License.

// use std::path::PathBuf;

// use serde::Deserialize;

// mod common;

// #[cfg(unix)]
// #[cfg_attr(feature = "ci", test)]
// #[allow(dead_code)]
// // We should detect the conda install along with the base env
// fn detect_virtual_env_wrapper() {
//     if cfg!(target_os = "linux") {
//         // Code to execute if OS is Linux
//     } else {
//         // Code to execute if OS is not Linux
//     }
//     use pet_conda::Conda;
//     use pet_core::{
//         manager::EnvManagerType, os_environment::EnvironmentApi,
//         python_environment::PythonEnvironmentCategory, Locator,
//     };
//     use std::path::PathBuf;

//     let env = EnvironmentApi::new();

//     let conda = Conda::from(&env);
//     let result = conda.find().unwrap();

//     assert_eq!(result.managers.len(), 1);

//     let info = get_conda_info();
//     let conda_dir = PathBuf::from(info.conda_prefix.clone());
//     let manager = &result.managers[0];
//     assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
//     assert_eq!(manager.tool, EnvManagerType::Conda);
//     assert_eq!(manager.version, info.conda_version.into());

//     let env = &result
//         .environments
//         .iter()
//         .find(|e| e.name == Some("base".into()))
//         .unwrap();
//     assert_eq!(env.prefix, conda_dir.clone().into());
//     assert_eq!(env.name, Some("base".into()));
//     assert_eq!(env.category, PythonEnvironmentCategory::Conda);
//     assert_eq!(env.executable, Some(conda_dir.join("bin").join("python")));
//     assert_eq!(env.version, Some(get_version(&info.python_version)));

//     assert_eq!(env.manager, Some(manager.clone()));
// }
