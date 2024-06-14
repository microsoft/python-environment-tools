// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::env::PythonEnv;
use pet_python_utils::executable::{find_executables, get_shortest_executable};
use pet_python_utils::version;
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
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if std::env::consts::OS != "macos" {
            return None;
        }

        let mut executable = resolve_symlink(&env.executable).unwrap_or(env.executable.clone());
        if !executable
            .to_string_lossy()
            .starts_with("/Library/Frameworks/Python.framework/Versions/")
        {
            return None;
        }

        let mut symlinks = vec![executable.clone(), env.executable.clone()];
        if executable.starts_with("/Library/Frameworks/Python.framework/Versions/Current") {
            // This is a symlink to the python executable, lets resolve it
            if let Some(exe) = resolve_symlink(&env.executable) {
                if exe.starts_with("/Library/Frameworks/Python.framework/Versions")
                    && !exe.starts_with("/Library/Frameworks/Python.framework/Versions/Current")
                {
                    symlinks.push(exe.clone());
                    executable = exe;
                }
            }
        }

        let prefix = executable.parent()?.parent()?;
        let version = version::from_header_files(prefix)?;

        // We know files in /usr/local/bin & /Library/Frameworks/Python.framework/Versions/Current/bin end up being symlinks to this python exe as well
        // Documented here https://docs.python.org/3/using/mac.html
        // Hence look for those symlinks as well.
        for bin in [
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/Library/Frameworks/Python.framework/Versions/Current/bin"),
        ] {
            for file in find_executables(&bin) {
                if let Some(symlink) = resolve_symlink(&file) {
                    if symlinks.contains(&symlink) {
                        symlinks.push(file);
                    }
                }
            }
        }

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

        let user_friendly_exe =
            get_shortest_executable(&Some(symlinks.clone())).unwrap_or(env.executable.clone());

        symlinks.sort();
        symlinks.dedup();

        Some(
            PythonEnvironmentBuilder::new(PythonEnvironmentCategory::MacPythonOrg)
                .executable(Some(user_friendly_exe))
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

                if let Some(env) = self.from(&PythonEnv::new(executable, Some(prefix), version)) {
                    println!("MAC Python {:?}", env);
                    reporter.report_environment(&env);
                }
            }
        }
    }
}
