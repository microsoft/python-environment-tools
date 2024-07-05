// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use find::find_and_report_envs;
use locators::create_locators;
use pet_conda::Conda;
use pet_core::{os_environment::EnvironmentApi, Configuration};
use pet_reporter::{self, cache::CacheReporter, stdio};
use std::{collections::BTreeMap, env, sync::Arc, time::SystemTime};

pub mod find;
pub mod locators;
pub mod resolve;

pub fn find_and_report_envs_stdio(print_list: bool, print_summary: bool, verbose: bool) {
    stdio::initialize_logger(if verbose {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    });
    let now = SystemTime::now();

    let stdio_reporter = Arc::new(stdio::create_reporter(print_list));
    let reporter = CacheReporter::new(stdio_reporter.clone());
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));

    let mut config = Configuration::default();
    if let Ok(cwd) = env::current_dir() {
        config.project_directories = Some(vec![cwd]);
    }
    let locators = create_locators(conda_locator.clone(), &environment);
    for locator in locators.iter() {
        locator.configure(&config);
    }

    let summary = find_and_report_envs(&reporter, config, &locators, &environment);

    if print_summary {
        let summary = summary.lock().unwrap();
        println!();
        println!("Breakdown by each locator:");
        println!("--------------------------");
        for locator in summary.find_locators_times.iter() {
            println!("{:<20} : {:?}", locator.0, locator.1);
        }
        println!();

        println!("Breakdown for finding Environments:");
        println!("-----------------------------------");
        println!(
            "{:<20} : {:?}",
            "Using locators", summary.find_locators_time
        );
        println!("{:<20} : {:?}", "PATH Variable", summary.find_path_time);
        println!(
            "{:<20} : {:?}",
            "Global virtual envs", summary.find_global_virtual_envs_time
        );
        println!(
            "{:<20} : {:?}",
            "Custom search paths", summary.find_search_paths_time
        );
        println!();
        let summary = stdio_reporter.get_summary();
        if !summary.managers.is_empty() {
            println!("Managers:");
            println!("---------");
            for (k, v) in summary
                .managers
                .clone()
                .into_iter()
                .map(|(k, v)| (format!("{k:?}"), v))
                .collect::<BTreeMap<String, u16>>()
            {
                println!("{k:<20} : {v:?}");
            }
            println!()
        }
        if !summary.environments.is_empty() {
            let total = summary
                .environments
                .clone()
                .iter()
                .fold(0, |total, b| total + b.1);
            println!("Environments ({total}):");
            println!("------------------");
            for (k, v) in summary
                .environments
                .clone()
                .into_iter()
                .map(|(k, v)| {
                    (
                        k.map(|v| format!("{:?}", v))
                            .unwrap_or("Unknown".to_string()),
                        v,
                    )
                })
                .collect::<BTreeMap<String, u16>>()
            {
                println!("{k:<20} : {v:?}");
            }
            println!()
        }
    }

    println!(
        "Refresh completed in {}ms",
        now.elapsed().unwrap().as_millis()
    )
}
