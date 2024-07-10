// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use find::find_and_report_envs;
use find::identify_python_executables_using_locators;
use find::SearchScope;
use locators::create_locators;
use log::warn;
use pet_conda::Conda;
use pet_conda::CondaLocator;
use pet_core::os_environment::Environment;
use pet_core::Locator;
use pet_core::{os_environment::EnvironmentApi, reporter::Reporter, Configuration};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_poetry::Poetry;
use pet_poetry::PoetryLocator;
use pet_reporter::collect;
use pet_reporter::{self, cache::CacheReporter, stdio};
use resolve::resolve_environment;
use std::path::PathBuf;
use std::{collections::BTreeMap, env, sync::Arc, time::SystemTime};

pub mod find;
pub mod locators;
pub mod resolve;

#[derive(Debug, Clone)]
pub struct FindOptions {
    pub print_list: bool,
    pub print_summary: bool,
    pub verbose: bool,
    pub report_missing: bool,
    pub workspace_dirs: Option<Vec<PathBuf>>,
    pub workspace_only: bool,
    pub global_only: bool,
}

pub fn find_and_report_envs_stdio(options: FindOptions) {
    stdio::initialize_logger(if options.verbose {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Warn
    });
    let now = SystemTime::now();
    let search_scope = if options.workspace_only {
        Some(SearchScope::Workspace)
    } else if options.global_only {
        Some(SearchScope::Global)
    } else {
        None
    };
    let (config, executable_to_find) = create_config(&options);
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));
    let poetry_locator = Arc::new(Poetry::from(&environment));

    let locators = create_locators(conda_locator.clone(), poetry_locator.clone(), &environment);
    for locator in locators.iter() {
        locator.configure(&config);
    }

    if let Some(executable) = executable_to_find {
        find_env(&executable, &locators, &environment)
    } else {
        find_envs(
            &options,
            &locators,
            config,
            conda_locator.as_ref(),
            poetry_locator.as_ref(),
            &environment,
            search_scope,
        );
    }

    println!("Completed in {}ms", now.elapsed().unwrap().as_millis())
}

fn create_config(options: &FindOptions) -> (Configuration, Option<PathBuf>) {
    let mut config = Configuration::default();
    let mut workspace_directories = vec![];
    if let Some(dirs) = options.workspace_dirs.clone() {
        workspace_directories.extend(dirs);
    }
    // If workspace folders have been provided do not add cwd.
    if workspace_directories.is_empty() {
        if let Ok(cwd) = env::current_dir() {
            workspace_directories.push(cwd);
        }
    }
    workspace_directories.sort();
    workspace_directories.dedup();

    let executable_to_find =
        if workspace_directories.len() == 1 && workspace_directories[0].is_file() {
            Some(workspace_directories[0].clone())
        } else {
            None
        };
    config.workspace_directories = Some(workspace_directories);

    (config, executable_to_find)
}

fn find_envs(
    options: &FindOptions,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    config: Configuration,
    conda_locator: &Conda,
    poetry_locator: &Poetry,
    environment: &dyn Environment,
    search_scope: Option<SearchScope>,
) {
    let stdio_reporter = Arc::new(stdio::create_reporter(options.print_list));
    let reporter = CacheReporter::new(stdio_reporter.clone());

    let summary = find_and_report_envs(&reporter, config, locators, environment, search_scope);
    if options.report_missing {
        // By now all conda envs have been found
        // Spawn conda
        // & see if we can find more environments by spawning conda.
        let _ = conda_locator.find_and_report_missing_envs(&reporter, None);
        let _ = poetry_locator.find_and_report_missing_envs(&reporter, None);
    }

    if options.print_summary {
        let summary = summary.lock().unwrap();
        if !summary.find_locators_times.is_empty() {
            println!();
            println!("Breakdown by each locator:");
            println!("--------------------------");
            for locator in summary.find_locators_times.iter() {
                println!("{:<20} : {:?}", locator.0, locator.1);
            }
            println!();
        }

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
            "Workspace folders", summary.find_workspace_directories_time
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
}

fn find_env(
    executable: &PathBuf,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    environment: &dyn Environment,
) {
    let collect_reporter = Arc::new(collect::create_reporter());
    let reporter = CacheReporter::new(collect_reporter.clone());
    let stdio_reporter = Arc::new(stdio::create_reporter(true));

    let global_env_search_paths: Vec<PathBuf> = get_search_paths_from_env_variables(environment);

    identify_python_executables_using_locators(
        vec![executable.clone()],
        locators,
        &reporter,
        &global_env_search_paths,
    );

    // Find the environment for the file provided.
    let environments = collect_reporter.environments.lock().unwrap();
    if let Some(env) = environments
        .iter()
        .find(|e| e.symlinks.clone().unwrap_or_default().contains(executable))
    {
        if let Some(manager) = &env.manager {
            stdio_reporter.report_manager(manager);
        }
        stdio_reporter.report_environment(env);
    } else {
        warn!("Failed to find the environment for {:?}", executable);
    }
}

pub fn resolve_report_stdio(executable: PathBuf, verbose: bool) {
    stdio::initialize_logger(if verbose {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Warn
    });
    let now = SystemTime::now();
    let stdio_reporter = Arc::new(stdio::create_reporter(true));
    let reporter = CacheReporter::new(stdio_reporter.clone());
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));
    let poetry_locator = Arc::new(Poetry::from(&environment));

    let mut config = Configuration::default();
    if let Ok(cwd) = env::current_dir() {
        config.workspace_directories = Some(vec![cwd]);
    }

    let locators = create_locators(conda_locator.clone(), poetry_locator.clone(), &environment);
    for locator in locators.iter() {
        locator.configure(&config);
    }

    if let Some(result) = resolve_environment(&executable, &locators, &environment) {
        //
        println!("Environment found for {:?}", executable);
        let env = &result.resolved.unwrap_or(result.discovered);
        if let Some(manager) = &env.manager {
            reporter.report_manager(manager);
        }
        reporter.report_environment(env);
    } else {
        println!("No environment found for {:?}", executable);
    }

    println!(
        "Resolve completed in {}ms",
        now.elapsed().unwrap().as_millis()
    )
}
