// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info, trace};
use pet::resolve::resolve_environment;
use pet_conda::Conda;
use pet_conda::CondaLocator;
use pet_core::{
    os_environment::{Environment, EnvironmentApi},
    reporter::Reporter,
    Configuration, Locator,
};
use pet_jsonrpc::{
    send_error, send_reply,
    server::{start_server, HandlersKeyedByMethodName},
};
use pet_poetry::Poetry;
use pet_poetry::PoetryLocator;
use pet_reporter::{cache::CacheReporter, jsonrpc};
use pet_telemetry::report_inaccuracies_identified_after_resolving;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, SystemTime},
};

use crate::{find::find_and_report_envs, locators::create_locators};

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

pub fn handle_refresh(context: Arc<Context>, id: u32, _params: Value) {
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
            "Environments in workspace folders found in {:?}",
            summary.find_workspace_directories_time
        );
        trace!("Finished refreshing environments in {:?}", summary.time);
        send_reply(id, Some(RefreshResult::new(summary.time)));

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
                conda_locator.find_and_report_missing_envs(reporter_ref.as_ref(), conda_executable);
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
            error!("Failed to parse request {params:?}: {e}");
            send_error(
                Some(id),
                -4,
                format!("Failed to parse request {params:?}: {e}"),
            );
        }
    }
}
