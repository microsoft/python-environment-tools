// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::Configuration;
use pet_reporter::{self, stdio};
use std::{collections::BTreeMap, env, time::SystemTime};

pub mod find;
pub mod locators;
pub mod resolve;

pub fn find_and_report_envs_stdio(print_list: bool, print_summary: bool) {
    stdio::initialize_logger(log::LevelFilter::Info);
    let now = SystemTime::now();

    let reporter = stdio::create_reporter(print_list);

    let mut config = Configuration::default();
    if let Ok(cwd) = env::current_dir() {
        config.search_paths = Some(vec![cwd]);
    }
    if print_summary {
        let summary = reporter.get_summary();
        if !summary.managers.is_empty() {
            println!("Managers:");
            println!("---------");
            for (k, v) in summary
                .managers
                .clone()
                .into_iter()
                .map(|(k, v)| (format!("{:?}", k), v))
                .collect::<BTreeMap<String, u16>>()
            {
                println!("{:<20} : {:?}", k, v);
            }
            println!()
        }
        if !summary.environments.is_empty() {
            let total = summary
                .environments
                .clone()
                .iter()
                .fold(0, |total, b| total + b.1);
            println!("Environments ({}):", total);
            println!("------------------");
            for (k, v) in summary
                .environments
                .clone()
                .into_iter()
                .map(|(k, v)| (format!("{:?}", k), v))
                .collect::<BTreeMap<String, u16>>()
            {
                println!("{:<20} : {:?}", k, v);
            }
            println!()
        }
    }

    println!(
        "Refresh completed in {}ms",
        now.elapsed().unwrap().as_millis()
    )
}
