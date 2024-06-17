// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use locators::Configuration;
use pet_conda::Conda;
use pet_core::os_environment::EnvironmentApi;
use pet_reporter::{self, stdio};
use std::{env, sync::Arc, time::SystemTime};

pub mod locators;

pub fn find_and_report_envs_stdio() {
    stdio::initialize_logger(log::LevelFilter::Info);
    let now = SystemTime::now();

    let reporter = stdio::create_reporter();
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));

    let mut config = Configuration::default();
    if let Ok(cwd) = env::current_dir() {
        config.search_paths = Some(vec![cwd]);
    }
    locators::find_and_report_envs(&reporter, conda_locator, config);
    println!(
        "Refresh completed in {}ms",
        now.elapsed().unwrap().as_millis()
    )
}
