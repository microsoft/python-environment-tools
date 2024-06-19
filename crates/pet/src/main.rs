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
    Find {
        #[arg(short, long)]
        list: Option<bool>,
    },
    /// Starts the JSON RPC Server.
    Server,
}

fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Find { list: Some(true) }) {
        Commands::Find { list } => find_and_report_envs_stdio(list.unwrap_or(true), true),
        Commands::Server => start_jsonrpc_server(),
    }
}
