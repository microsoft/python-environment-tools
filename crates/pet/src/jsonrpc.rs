// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace};
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
use pet_reporter::{environment::Environment, jsonrpc};
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
    let conda_locator = Arc::new(Conda::from(&environment));
    let context = Context {
        reporter: jsonrpc_reporter,
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
    duration: Option<u128>,
}

impl RefreshResult {
    pub fn new(duration: Result<Duration, SystemTimeError>) -> RefreshResult {
        RefreshResult {
            duration: duration.ok().map(|d| d.as_millis()),
        }
    }
}

pub fn handle_refresh(context: Arc<Context>, id: u32, params: Value) {
    match serde_json::from_value::<RequestOptions>(params.clone()) {
        Ok(request_options) => {
            let mut cfg = context.configuration.write().unwrap();
            cfg.search_paths = request_options.search_paths;
            cfg.conda_executable = request_options.conda_executable;
            drop(cfg);
            let config = context.configuration.read().unwrap().clone();
            for locator in context.locators.iter() {
                locator.configure(&config);
            }
            let now = SystemTime::now();
            find_and_report_envs(
                context.reporter.as_ref(),
                config,
                &context.locators,
                context.conda_locator.clone(),
            );
            send_reply(id, Some(RefreshResult::new(now.elapsed())));
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
            // Start in a new thread, we can have multiple resolve requests.
            thread::spawn(move || {
                let now = SystemTime::now();
                trace!("Resolving env {:?}", executable);
                if let Some(result) = resolve_environment(&executable, &context.locators) {
                    if let Some(resolved) = result.resolved {
                        // Gather telemetry of this resolved env and see what we got wrong.
                        let _ = report_inaccuracies_identified_after_resolving(
                            context.reporter.as_ref(),
                            &result.discovered,
                            &resolved,
                        );

                        send_reply(id, Some(ResolveResult::new(&resolved, now.elapsed())));
                    } else {
                        error!(
                            "Failed to resolve env, returning discovered env {:?}",
                            executable
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
