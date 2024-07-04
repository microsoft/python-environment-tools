// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::env::PythonEnv;
use pet_python_utils::executable::find_executables;
use pet_python_utils::version;
use pet_virtualenv::is_virtualenv;
use std::fs;
use std::path::PathBuf;

pub struct MacPythonOrg {}

impl MacPythonOrg {
    pub fn new() -> MacPythonOrg {
        MacPythonOrg {}
    }
}
impl Default for MacPythonOrg {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for MacPythonOrg {
    fn get_name(&self) -> &'static str {
        "MacPythonOrg"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::MacPythonOrg]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if std::env::consts::OS != "macos" {
            return None;
        }
        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }

        let mut executable = resolve_symlink(&env.executable).unwrap_or(env.executable.clone());
        if !executable
            .to_string_lossy()
            .starts_with("/Library/Frameworks/Python.framework/Versions/")
        {
            return None;
        }

        let mut version_is_current = false;
        let mut symlinks = vec![executable.clone(), env.executable.clone()];
        if executable.starts_with("/Library/Frameworks/Python.framework/Versions/Current") {
            // This is a symlink to the python executable, lets resolve it
            let exe_to_resolve =
                "/Library/Frameworks/Python.framework/Versions/Current/bin/python3";
            if let Some(exe) = resolve_symlink(&exe_to_resolve) {
                if exe.starts_with("/Library/Frameworks/Python.framework/Versions")
                    && !exe.starts_with("/Library/Frameworks/Python.framework/Versions/Current")
                {
                    // Given that the exe we were given is the `Current/bin/python`, we know this is current.
                    version_is_current = true;
                    symlinks.push(exe.clone());
                    symlinks.push(PathBuf::from(exe_to_resolve));
                    executable = exe;
                }
            }
        } else {
            // Check if this is the current version.
            let exe_to_resolve =
                "/Library/Frameworks/Python.framework/Versions/Current/bin/python3";
            if let Some(exe) = resolve_symlink(&exe_to_resolve) {
                if exe == executable {
                    // Yes, this is the current version
                    version_is_current = true;
                    symlinks.push(exe.clone());
                    symlinks.push(PathBuf::from(exe_to_resolve));
                }
            }
        }

        let prefix = executable.parent()?.parent()?;
        let version = version::from_header_files(prefix)?;

        // Also look for other python* files in the same directory as the above executable
        for exe in find_executables(executable.parent()?) {
            if symlinks.contains(&exe) {
                continue;
            }
            if let Some(symlink) = resolve_symlink(&exe) {
                if symlinks.contains(&symlink) {
                    symlinks.push(exe);
                }
            }
        }

        // We know files in /usr/local/bin & /Library/Frameworks/Python.framework/Versions/Current/bin end up being symlinks to this python exe as well
        // Documented here https://docs.python.org/3/using/mac.html
        // Hence look for those symlinks as well.
        for bin in [
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/Library/Frameworks/Python.framework/Versions/Current/bin"),
        ] {
            for file in find_executables(&bin) {
                // If we're looking in the `Current/bin`, then no need to resolve symlinks
                // As we already know this is the current version.
                // Note: We can resolve the symlink for /Library/Frameworks/Python.framework/Versions/Current/bin/python3
                // However in rust for some reason we cannot resolve the symlink for /Library/Frameworks/Python.framework/Versions/Current/bin/python3.10
                if version_is_current
                    && file.starts_with("/Library/Frameworks/Python.framework/Versions/Current/bin")
                {
                    symlinks.push(file);
                    continue;
                }
                if let Some(symlink) = resolve_symlink(&file) {
                    if symlinks.contains(&symlink) {
                        symlinks.push(file);
                    }
                }
            }
        }

        symlinks.sort();
        symlinks.dedup();

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::MacPythonOrg))
                .executable(Some(executable.clone()))
                .version(Some(version))
                .prefix(Some(prefix.to_path_buf()))
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, reporter: &dyn Reporter) {
        if std::env::consts::OS != "macos" {
            return;
        }

        if let Ok(reader) = fs::read_dir("/Library/Frameworks/Python.framework/Versions/") {
            for file in reader.filter_map(Result::ok) {
                let prefix = file.path();
                // Ignore the `/Library/Frameworks/Python.framework/Versions/Current` folder, as this only contains symlinks to the actual python installations
                // We will account for the symlinks in these folder later
                if prefix
                    .to_string_lossy()
                    .starts_with("/Library/Frameworks/Python.framework/Versions/Current")
                {
                    continue;
                }

                let executable = prefix.join("bin").join("python3");
                let version = version::from_header_files(&prefix);

                if let Some(env) = self.try_from(&PythonEnv::new(executable, Some(prefix), version))
                {
                    reporter.report_environment(&env);
                }
            }
        }
    }
}
