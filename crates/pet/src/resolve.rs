// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{path::PathBuf, sync::Arc};

use log::warn;
use pet_core::{arch::Architecture, python_environment::PythonEnvironment, Locator};
use pet_python_utils::env::{PythonEnv, ResolvedPythonEnv};

use crate::locators::identify_python_environment_using_locators;

pub struct ResolvedEnvironment {
    pub discovered: PythonEnvironment,
    pub resolved: Option<PythonEnvironment>,
}

pub fn resolve_environment(
    executable: &PathBuf,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
) -> Option<ResolvedEnvironment> {
    // First check if this is a known environment
    let env = PythonEnv::new(executable.to_owned(), None, None);
    if let Some(env) = identify_python_environment_using_locators(&env, locators) {
        // Ok we got the environment.
        // Now try to resolve this fully, by spawning python.
        if let Some(ref executable) = env.executable {
            if let Some(info) = ResolvedPythonEnv::from(executable) {
                let discovered = env.clone();
                let mut resolved = env.clone();
                let mut symlinks = resolved.symlinks.clone().unwrap_or_default();

                symlinks.push(info.executable.clone());
                symlinks.append(&mut info.symlink.clone().unwrap_or_default());
                resolved.version = Some(info.version);
                resolved.prefix = Some(info.prefix);
                resolved.arch = Some(if info.is64_bit {
                    Architecture::X64
                } else {
                    Architecture::X86
                });

                Some(ResolvedEnvironment {
                    discovered,
                    resolved: Some(resolved),
                })
            } else {
                Some(ResolvedEnvironment {
                    discovered: env,
                    resolved: None,
                })
            }
        } else {
            warn!("Unknown Python Env {:?} resolved as {:?}", executable, env);
            Some(ResolvedEnvironment {
                discovered: env,
                resolved: None,
            })
        }
    } else {
        warn!("Unknown Python Env {:?}", executable);
        None
    }
}
