// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jsonrpc::start_jsonrpc_server;
use pet::{find_and_report_envs_stdio, resolve_report_stdio, FindOptions};
use pet_core::python_environment::PythonEnvironmentKind;

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
        /// List of files/folders to search for environments.
        /// The current directory is automatically used as a workspace folder if none provided.
        #[arg(value_name = "SEARCH PATHS")]
        search_paths: Option<Vec<PathBuf>>,

        /// List the environments found.
        #[arg(short, long)]
        list: bool,

        /// Directory to cache the environment information after spawning Python.
        #[arg(short, long)]
        cache_directory: Option<PathBuf>,

        /// Display verbose output (defaults to warnings).
        #[arg(short, long)]
        verbose: bool,

        /// Look for missing environments and report them (e.g. spawn conda and find what was missed).
        #[arg(short, long)]
        report_missing: bool,

        /// Exclusively search just the workspace directories.
        /// I.e. exclude all global environments.
        #[arg(short, long, conflicts_with = "kind")]
        workspace: bool,

        /// Exclusively search for a specific Python environment kind.
        /// Will not search in the workspace directories.
        #[arg(short, long, conflicts_with = "workspace")]
        kind: Option<PythonEnvironmentKind>,
    },
    /// Resolves & reports the details of the the environment to the standard output.
    Resolve {
        /// Fully qualified path to the Python executable
        #[arg(value_name = "PYTHON EXE")]
        executable: PathBuf,

        /// Directory to cache the environment information after spawning Python.
        #[arg(short, long)]
        cache_directory: Option<PathBuf>,

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
        search_paths: None,
        workspace: false,
        cache_directory: None,
        kind: None,
    }) {
        Commands::Find {
            list,
            verbose,
            report_missing,
            search_paths,
            workspace,
            cache_directory,
            kind,
        } => {
            let mut workspace_only = workspace;
            if search_paths.clone().is_some()
                && search_paths
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .all(|f| f.is_file())
            {
                workspace_only = true;
            }

            find_and_report_envs_stdio(FindOptions {
                print_list: list,
                print_summary: true,
                verbose,
                report_missing,
                search_paths,
                workspace_only,
                cache_directory,
                kind,
            });
        }
        Commands::Resolve {
            executable,
            verbose,
            cache_directory,
        } => resolve_report_stdio(executable, verbose, cache_directory),
        Commands::Server => start_jsonrpc_server(),
    }
}
