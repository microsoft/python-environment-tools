// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::ops::Deref;

use lazy_static::lazy_static;
use log::warn;
use pet_core::{
    python_environment::PythonEnvironment, reporter::Reporter,
    telemetry::inaccurate_python_info::InaccuratePythonEnvironmentInfo,
};
use pet_fs::path::norm_case;
use regex::Regex;

lazy_static! {
    static ref PYTHON_VERSION: Regex = Regex::new(r"(\d+\.\d+\.\d+).*")
        .expect("Error creating Python Version Regex for comparison");
}

pub fn report_inaccuracies_identified_after_resolving(
    _reporter: &dyn Reporter,
    env: &PythonEnvironment,
    resolved: &PythonEnvironment,
) -> Option<()> {
    let known_symlinks = env.symlinks.clone().unwrap_or_default();
    let resolved_executable = &resolved.executable.clone()?;
    let norm_cased_executable = norm_case(resolved_executable);

    let mut invalid_executable = env.executable.clone().unwrap_or_default()
        != resolved_executable.deref()
        && env.executable.clone().unwrap_or_default() != norm_cased_executable;
    if env.executable.clone().is_none() {
        invalid_executable = false;
    }

    let mut executable_not_in_symlinks = !known_symlinks.contains(resolved_executable)
        && !known_symlinks.contains(&norm_cased_executable);
    if env.executable.is_none() {
        executable_not_in_symlinks = false;
    }

    let mut invalid_prefix = env.prefix.clone().unwrap_or_default() != resolved.prefix.clone()?;
    if env.prefix.clone().is_none() {
        invalid_prefix = false;
    }

    let mut invalid_arch = env.arch.clone() != resolved.arch.clone();
    if env.arch.clone().is_none() {
        invalid_arch = false;
    }

    let invalid_version = are_versions_different(
        &resolved.version.clone()?,
        &env.version.clone().unwrap_or_default(),
    );

    if invalid_executable
        || executable_not_in_symlinks
        || invalid_prefix
        || invalid_arch
        || invalid_version.unwrap_or_default()
    {
        let event = InaccuratePythonEnvironmentInfo {
            category: env.category.clone(),
            invalid_executable: Some(invalid_executable),
            executable_not_in_symlinks: Some(executable_not_in_symlinks),
            invalid_prefix: Some(invalid_prefix),
            invalid_version,
            invalid_arch: Some(invalid_arch),
        };
        warn!(
            "Inaccurate Python Environment Info for => \n{}.\nResolved as => \n{}\nIncorrect information => \n{}",
            env, resolved, event
        );
        // reporter.report_telemetry(TelemetryEvent::InaccuratePythonEnvironmentInfo(event));
    }
    Option::Some(())
}

fn are_versions_different(actual: &str, expected: &str) -> Option<bool> {
    let actual = PYTHON_VERSION.captures(actual)?;
    let actual = actual.get(1)?.as_str().to_string();
    let expected = PYTHON_VERSION.captures(expected)?;
    let expected = expected.get(1)?.as_str().to_string();
    Some(actual != expected)
}
