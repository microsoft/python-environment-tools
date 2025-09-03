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
