// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info, trace};
use pet::resolve::resolve_environment;
use pet_conda::Conda;
use pet_conda::CondaLocator;
use pet_core::python_environment::PythonEnvironment;
use pet_core::telemetry::refresh_performance::RefreshPerformance;
use pet_core::telemetry::TelemetryEvent;
use pet_core::{
    os_environment::{Environment, EnvironmentApi},
    reporter::Reporter,
    Configuration, Locator,
};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_jsonrpc::{
    send_error, send_reply,
    server::{start_server, HandlersKeyedByMethodName},
};
use pet_poetry::Poetry;
use pet_poetry::PoetryLocator;
use pet_reporter::collect;
use pet_reporter::{cache::CacheReporter, jsonrpc};
use pet_telemetry::report_inaccuracies_identified_after_resolving;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::{self, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::SystemTime,
};

use crate::find::find_and_report_envs;
use crate::find::find_python_environments_in_workspace_folder_recursive;
use crate::find::identify_python_executables_using_locators;
use crate::find::SearchScope;
use crate::locators::create_locators;

pub struct Context {
    configuration: RwLock<Configuration>,
    locators: Arc<Vec<Arc<dyn Locator>>>,
    conda_locator: Arc<Conda>,
    poetry_locator: Arc<Poetry>,
    os_environment: Arc<dyn Environment>,
}

static MISSING_ENVS_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn start_jsonrpc_server() {
    jsonrpc::initialize_logger(log::LevelFilter::Trace);

    // These are globals for the the lifetime of the server.
    // Hence passed around as Arcs via the context.
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));
    let poetry_locator = Arc::new(Poetry::from(&environment));
    let context = Context {
        locators: create_locators(conda_locator.clone(), poetry_locator.clone(), &environment),
        conda_locator,
        poetry_locator,
        configuration: RwLock::new(Configuration::default()),
        os_environment: Arc::new(environment),
    };

    let mut handlers = HandlersKeyedByMethodName::new(Arc::new(context));
    handlers.add_request_handler("configure", handle_configure);
    handlers.add_request_handler("refresh", handle_refresh);
    handlers.add_request_handler("resolve", handle_resolve);
    handlers.add_request_handler("find", handle_find);
    handlers.add_request_handler("condaInfo", handle_conda_telemetry);
    start_server(&handlers)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureOptions {
    /// These are paths like workspace folders, where we can look for environments.
    pub workspace_directories: Option<Vec<PathBuf>>,
    pub conda_executable: Option<PathBuf>,
    pub poetry_executable: Option<PathBuf>,
    /// Custom locations where environments can be found. Generally global locations where virtualenvs & the like can be found.
    /// Workspace directories should not be included into this list.
    pub environment_directories: Option<Vec<PathBuf>>,
}

