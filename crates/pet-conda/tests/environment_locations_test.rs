// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[cfg(unix)]
#[test]
fn read_environment_txt() {
    use common::{create_env_variables, resolve_test_path};
    use pet_conda::environment_locations::get_conda_envs_from_environment_txt;
    use std::path::PathBuf;

    let root = resolve_test_path(&["unix", "root_empty"]).into();
    let home = resolve_test_path(&["unix", "user_home_with_environments_txt"]).into();
    let env = create_env_variables(home, root);

    let mut environments = get_conda_envs_from_environment_txt(&env);
    environments.sort();

    let mut expected = vec![
        "/Users/username/miniconda3",
        "/Users/username/miniconda3/envs/xyz",
        "/Users/username/miniconda3/envs/conda1",
        "/Users/username/miniconda3/envs/conda2",
        "/Users/username/miniconda3/envs/conda3",
        "/Users/username/miniconda3/envs/conda4",
        "/Users/username/miniconda3/envs/conda5",
        "/Users/username/miniconda3/envs/conda6",
        "/Users/username/miniconda3/envs/conda7",
        "/Users/username/miniconda3/envs/conda8",
        "/Users/username/.pyenv/versions/miniconda3-latest",
        "/Users/username/.pyenv/versions/miniconda3-latest/envs/myenv",
        "/Users/username/.pyenv/versions/miniforge3-4.10.1-1",
        "/Users/username/.pyenv/versions/anaconda3-2023.03",
        "/Users/username/miniforge3/envs/sample1",
        "/Users/username/temp/conda_work_folder",
        "/Users/username/temp/conda_work_folder_3.12",
        "/Users/username/temp/conda_work_folder__no_python",
        "/Users/username/temp/conda_work_folder_from_root",
        "/Users/username/temp/sample-conda-envs-folder/hello_world",
        "/Users/username/temp/sample-conda-envs-folder2/another",
        "/Users/username/temp/sample-conda-envs-folder2/xyz",
    ]
    .iter()
    .map(PathBuf::from)
    .collect::<Vec<PathBuf>>();
    expected.sort();

    assert_eq!(environments, expected);
}

#[cfg(unix)]
#[test]
fn non_existent_envrionments_txt() {
    use common::{create_env_variables, resolve_test_path};
    use pet_conda::environment_locations::get_conda_envs_from_environment_txt;

    let root = resolve_test_path(&["unix", "root_empty"]).into();
    let home = resolve_test_path(&["unix", "bogus directory"]).into();
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
