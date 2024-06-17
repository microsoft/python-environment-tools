// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info};
use pet::locators::{find_and_report_envs, Configuration};
use pet_conda::Conda;
use pet_core::{os_environment::EnvironmentApi, reporter::Reporter};
use pet_jsonrpc::{
    send_reply,
    server::{start_server, HandlersKeyedByMethodName},
};
use pet_reporter::jsonrpc;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::SystemTime,
};

pub struct Context {
    reporter: Arc<dyn Reporter>,
    conda_locator: Arc<Conda>,
    configuration: RwLock<Configuration>,
}

pub fn start_jsonrpc_server() {
    jsonrpc::initialize_logger(log::LevelFilter::Trace);

    // These are globals for the the lifetime of the server.
    // Hence passed around as Arcs via the context.
    let environment = EnvironmentApi::new();
    let jsonrpc_reporter = jsonrpc::create_reporter();
    let conda_locator = Arc::new(Conda::from(&environment));
    let context = Context {
        reporter: Arc::new(jsonrpc_reporter),
        conda_locator,
        configuration: RwLock::new(Configuration::default()),
    };

    let mut handlers = HandlersKeyedByMethodName::new(Arc::new(context));
    handlers.add_request_handler("refresh", handle_refresh);
    start_server(&handlers)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestOptions {
    pub search_paths: Option<Vec<PathBuf>>,
    pub conda_executable: Option<PathBuf>,
}

pub fn handle_refresh(context: Arc<Context>, id: u32, params: Value) {
    let request_options: RequestOptions = serde_json::from_value(params).unwrap();
    let mut cfg = context.configuration.write().unwrap();
    cfg.search_paths = request_options.search_paths;
    cfg.conda_executable = request_options.conda_executable;
    drop(cfg);
    let config = context.configuration.read().unwrap().clone();

    info!("Started Refreshing Environments");
    let now = SystemTime::now();
    find_and_report_envs(
        context.reporter.as_ref(),
        context.conda_locator.clone(),
        config,
    );

    if let Ok(duration) = now.elapsed() {
        send_reply(id, Some(duration.as_millis()));
    } else {
        send_reply(id, None::<u128>);
        error!("Failed to calculate duration");
    }
}
