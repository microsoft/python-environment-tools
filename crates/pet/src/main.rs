// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet::find_and_report_envs_stdio;

fn main() {
    // initialize_logger(LevelFilter::Trace);

    // log::info!("Starting Native Locator");
    // let now = SystemTime::now();
    // let mut dispatcher = create_dispatcher();

    find_and_report_envs_stdio();
    // find_and_report_envs_jsonrpc();

    // match now.elapsed() {
    //     Ok(elapsed) => {
    //         log::info!("Native Locator took {} milliseconds.", elapsed.as_millis());
    //     }
    //     Err(e) => {
    //         log::error!("Error getting elapsed time: {:?}", e);
    //     }
    // }

    // dispatcher.exit();
}
