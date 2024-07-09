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
        list: bool,

        /// Whether to display verbose output (defaults to just info).
        #[arg(short, long)]
        verbose: bool,

        /// Whether to look for missing environments and report them (e.g. spawn conda and find what was missed).
        #[arg(short, long)]
        report_missing: bool,
    },
    /// Starts the JSON RPC Server.
    Server,
}

fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Find {
        list: true,
        verbose: false,
        report_missing: false,
    }) {
        Commands::Find {
            list,
            verbose,
            report_missing,
        } => find_and_report_envs_stdio(list, true, verbose, report_missing),
        Commands::Server => start_jsonrpc_server(),
    }
}
