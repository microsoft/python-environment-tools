// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::get_homebrew_prefix_bin;
use environments::get_python_info;
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::resolve_symlink;
use pet_python_utils::{env::PythonEnv, executable::find_executables};
use pet_virtualenv::is_virtualenv;
use std::{path::PathBuf, thread};

mod env_variables;
mod environment_locations;
mod environments;
mod sym_links;

pub struct Homebrew {
    environment: EnvVariables,
}

impl Homebrew {
    pub fn from(environment: &dyn Environment) -> Homebrew {
        Homebrew {
            environment: EnvVariables::from(environment),
        }
    }
}

/// Deafult prefix paths for Homebrew
/// Below are from the docs `man brew`      Display Homebrew’s install path. Default:
/// - macOS ARM: /opt/homebrew
/// - macOS Intel: /usr/local
/// - Linux: /home/linuxbrew/.linuxbrew
fn from(env: &PythonEnv) -> Option<PythonEnvironment> {
    // Assume we create a virtual env from a homebrew python install,
    // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
    // Hence the first part of the condition will be true, but the second part will be false.
    if is_virtualenv(env) {
        return None;
    }

    // Note: Sometimes if Python 3.10 was installed by other means (e.g. from python.org or other)
    // & then you install Python 3.10 via Homebrew, then some files will get installed via homebrew,
    // However everything (symlinks, Python executable `sys.executable`, `sys.prefix`) eventually point back to the existing installation.
    // Thus we do not end up with two versions of python 3.10, i.e. the existing installation is not overwritten nor duplicated.
    // Hence we never end up reporting 3.10 for home brew (as mentioned when you try to resolve the exe it points to existing install, now homebrew).
    let exe = env.executable.clone();
    let exe_file_name = exe.file_name()?;
    let resolved_file = resolve_symlink(&exe).unwrap_or(exe.clone());
    // Cellar is where the executables will be installed, see below link
    // https://docs.brew.sh/Formula-Cookbook#an-introduction
    // From above link > Homebrew installs formulae to the Cellar at $(brew --cellar)
    // and then symlinks some of the installation into the prefix at $(brew --prefix) (e.g. /opt/homebrew) so that other programs can see what’s going on.
    // Hence look in `Cellar` directory
    if resolved_file.starts_with("/opt/homebrew/Cellar") {
        // Symlink  - /opt/homebrew/bin/python3.12
        // Symlink  - /opt/homebrew/opt/python3/bin/python3.12
        // Symlink  - /opt/homebrew/opt/python@3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Cellar/python@3.12/3.12.3/bin/python3.12
        // Real exe - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // Symlink  - /opt/homebrew/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // Symlink  - /opt/homebrew/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // SysPrefix- /opt/homebrew/opt/python@3.12/Frameworks/Python.framework/Versions/3.12
        get_python_info(
            &PathBuf::from("/opt/homebrew/bin").join(exe_file_name),
            &resolved_file,
        )
    } else if resolved_file.starts_with("/home/linuxbrew/.linuxbrew") {
        // Symlink  - /home/linuxbrew/.linuxbrew/bin/python3.12
        // Symlink  - /home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3.12
        // Real exe - /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3/bin/python3.12
        // SysPrefix- /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3

        get_python_info(
            &PathBuf::from("/home/linuxbrew/.linuxbrew/bin").join(exe_file_name),
            &resolved_file,
        )
    } else if resolved_file.starts_with("/usr/local/Cellar") {
        // Symlink  - /usr/local/bin/python3.8
        // Symlink  - /usr/local/opt/python@3.8/bin/python3.8
        // Symlink  - /usr/local/Cellar/python@3.8/3.8.19/bin/python3.8
        // Real exe - /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8
        // SysPrefix- /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8
        get_python_info(
            &PathBuf::from("/usr/local/bin").join(exe_file_name),
            &resolved_file,
        )
    } else {
        None
    }
}

impl Locator for Homebrew {
    fn get_name(&self) -> &'static str {
        "Homebrew"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentCategory> {
        vec![PythonEnvironmentCategory::Homebrew]
    }
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        from(env)
    }

    fn find(&self, reporter: &dyn Reporter) {
        let homebrew_prefix_bins = get_homebrew_prefix_bin(&self.environment);
        thread::scope(|s| {
            for homebrew_prefix_bin in &homebrew_prefix_bins {
                let homebrew_python_exes = find_executables(homebrew_prefix_bin);
                for file in homebrew_python_exes.iter().filter(|f| {
                    let file_name = f
                        .file_name()
                        .unwrap_or_default()
                        .to_str()
                        .unwrap_or_default()
                        .to_lowercase();
                    file_name.starts_with("python")
                    // If this file name is `python3`, then ignore this for now.
                    // We would prefer to use `python3.x` instead of `python3`.
                    // That way its more consistent and future proof
                        && file_name != "python3"
                        && file_name != "python"
                }) {
                    let file = file.clone();
                    s.spawn(move || {
                        // Sometimes we end up with other python installs in the Homebrew bin directory.
                        // E.g. /usr/local/bin is treated as a location where homebrew can be found (homebrew bin)
                        // However this is a very generic location, and we might end up with other python installs here.
                        // Hence call `resolve` to correctly identify homebrew python installs.
                        let env_to_resolve = PythonEnv::new(file.clone(), None, None);
                        if let Some(env) = from(&env_to_resolve) {
                            reporter.report_environment(&env);
                        }
                    });
                }
            }
        });
    }
}
