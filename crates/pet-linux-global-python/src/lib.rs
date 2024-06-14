// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::executable::get_shortest_executable;
use pet_python_utils::version;
use pet_python_utils::{
    env::{PythonEnv, ResolvedPythonEnv},
    executable::find_executables,
};
use std::path::PathBuf;

pub struct LinuxGlobalPython {}

impl LinuxGlobalPython {
    pub fn new() -> LinuxGlobalPython {
        LinuxGlobalPython {}
    }
}
impl Default for LinuxGlobalPython {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for LinuxGlobalPython {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if std::env::consts::OS == "macos" || std::env::consts::OS == "windows" {
            return None;
        }

        if !env
            .executable
            .to_string_lossy()
            .starts_with("/Library/Developer/CommandLineTools/usr/bin/python")
        {
            return None;
        }

        let mut version = env.version.clone();
        let mut prefix = env.prefix.clone();
        let mut symlinks = vec![env.executable.clone()];

        let existing_symlinks = env.symlinks.clone();
        if let Some(existing_symlinks) = existing_symlinks {
            symlinks.append(&mut existing_symlinks.clone());
        }

        // We know that /Library/Developer/CommandLineTools/usr/bin/python3 is actually a symlink to
        // /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9
        // Verify this and add that to the list of symlinks as well.
        if let Some(symlink) = resolve_symlink(&env.executable) {
            symlinks.push(symlink);
        }

        // We know /usr/bin/python3 can end up pointing to this same Python exe as well
        // Hence look for those symlinks as well.
        // Unfortunately /usr/bin/python3 is not a real symlink
        // Hence we must spawn and verify it points to the same Python exe.
        for possible_exes in [PathBuf::from("/usr/bin/python3")] {
            if !symlinks.contains(&possible_exes) {
                if let Some(resolved_env) = ResolvedPythonEnv::from(&possible_exes) {
                    if symlinks.contains(&resolved_env.executable) {
                        symlinks.push(possible_exes);
                        // Use the latest accurate information we have.
                        version = Some(resolved_env.version);
                        prefix = Some(resolved_env.prefix);
                    }
                }
            }
        }
        // Similarly the final exe can be /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9
        // & we might have another file `python3` in that bin directory which would point to the same exe.
        // Lets get those as well.
        if let Some(real_exe) = symlinks.iter().find(|s| {
            s.to_string_lossy()
                .contains("/Library/Developer/CommandLineTools/Library/Frameworks")
        }) {
            let python3 = real_exe.with_file_name("python3");
            if !symlinks.contains(&python3) {
                if let Some(symlink) = resolve_symlink(&python3) {
                    if symlinks.contains(&symlink) {
                        symlinks.push(python3);
                    }
                }
            }
        }

        symlinks.sort();
        symlinks.dedup();

        if prefix.is_none() {
            // We would have identified the symlinks by now.
            // Look for the one with the path `/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9`
            if let Some(symlink) = symlinks.iter().find(|s| {
                s.to_string_lossy().starts_with("/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions")
            }) {
                // Prefix is of the form `/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9`
                // The symlink would be the same, all we need is to remove the last 2 components (exe and bin directory).
                prefix = symlink.parent()?.parent().map(|p| p.to_path_buf());
            }
        }

        if version.is_none() {
            if let Some(prefix) = &prefix {
                version = version::from_header_files(prefix);
            }
        }
        if version.is_none() || prefix.is_none() {
            if let Some(resolved_env) = ResolvedPythonEnv::from(&env.executable) {
                version = Some(resolved_env.version);
                prefix = Some(resolved_env.prefix);
            }
        }
        let user_friendly_exe =
            get_shortest_executable(&Some(symlinks.clone())).unwrap_or(env.executable.clone());

        Some(
            PythonEnvironmentBuilder::new(PythonEnvironmentCategory::MacCommandLineTools)
                .executable(Some(user_friendly_exe))
                .version(version)
                .prefix(prefix)
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, _reporter: &dyn Reporter) {
        if std::env::consts::OS == "macos" || std::env::consts::OS == "windows" {
            return;
        }

        for exe in find_executables("/Library/Developer/CommandLineTools/usr")
            .iter()
            .filter(
                |f|                     // If this file name is `python3`, then ignore this for now.
            // We would prefer to use `python3.x` instead of `python3`.
            // That way its more consistent and future proof
                f.file_name().unwrap_or_default() != "python3" &&
                f.file_name().unwrap_or_default() != "python",
            )
        {
            // These files should end up being symlinks to something like /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9
            let mut env = PythonEnv::new(exe.to_owned(), None, None);
            let mut symlinks = vec![];
            if let Some(symlink) = resolve_symlink(exe) {
                // Symlinks must exist, they always point to something like the following
                // /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9
                symlinks.push(symlink);
            }

            // Also check whether the corresponding python and python3 files in this directory point to the same files.
            for python_exe in &["python", "python3"] {
                let python_exe = exe.with_file_name(python_exe);
                if let Some(symlink) = resolve_symlink(&python_exe) {
                    if symlinks.contains(&symlink) {
                        symlinks.push(python_exe);
                    }
                }
            }
            env.symlinks = Some(symlinks);
            if let Some(env) = self.from(&env) {
                _reporter.report_environment(&env);
            }
        }
    }
}
