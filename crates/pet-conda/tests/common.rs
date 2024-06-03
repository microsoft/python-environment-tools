// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_conda::utils::CondaEnvironmentVariables;
use std::path::PathBuf;

#[allow(dead_code)]
pub fn resolve_test_path(paths: &[&str]) -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");

    paths.iter().for_each(|p| root.push(p));

    root
}

#[allow(dead_code)]
pub fn create_env_variables(home: PathBuf, root: PathBuf) -> CondaEnvironmentVariables {
    CondaEnvironmentVariables {
        home: Some(home),
        root: Some(root),
        allusersprofile: None,
        conda_prefix: None,
        conda_root: None,
        condarc: None,
        homedrive: None,
        known_global_search_locations: vec![],
        path: None,
        programdata: None,
        userprofile: None,
        xdg_config_home: None,
    }
}
