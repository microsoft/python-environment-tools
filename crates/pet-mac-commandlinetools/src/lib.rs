// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::version;
use pet_python_utils::{
    env::{PythonEnv, ResolvedPythonEnv},
    executable::find_executables,
};
use pet_virtualenv::is_virtualenv;
use std::path::PathBuf;

pub struct MacCmdLineTools {}

impl MacCmdLineTools {
    pub fn new() -> MacCmdLineTools {
        MacCmdLineTools {}
    }
}
impl Default for MacCmdLineTools {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for MacCmdLineTools {
    fn get_name(&self) -> &'static str {
        "MacCmdLineTools"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::MacCommandLineTools]
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

        if !env
            .executable
            .starts_with("/Library/Developer/CommandLineTools/usr/bin")
            && !env.executable.starts_with(
                "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions",
            )
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

        // Possible we got the file /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9
        // We know that /Library/Developer/CommandLineTools/usr/bin/python3 is a symlink to the above.
        if env
            .executable
            .starts_with("/Library/Developer/CommandLineTools/usr/bin")
        {
            let exe = PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/python3");
            if let Some(symlink) = resolve_symlink(&exe) {
                if symlinks.contains(&symlink) {
                    symlinks.push(symlink);

                    // Rest of the files in this directory are also symlinks to the same exe.
                    for exe in find_executables(PathBuf::from(
                        "/Library/Developer/CommandLineTools/usr/bin",
                    )) {
                        if !symlinks.contains(&exe) {
                            if let Some(symlink) = resolve_symlink(&exe) {
                                if symlinks.contains(&symlink) {
                                    symlinks.push(exe);
                                }
                            }
                        }
                    }
                }
            }
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

        // Find other exes that are symlinks to the same exe in /Library/Developer/CommandLineTools/usr/bin
        for exe in find_executables("/Library/Developer/CommandLineTools/usr/bin") {
            if !symlinks.contains(&exe) {
                if let Some(symlink) = resolve_symlink(&exe) {
                    if symlinks.contains(&symlink) {
                        symlinks.push(exe);
                    }
                }
            }
        }

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

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::MacCommandLineTools))
                .executable(Some(env.executable.clone()))
                .version(version)
                .prefix(prefix)
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, _reporter: &dyn Reporter) {
        // We will end up looking in current PATH variable
        // Given thats done else where, lets not repeat it here.
        if std::env::consts::OS != "macos" {
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
            if let Some(env) = self.try_from(&env) {
                _reporter.report_environment(&env);
            }
        }
    }
}
