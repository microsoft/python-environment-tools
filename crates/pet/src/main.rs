// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jsonrpc::start_jsonrpc_server;
use pet::{find_and_report_envs_stdio, resolve_report_stdio, FindOptions};

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
        /// List of folders to search for environments.
        /// The current directory is automatically used as a workspace folder if none provided.
        #[arg(value_name = "WORKSPACE FOLDERS")]
        workspace_dirs: Option<Vec<PathBuf>>,

        /// List the environments found.
        #[arg(short, long)]
        list: bool,

        /// Display verbose output (defaults to warnings).
        #[arg(short, long)]
        verbose: bool,

        /// Look for missing environments and report them (e.g. spawn conda and find what was missed).
        #[arg(short, long)]
        report_missing: bool,

        /// Exclusively search just the workspace directories.
        /// I.e. exclude all global environments.
        #[arg(short, long)]
        workspace_only: bool,
    },
    /// Resolves & reports the details of the the environment to the standard output.
    Resolve {
        /// Fully qualified path to the Python executable
        #[arg(value_name = "PYTHON EXE")]
        executable: PathBuf,

        /// Whether to display verbose output (defaults to warnings).
        #[arg(short, long)]
        verbose: bool,
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
        workspace_dirs: None,
        workspace_only: false,
    }) {
        Commands::Find {
            list,
            verbose,
            report_missing,
            workspace_dirs,
            workspace_only,
        } => find_and_report_envs_stdio(FindOptions {
            print_list: list,
            print_summary: true,
            verbose,
            report_missing,
            workspace_dirs,
            workspace_only,
        }),
        Commands::Resolve {
            executable,
            verbose,
        } => resolve_report_stdio(executable, verbose),
        Commands::Server => start_jsonrpc_server(),
    }
}
