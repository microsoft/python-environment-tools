// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_reporter::{self, jsonrpc, stdio};

mod locators;

pub fn find_and_report_envs_jsonrpc() {
    jsonrpc::initialize_logger(log::LevelFilter::Trace);
    let jsonrpc_reporter = jsonrpc::create_reporter();
    locators::find_and_report_envs(&jsonrpc_reporter);
}
pub fn find_and_report_envs_stdio() {
    stdio::initialize_logger(log::LevelFilter::Info);
    let jsonrpc_reporter = stdio::create_reporter();
    locators::find_and_report_envs(&jsonrpc_reporter);
}