pub fn handle_configure(context: Arc<Context>, id: u32, params: Value) {
    match serde_json::from_value::<ConfigureOptions>(params.clone()) {
        Ok(configure_options) => {
            // Start in a new thread, we can have multiple requests.
            thread::spawn(move || {
                let mut cfg = context.configuration.write().unwrap();
                cfg.workspace_directories = configure_options.workspace_directories;
                cfg.conda_executable = configure_options.conda_executable;
                cfg.environment_directories = configure_options.environment_directories;
                cfg.poetry_executable = configure_options.poetry_executable;
                trace!("Configuring locators: {:?}", cfg);
                drop(cfg);
                let config = context.configuration.read().unwrap().clone();
                for locator in context.locators.iter() {
                    locator.configure(&config);
                }
                send_reply(id, None::<()>);
            });
        }
        Err(e) => {
            send_reply(id, None::<u128>);
            error!("Failed to parse configure options {:?}: {}", params, e);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOptions {
    /// The search paths are the paths where we will look for environments.
    /// Defaults to searching everywhere (or when None), else it can be restricted to a specific scope.
    pub search_scope: Option<SearchScope>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RefreshResult {
    duration: u128,
}

impl RefreshResult {
    pub fn new(duration: Duration) -> RefreshResult {
        RefreshResult {
            duration: duration.as_millis(),
        }
    }
}

pub fn handle_refresh(context: Arc<Context>, id: u32, params: Value) {
    let params = match params {
        Value::Null => json!({}),
        _ => params,
    };
    match serde_json::from_value::<RefreshOptions>(params.clone()) {
        Ok(refres_options) => {
            // Start in a new thread, we can have multiple requests.
            thread::spawn(move || {
                let config = context.configuration.read().unwrap().clone();
                let reporter = Arc::new(CacheReporter::new(Arc::new(jsonrpc::create_reporter())));

                trace!("Start refreshing environments, config: {:?}", config);
                let summary = find_and_report_envs(
                    reporter.as_ref(),
                    config,
                    &context.locators,
                    context.os_environment.deref(),
                    refres_options.search_scope,
                );
                let summary = summary.lock().unwrap();
                for locator in summary.locators.iter() {
                    info!("Locator {} took {:?}", locator.0, locator.1);
                }
                for item in summary.breakdown.iter() {
                    info!("Locator {} took {:?}", item.0, item.1);
                }
                trace!("Finished refreshing environments in {:?}", summary.total);
                send_reply(id, Some(RefreshResult::new(summary.total)));

                let perf = RefreshPerformance {
                    total: summary.total.as_millis(),
                    locators: summary
                        .locators
                        .clone()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.as_millis()))
                        .collect::<BTreeMap<String, u128>>(),
                    breakdown: summary
                        .breakdown
                        .clone()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.as_millis()))
                        .collect::<BTreeMap<String, u128>>(),
                };
                reporter.report_telemetry(&TelemetryEvent::RefreshPerformance(perf));
                // Find an report missing envs for the first launch of this process.
                if MISSING_ENVS_REPORTED
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .ok()
                    .unwrap_or_default()
                {
                    // By now all conda envs have been found
                    // Spawn conda  in a separate thread.
                    // & see if we can find more environments by spawning conda.
                    // But we will not wait for this to complete.
                    let conda_locator = context.conda_locator.clone();
                    let conda_executable = context
                        .configuration
                        .read()
                        .unwrap()
                        .conda_executable
                        .clone();
                    let reporter_ref = reporter.clone();
                    thread::spawn(move || {
                        conda_locator
                            .find_and_report_missing_envs(reporter_ref.as_ref(), conda_executable);
                        Some(())
                    });

                    // By now all poetry envs have been found
                    // Spawn poetry exe in a separate thread.
                    // & see if we can find more environments by spawning poetry.
                    // But we will not wait for this to complete.
                    let poetry_locator = context.poetry_locator.clone();
                    let poetry_executable = context
                        .configuration
                        .read()
                        .unwrap()
                        .poetry_executable
                        .clone();
                    let reporter_ref = reporter.clone();
                    thread::spawn(move || {
                        poetry_locator
                            .find_and_report_missing_envs(reporter_ref.as_ref(), poetry_executable);
                        Some(())
                    });
                }
            });
        }
        Err(e) => {
            error!("Failed to parse refresh {params:?}: {e}");
            send_error(
                Some(id),
                -4,
                format!("Failed to parse refresh {params:?}: {e}"),
            );
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolveOptions {
    pub executable: PathBuf,
}

pub fn handle_resolve(context: Arc<Context>, id: u32, params: Value) {
    match serde_json::from_value::<ResolveOptions>(params.clone()) {
        Ok(request_options) => {
            let executable = request_options.executable.clone();
            // Start in a new thread, we can have multiple resolve requests.
            let environment = context.os_environment.clone();
            thread::spawn(move || {
                let now = SystemTime::now();
                trace!("Resolving env {:?}", executable);
                if let Some(result) =
                    resolve_environment(&executable, &context.locators, environment.deref())
                {
                    if let Some(resolved) = result.resolved {
                        // Gather telemetry of this resolved env and see what we got wrong.
                        let jsonrpc_reporter = jsonrpc::create_reporter();
                        let _ = report_inaccuracies_identified_after_resolving(
                            &jsonrpc_reporter,
                            &result.discovered,
                            &resolved,
                        );

                        trace!(
                            "Resolved env ({:?}) {executable:?} as {resolved:?}",
                            now.elapsed()
                        );
                        send_reply(id, resolved.into());
                    } else {
                        error!(
                            "Failed to resolve env {executable:?}, returning discovered env {:?}",
                            result.discovered
                        );
                        send_reply(id, result.discovered.into());
                    }
                } else {
                    error!("Failed to resolve env {executable:?}");
                    send_error(
                        Some(id),
                        -4,
                        format!("Failed to resolve env {executable:?}"),
                    );
                }
            });
        }
        Err(e) => {
            error!("Failed to parse resolve {params:?}: {e}");
            send_error(
                Some(id),
                -4,
                format!("Failed to parse resolve {params:?}: {e}"),
            );
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindOptions {
    /// Search path, can be a directory or a Python executable as well.
    /// If passing a directory, the assumption is that its a project directory (workspace folder).
    /// This is important, because any poetry/pipenv environment found will have the project directory set.
    pub search_path: PathBuf,
}

pub fn handle_find(context: Arc<Context>, id: u32, params: Value) {
    thread::spawn(
        move || match serde_json::from_value::<FindOptions>(params.clone()) {
            Ok(find_options) => {
                let global_env_search_paths: Vec<PathBuf> =
                    get_search_paths_from_env_variables(context.os_environment.as_ref());

                let collect_reporter = Arc::new(collect::create_reporter());
                let reporter = CacheReporter::new(collect_reporter.clone());
                if find_options.search_path.is_file() {
                    identify_python_executables_using_locators(
                        vec![find_options.search_path.clone()],
                        &context.locators,
                        &reporter,
                        &global_env_search_paths,
                    );
                } else {
                    find_python_environments_in_workspace_folder_recursive(
                        &find_options.search_path,
                        &reporter,
                        &context.locators,
                        &global_env_search_paths,
                    );
                }

                let envs = collect_reporter.environments.lock().unwrap().clone();
                if envs.is_empty() {
                    send_reply(id, None::<Vec<PythonEnvironment>>);
                } else {
                    send_reply(id, envs.into());
                }
            }
            Err(e) => {
                error!("Failed to parse find {params:?}: {e}");
                send_error(
                    Some(id),
                    -4,
                    format!("Failed to parse find {params:?}: {e}"),
                );
            }
        },
    );
}

pub fn handle_conda_telemetry(context: Arc<Context>, id: u32, _params: Value) {
    thread::spawn(move || {
        let conda_locator = context.conda_locator.clone();
        let conda_executable = context
            .configuration
            .read()
            .unwrap()
            .conda_executable
            .clone();
        let info = conda_locator.get_info_for_telemetry(conda_executable);
        send_reply(id, info.into());
    });
}
