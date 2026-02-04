// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[cfg(unix)]
#[test]
fn finds_manager_from_root_env() {
    use common::resolve_test_path;
    use pet_conda::manager::CondaManager;

    let path = resolve_test_path(&["unix", "anaconda3-2023.03"]);

    let manager = CondaManager::from(&path).unwrap();

    assert_eq!(manager.executable, path.join("bin").join("conda"));
    assert_eq!(manager.version, Some("23.1.0".into()));
}

#[cfg(unix)]
#[test]
fn finds_manager_from_root_within_an_env() {
    use common::resolve_test_path;
    use pet_conda::manager::CondaManager;

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
    use common::resolve_test_path;
    use pet_conda::manager::CondaManager;

    let path = resolve_test_path(&["unix", "bogus_directory"]);

    assert!(CondaManager::from(&path).is_none());
}

/// Test that find_conda_binary finds conda from the PATH environment variable.
/// This is important for discovering conda installations on mapped drives and
/// other non-standard locations (fixes https://github.com/microsoft/python-environment-tools/issues/194).
#[cfg(unix)]
#[test]
fn finds_conda_binary_from_path() {
    use common::{create_test_environment, resolve_test_path};
    use pet_conda::env_variables::EnvVariables;
    use pet_conda::manager::find_conda_binary;
    use std::collections::HashMap;

    let anaconda_bin = resolve_test_path(&["unix", "anaconda3-2023.03", "bin"]);
    let path_value = anaconda_bin.to_string_lossy().to_string();

    let mut vars = HashMap::new();
    vars.insert("PATH".to_string(), path_value);

    let env = create_test_environment(vars, None, vec![], None);
    let env_vars = EnvVariables::from(&env);

    let conda_binary = find_conda_binary(&env_vars);

    assert!(conda_binary.is_some());
    assert_eq!(
        conda_binary.unwrap(),
        resolve_test_path(&["unix", "anaconda3-2023.03", "bin", "conda"])
    );
}

/// Test that find_conda_binary also works when conda is in the condabin directory
/// (common on Windows with Miniforge/Anaconda where condabin is added to PATH).
#[cfg(unix)]
#[test]
fn finds_conda_binary_from_condabin_path() {
    use common::{create_test_environment, resolve_test_path};
    use pet_conda::env_variables::EnvVariables;
    use pet_conda::manager::find_conda_binary;
    use std::collections::HashMap;

    let anaconda_condabin = resolve_test_path(&["unix", "anaconda3-2023.03", "condabin"]);
    let path_value = anaconda_condabin.to_string_lossy().to_string();

    let mut vars = HashMap::new();
    vars.insert("PATH".to_string(), path_value);

    let env = create_test_environment(vars, None, vec![], None);
    let env_vars = EnvVariables::from(&env);

    let conda_binary = find_conda_binary(&env_vars);

    assert!(conda_binary.is_some());
    assert_eq!(
        conda_binary.unwrap(),
        resolve_test_path(&["unix", "anaconda3-2023.03", "condabin", "conda"])
    );
}

/// Test that find_conda_binary returns None when conda is not on PATH.
#[cfg(unix)]
#[test]
fn does_not_find_conda_binary_when_not_on_path() {
    use common::{create_test_environment, resolve_test_path};
    use pet_conda::env_variables::EnvVariables;
    use pet_conda::manager::find_conda_binary;
    use std::collections::HashMap;

    // Use a path that doesn't have conda
    let some_other_path = resolve_test_path(&["unix", "bogus_directory"]);
    let path_value = some_other_path.to_string_lossy().to_string();

    let mut vars = HashMap::new();
    vars.insert("PATH".to_string(), path_value);

    let env = create_test_environment(vars, None, vec![], None);
    let env_vars = EnvVariables::from(&env);

    let conda_binary = find_conda_binary(&env_vars);

    assert!(conda_binary.is_none());
}
