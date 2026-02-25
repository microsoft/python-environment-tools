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
) -> Option<InaccuratePythonEnvironmentInfo> {
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

    let invalid_prefix = if let Some(ref env_prefix) = env.prefix {
        let resolved_prefix = resolved.prefix.clone()?;
        // Canonicalize both paths to handle symlinks — a venv prefix like
        // /usr/local/venvs/myvenv may be a symlink to /usr/local/venvs/versioned/myvenv-1.0.51,
        // and sys.prefix returns the resolved target. Without this, the comparison
        // produces a false positive "Prefix is incorrect" warning. (See #358)
        // Wrap in norm_case to handle Windows UNC prefix (`\\?\`) from canonicalize.
        let env_canonical =
            norm_case(std::fs::canonicalize(env_prefix).unwrap_or(env_prefix.clone()));
        let resolved_canonical =
            norm_case(std::fs::canonicalize(&resolved_prefix).unwrap_or(resolved_prefix));
        env_canonical != resolved_canonical
    } else {
        false
    };

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
            kind: env.kind,
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
        return Some(event);
    }
    None
}

fn are_versions_different(actual: &str, expected: &str) -> Option<bool> {
    let actual = PYTHON_VERSION.captures(actual)?;
    let actual = actual.get(1)?.as_str().to_string();
    let expected = PYTHON_VERSION.captures(expected)?;
    let expected = expected.get(1)?.as_str().to_string();
    Some(actual != expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_core::{
        manager::EnvManager,
        python_environment::{PythonEnvironmentBuilder, PythonEnvironmentKind},
        telemetry::TelemetryEvent,
    };
    use std::path::PathBuf;

    struct NoopReporter;
    impl Reporter for NoopReporter {
        fn report_manager(&self, _: &EnvManager) {}
        fn report_environment(&self, _: &PythonEnvironment) {}
        fn report_telemetry(&self, _: &TelemetryEvent) {}
    }

    fn make_env(
        executable: PathBuf,
        prefix: PathBuf,
        version: &str,
        symlinks: Vec<PathBuf>,
    ) -> PythonEnvironment {
        PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Venv))
            .executable(Some(executable))
            .prefix(Some(prefix))
            .version(Some(version.to_string()))
            .symlinks(Some(symlinks))
            .build()
    }

    #[test]
    fn same_prefix_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().to_path_buf();
        let exe = prefix.join("bin").join("python");

        let env = make_env(exe.clone(), prefix.clone(), "3.12.7", vec![exe.clone()]);
        let resolved = make_env(exe.clone(), prefix, "3.12.7", vec![exe]);

        let result = report_inaccuracies_identified_after_resolving(&NoopReporter, &env, &resolved);
        assert!(result.is_none(), "identical prefixes should not be flagged");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_prefix_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let real_prefix = dir.path().join("versioned").join("myvenv-1.0.51");
        std::fs::create_dir_all(&real_prefix).unwrap();
        let symlink_prefix = dir.path().join("myvenv");
        std::os::unix::fs::symlink(&real_prefix, &symlink_prefix).unwrap();

        let exe = symlink_prefix.join("bin").join("python");

        // Discovery sees the symlink path as prefix
        let env = make_env(exe.clone(), symlink_prefix, "3.12.7", vec![exe.clone()]);
        // Resolution (spawning Python) returns the canonical path
        let resolved = make_env(exe.clone(), real_prefix, "3.12.7", vec![exe]);

        let result = report_inaccuracies_identified_after_resolving(&NoopReporter, &env, &resolved);
        assert!(
            result.is_none(),
            "symlinked prefix to the same directory should not be flagged"
        );
    }

    #[test]
    fn different_prefix_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let prefix_a = dir.path().join("env_a");
        let prefix_b = dir.path().join("env_b");
        std::fs::create_dir_all(&prefix_a).unwrap();
        std::fs::create_dir_all(&prefix_b).unwrap();

        let exe = prefix_a.join("bin").join("python");

        let env = make_env(exe.clone(), prefix_a, "3.12.7", vec![exe.clone()]);
        let resolved = make_env(exe.clone(), prefix_b, "3.12.7", vec![exe]);

        let result = report_inaccuracies_identified_after_resolving(&NoopReporter, &env, &resolved);
        let event = result.expect("genuinely different prefixes should be flagged");
        assert_eq!(event.invalid_prefix, Some(true));
    }

    #[test]
    fn none_prefix_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().to_path_buf();
        let exe = prefix.join("bin").join("python");

        // env has no prefix
        let env = PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Venv))
            .executable(Some(exe.clone()))
            .version(Some("3.12.7".to_string()))
            .symlinks(Some(vec![exe.clone()]))
            .build();
        let resolved = make_env(exe.clone(), prefix, "3.12.7", vec![exe]);

        let result = report_inaccuracies_identified_after_resolving(&NoopReporter, &env, &resolved);
        assert!(
            result.is_none(),
            "None prefix should not cause any inaccuracy flag"
        );
    }
}
