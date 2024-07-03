// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info, trace};
use pet::resolve::resolve_environment;
use pet_conda::Conda;
use pet_core::{
    os_environment::EnvironmentApi, python_environment::PythonEnvironment, reporter::Reporter,
    Configuration, Locator,
};
use pet_jsonrpc::{
    send_error, send_reply,
    server::{start_server, HandlersKeyedByMethodName},
};
use pet_reporter::{cache::CacheReporter, environment::Environment, jsonrpc};
use pet_telemetry::report_inaccuracies_identified_after_resolving;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, SystemTime, SystemTimeError},
};

use crate::{find::find_and_report_envs, locators::create_locators};

pub struct Context {
    reporter: Arc<dyn Reporter>,
    configuration: RwLock<Configuration>,
    locators: Arc<Vec<Arc<dyn Locator>>>,
    conda_locator: Arc<Conda>,
}

pub fn start_jsonrpc_server() {
    jsonrpc::initialize_logger(log::LevelFilter::Trace);

    // These are globals for the the lifetime of the server.
    // Hence passed around as Arcs via the context.
    let environment = EnvironmentApi::new();
    let jsonrpc_reporter = Arc::new(jsonrpc::create_reporter());
    let reporter = Arc::new(CacheReporter::new(jsonrpc_reporter.clone()));
    let conda_locator = Arc::new(Conda::from(&environment));
    let context = Context {
        reporter,
        locators: create_locators(conda_locator.clone()),
        conda_locator,
        configuration: RwLock::new(Configuration::default()),
    };

    let mut handlers = HandlersKeyedByMethodName::new(Arc::new(context));
    handlers.add_request_handler("refresh", handle_refresh);
    handlers.add_request_handler("resolve", handle_resolve);
    start_server(&handlers)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestOptions {
    /// These are paths like workspace folders, where we can look for environments.
    pub search_paths: Option<Vec<PathBuf>>,
    pub conda_executable: Option<PathBuf>,
    pub poetry_executable: Option<PathBuf>,
    /// Custom locations where environments can be found.
    /// These are different from search_paths, as these are specific directories where environments are expected.
    /// search_paths on the other hand can be any directory such as a workspace folder, where envs might never exist.
    pub environment_paths: Option<Vec<PathBuf>>,
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
    match serde_json::from_value::<RequestOptions>(params.clone()) {
        Ok(request_options) => {
            // Start in a new thread, we can have multiple requests.
            thread::spawn(move || {
                let mut cfg = context.configuration.write().unwrap();
                cfg.search_paths = request_options.search_paths;
                cfg.conda_executable = request_options.conda_executable;
                drop(cfg);
                let config = context.configuration.read().unwrap().clone();
                for locator in context.locators.iter() {
                    locator.configure(&config);
                }
                let summary = find_and_report_envs(
                    context.reporter.as_ref(),
                    config,
                    &context.locators,
                    context.conda_locator.clone(),
                );
                let summary = summary.lock().unwrap();
                for locator in summary.find_locators_times.iter() {
                    info!("Locator {} took {:?}", locator.0, locator.1);
                }
                info!(
                    "Environments found using locators in {:?}",
                    summary.find_locators_time
                );
                info!("Environments in PATH found in {:?}", summary.find_path_time);
                info!(
                    "Environments in global virtual env paths found in {:?}",
                    summary.find_global_virtual_envs_time
                );
                info!(
                    "Environments in custom search paths found in {:?}",
                    summary.find_search_paths_time
                );
                send_reply(id, Some(RefreshResult::new(summary.time)));
            });
        }
        Err(e) => {
            send_reply(id, None::<u128>);
            error!("Failed to parse request options {:?}: {}", params, e);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolveOptions {
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolveResult {
    environment: Environment,
    duration: Option<u128>,
}

impl ResolveResult {
    fn new(env: &PythonEnvironment, duration: Result<Duration, SystemTimeError>) -> ResolveResult {
        ResolveResult {
            environment: Environment::from(env),
            duration: duration.ok().map(|d| d.as_millis()),
        }
    }
}

pub fn handle_resolve(context: Arc<Context>, id: u32, params: Value) {
    match serde_json::from_value::<ResolveOptions>(params.clone()) {
        Ok(request_options) => {
            let executable = request_options.executable.clone();
            let search_paths = context.configuration.read().unwrap().clone().search_paths;
            let search_paths = search_paths.unwrap_or_default();
            // Start in a new thread, we can have multiple resolve requests.
            thread::spawn(move || {
                let now = SystemTime::now();
                trace!("Resolving env {:?}", executable);
                if let Some(result) =
                    resolve_environment(&executable, &context.locators, search_paths)
                {
                    if let Some(resolved) = result.resolved {
                        // Gather telemetry of this resolved env and see what we got wrong.
                        let _ = report_inaccuracies_identified_after_resolving(
                            context.reporter.as_ref(),
                            &result.discovered,
                            &resolved,
                        );

                        trace!("Resolved env {:?} as {:?}", executable, resolved);
                        send_reply(id, Some(ResolveResult::new(&resolved, now.elapsed())));
                    } else {
                        error!(
                            "Failed to resolve env {:?}, returning discovered env {:?}",
                            executable, result.discovered
                        );
                        send_reply(
                            id,
                            Some(ResolveResult::new(&result.discovered, now.elapsed())),
                        );
                    }
                } else {
                    error!("Failed to resolve env {:?}", executable);
                    send_error(
                        Some(id),
                        -4,
                        format!("Failed to resolve env {:?}", executable),
                    );
                }
            });
        }
        Err(e) => {
            error!("Failed to parse request {:?}: {}", params, e);
            send_error(
                Some(id),
                -4,
                format!("Failed to parse request {:?}: {}", params, e),
            );
        }
    }
}
