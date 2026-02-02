// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::find::find_and_report_envs;
use crate::find::find_python_environments_in_workspace_folder_recursive;
use crate::find::identify_python_executables_using_locators;
use crate::find::SearchScope;
use crate::locators::create_locators;
use lazy_static::lazy_static;
use log::{error, info, trace};
use pet::initialize_tracing;
use pet::resolve::resolve_environment;
use pet_conda::Conda;
use pet_conda::CondaLocator;
use pet_core::python_environment::PythonEnvironment;
use pet_core::python_environment::PythonEnvironmentKind;
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
use pet_python_utils::cache::clear_cache;
use pet_python_utils::cache::set_cache_directory;
use pet_reporter::collect;
use pet_reporter::{cache::CacheReporter, jsonrpc};
use pet_telemetry::report_inaccuracies_identified_after_resolving;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::{self, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::SystemTime,
};
use tracing::info_span;

lazy_static! {
    /// Used to ensure we can have only one refreh at a time.
    static ref REFRESH_LOCK: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
}

pub struct Context {
    configuration: RwLock<Configuration>,
    locators: Arc<Vec<Arc<dyn Locator>>>,
    conda_locator: Arc<Conda>,
    poetry_locator: Arc<Poetry>,
    os_environment: Arc<dyn Environment>,
}

static MISSING_ENVS_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn start_jsonrpc_server() {
    // Initialize tracing for performance profiling (controlled by RUST_LOG env var)
    // Note: This includes log compatibility, so we don't call jsonrpc::initialize_logger
    initialize_tracing(false);

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
    handlers.add_request_handler("clear", handle_clear_cache);
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
    /// Directory to cache the Python environment details.
    pub cache_directory: Option<PathBuf>,
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
                // We will not support changing the cache directories once set.
                // No point, supporting such a use case.
                if let Some(cache_directory) = configure_options.cache_directory {
                    set_cache_directory(cache_directory.clone());
                    cfg.cache_directory = Some(cache_directory);
                }
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
    /// If provided, then limit the search to this kind of environments.
    pub search_kind: Option<PythonEnvironmentKind>,
    /// If provided, then limit the search paths to these.
    /// Note: Search paths can also include Python exes or Python env folders.
    /// Traditionally, search paths are workspace folders.
    pub search_paths: Option<Vec<PathBuf>>,
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
        Value::Array(_) => json!({}),
        _ => params,
    };
    match serde_json::from_value::<Option<RefreshOptions>>(params.clone()) {
        Ok(refresh_options) => {
            let refresh_options = refresh_options.unwrap_or(RefreshOptions {
                search_kind: None,
                search_paths: None,
            });
            // Start in a new thread, we can have multiple requests.
            thread::spawn(move || {
                let _span = info_span!("handle_refresh",
                    search_kind = ?refresh_options.search_kind,
                    has_search_paths = refresh_options.search_paths.is_some()
                )
                .entered();

                // Ensure we can have only one refresh at a time.
                let lock = REFRESH_LOCK.lock().unwrap();

                let mut config = context.configuration.read().unwrap().clone();
                let reporter = Arc::new(CacheReporter::new(Arc::new(jsonrpc::create_reporter(
                    refresh_options.search_kind,
                ))));

                let mut search_scope = None;

                // If search kind is provided and no search_paths, then we will only search in the global locations.
                if refresh_options.search_kind.is_some() || refresh_options.search_paths.is_some() {
                    // Always clear this, as we will either serach in specified folder or a specific kind in global locations.
                    config.workspace_directories = None;
                    if let Some(search_paths) = refresh_options.search_paths {
                        // These workspace folders are only for this refresh.
                        config.workspace_directories = Some(
                            search_paths
                                .iter()
                                .filter(|p| p.is_dir())
                                .cloned()
                                .collect(),
                        );
                        config.executables = Some(
                            search_paths
                                .iter()
                                .filter(|p| p.is_file())
                                .cloned()
                                .collect(),
                        );
                        search_scope = Some(SearchScope::Workspace);
                    } else if let Some(search_kind) = refresh_options.search_kind {
                        config.executables = None;
                        search_scope = Some(SearchScope::Global(search_kind));
                    }

                    // Configure the locators with the modified config.
                    for locator in context.locators.iter() {
                        locator.configure(&config);
                    }
                } else {
                    // Re-configure the locators with an un-modified config.
                    // Possible we congirued the locators with a modified config in the in the previous request.
                    // & the config was scoped to a particular search folder, executables or kind.
                    for locator in context.locators.iter() {
                        locator.configure(&config);
                    }
                }

                trace!("Start refreshing environments, config: {:?}", config);
                let summary = find_and_report_envs(
                    reporter.as_ref(),
                    config,
                    &context.locators,
                    context.os_environment.deref(),
                    search_scope,
                );
                let summary = summary.lock().unwrap();
                for locator in summary.locators.iter() {
                    info!("Locator {:?} took {:?}", locator.0, locator.1);
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
                        .map(|(k, v)| (format!("{k:?}"), v.as_millis()))
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

                drop(lock);
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
                        let jsonrpc_reporter = jsonrpc::create_reporter(None);
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
#[serde(rename_all = "camelCase")]
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
                        context
                            .configuration
                            .read()
                            .unwrap()
                            .clone()
                            .environment_directories
                            .as_deref()
                            .unwrap_or(&[]),
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

pub fn handle_clear_cache(_context: Arc<Context>, id: u32, _params: Value) {
    thread::spawn(move || {
        if let Err(e) = clear_cache() {
            error!("Failed to clear cache {:?}", e);
            send_error(Some(id), -4, format!("Failed to clear cache {e:?}"));
        } else {
            info!("Cleared cache");
            send_reply(id, None::<()>);
        }
    });
}
