// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use common::resolve_test_path;
use pet_conda::manager::CondaManager;

#[cfg(unix)]
#[test]
fn finds_manager_from_root_env() {
    let path = resolve_test_path(&["unix", "anaconda3-2023.03"]);

    let manager = CondaManager::from(&path).unwrap();

    assert_eq!(manager.executable, path.join("bin").join("conda"));
    assert_eq!(manager.version, Some("23.1.0".into()));
}

#[cfg(unix)]
#[test]
fn finds_manager_from_root_within_an_env() {
    let conda_dir = resolve_test_path(&["unix", "anaconda3-2023.03"]);
    let path = resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]);

    let manager = CondaManager::from(&path).unwrap();

    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.version, Some("23.1.0".into()));

    // Try a conda env without Python
    let path = resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "without_python"]);

    let manager = CondaManager::from(&path).unwrap();

    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.version, Some("23.1.0".into()));
}

#[cfg(unix)]
#[test]
fn does_not_find_conda_env_for_bogus_dirs() {
    let path = resolve_test_path(&["unix", "bogus_directory"]);

    assert_eq!(CondaManager::from(&path).is_none(), true);
}
