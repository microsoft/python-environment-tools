// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use clap::{Parser, Subcommand};
use jsonrpc::start_jsonrpc_server;
use pet::find_and_report_envs_stdio;

mod find;
mod jsonrpc;
mod locators;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Finds the environments and reports them to the standard output.
    Find,
    /// Starts the JSON RPC Server (note: today server shuts down immediately, that's a bug).
    Server,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Server) => start_jsonrpc_server(),
        _ => find_and_report_envs_stdio(),
    }
}
