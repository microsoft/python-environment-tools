// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use common::{create_test_environment, resolve_test_path};
use pet_conda::environment_locations::{
    get_conda_envs_from_environment_txt, get_environments_in_conda_dir, get_known_conda_locations,
};
use std::{collections::HashMap, path::PathBuf};

#[test]
fn read_environment_txt() {
    let root = resolve_test_path(&["unix", "root_empty"]).into();
    let home = resolve_test_path(&["unix", "user_home_with_environments_txt"]).into();
    let env = create_test_environment(root, home, HashMap::new(), vec![]);

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

#[test]
fn non_existent_envrionments_txt() {
    let root = resolve_test_path(&["unix", "root_empty"]).into();
    let home = resolve_test_path(&["unix", "bogus directory"]).into();
    let env = create_test_environment(root, home, HashMap::new(), vec![]);

    let environments = get_conda_envs_from_environment_txt(&env);

    assert_eq!(environments.len(), 0);
}

#[test]
fn known_install_locations() {
    let root = resolve_test_path(&["unix", "root_empty"]).into();
    let home = resolve_test_path(&["unix", "user_home"]).into();
    let env = create_test_environment(root, home, HashMap::new(), vec![]);

    let mut locations = get_known_conda_locations(&env);
    locations.sort();

    let mut expected = [
        vec![
            "/opt/anaconda3/bin",
            "/opt/miniconda3/bin",
            "/usr/local/anaconda3/bin",
            "/usr/local/miniconda3/bin",
            "/usr/anaconda3/bin",
            "/usr/miniconda3/bin",
            "/home/anaconda3/bin",
            "/home/miniconda3/bin",
            "/anaconda3/bin",
            "/miniconda3/bin",
        ]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>(),
        vec![
            resolve_test_path(&["unix", "user_home", "anaconda3", "bin"]),
            resolve_test_path(&["unix", "user_home", "miniconda3", "bin"]),
        ],
    ]
    .concat();
    expected.sort();

    assert_eq!(locations, expected);
}

#[test]
fn list_conda_envs_in_install_location() {
    let path = resolve_test_path(&["unix", "anaconda3-2023.03"]);

    let mut locations = get_environments_in_conda_dir(&path);
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

// #[test]
// fn get_conda_environment_paths_test() {
//     let now = SystemTime::now();

//     let env = EnvironmentApi {};
//     let envs = get_conda_environment_paths(&env);
//     println!("{:?}", envs);
//     println!("{:?}", envs);
//     println!("{:?}", envs);
//     println!("{:?}", envs);
//     println!("{:?}", envs);
//     match now.elapsed() {
//         Ok(elapsed) => {
//             println!("Native Locator took {} milliseconds.", elapsed.as_millis());
//             println!("Native Locator took {} milliseconds.", elapsed.as_millis());
//             println!("Native Locator took {} milliseconds.", elapsed.as_millis());
//         }
//         Err(e) => {
//             log::error!("Error getting elapsed time: {:?}", e);
//         }
//     }
// }
