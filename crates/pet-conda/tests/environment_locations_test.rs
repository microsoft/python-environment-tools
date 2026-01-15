// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[cfg(unix)]
#[test]
fn non_existent_envrionments_txt() {
    use common::{create_env_variables, resolve_test_path};
    use pet_conda::environment_locations::get_conda_envs_from_environment_txt;

    let root = resolve_test_path(&["unix", "root_empty"]);
    let home = resolve_test_path(&["unix", "bogus directory"]);
    let env = create_env_variables(home, root);

    let environments = get_conda_envs_from_environment_txt(&env);

    assert!(environments.is_empty());
}

#[cfg(unix)]
#[test]
fn list_conda_envs_in_install_location() {
    use common::resolve_test_path;
    use pet_conda::environment_locations::get_environments;

    let path = resolve_test_path(&["unix", "anaconda3-2023.03"]);

    let mut locations = get_environments(&path);
    locations.sort();

    assert_eq!(
        locations,
        vec![
            resolve_test_path(&["unix", "anaconda3-2023.03"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "myenv"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "without_python"]),
        ]
    );
}

/// Test that when get_environments is called with a child environment under the `envs` folder,
/// it also discovers the parent conda install (base environment) and all sibling environments.
/// This is the fix for https://github.com/microsoft/python-environment-tools/issues/236
/// where the base conda environment wasn't discovered when only child envs were listed
/// in environments.txt (e.g., from Homebrew Cask installs like /opt/homebrew/Caskroom/miniforge/base).
#[cfg(unix)]
#[test]
fn list_conda_envs_discovers_base_from_child_env() {
    use common::resolve_test_path;
    use pet_conda::environment_locations::get_environments;

    // Call get_environments with a child environment path (not the install directory)
    let child_env_path = resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "myenv"]);

    let mut locations = get_environments(&child_env_path);
    locations.sort();

    // Should discover not only the child env, but also the base env (conda install dir)
    // and all sibling environments
    assert_eq!(
        locations,
        vec![
            resolve_test_path(&["unix", "anaconda3-2023.03"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "myenv"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "without_python"]),
        ]
    );
}

/// Test that get_environments works correctly with an env_python_3 child environment
/// (another sibling to verify the fix works for any child env under envs folder).
#[cfg(unix)]
#[test]
fn list_conda_envs_discovers_base_from_another_child_env() {
    use common::resolve_test_path;
    use pet_conda::environment_locations::get_environments;

    // Call get_environments with a different child environment path
    let child_env_path = resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]);

    let mut locations = get_environments(&child_env_path);
    locations.sort();

    // Should discover the base env and all sibling environments
    assert_eq!(
        locations,
        vec![
            resolve_test_path(&["unix", "anaconda3-2023.03"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "myenv"]),
            resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "without_python"]),
        ]
    );
}
