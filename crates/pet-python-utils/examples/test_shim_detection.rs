// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_python_utils::is_pyenv_shim;
use std::path::PathBuf;

fn main() {
    env_logger::init();
    
    let shim_path = PathBuf::from("/tmp/fake_pyenv/shims/python3.10");
    println!("Testing path: {:?}", shim_path);
    println!("Is pyenv shim: {}", is_pyenv_shim(&shim_path));
    
    let regular_path = PathBuf::from("/usr/bin/python3");
    println!("Testing path: {:?}", regular_path);
    println!("Is regular path shim: {}", is_pyenv_shim(&regular_path));
}