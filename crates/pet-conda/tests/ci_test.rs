// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use needs_env_var::needs_env_var;

#[cfg(unix)]
#[test]
#[needs_env_var(CI)]
fn conda_ci() {
    use pet_conda::Conda;
    use pet_core::{os_environment::EnvironmentApi, Locator};

    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let result = conda.find();
    println!("SERVER CI Started");
    println!("SERVER CI REsults{:?}", result);
}
    