// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_python_utils::version;
use pet_python_utils::{env::PythonEnv, executable::find_executables};
use std::fs;

pub fn is_virtualenv(env: &PythonEnv) -> bool {
    if env.prefix.is_none() {
        let mut bin = env.executable.clone();
        bin.pop();
        // Check if the executable is in a bin or Scripts directory.
        // Possible for some reason we do not have the prefix.
        if !bin.ends_with("bin") && !bin.ends_with("Scripts") {
            return false;
        }
    }
    if let Some(bin) = env.executable.parent() {
        // Check if there are any activate.* files in the same directory as the interpreter.
        //
        // env
        // |__ activate, activate.*  <--- check if any of these files exist
        // |__ python  <--- interpreterPath

        // if let Some(parent_path) = PathBuf::from(env.)
        // const directory = path.dirname(interpreterPath);
        // const files = await fsapi.readdir(directory);
        // const regex = /^activate(\.([A-z]|\d)+)?$/i;
        if fs::metadata(bin.join("activate")).is_ok()
            || fs::metadata(bin.join("activate.bat")).is_ok()
        {
            return true;
        }

        // Support for activate.ps, etc.
        if let Ok(files) = std::fs::read_dir(bin) {
            for file in files.filter_map(Result::ok).map(|e| e.path()) {
                if file
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .starts_with("activate")
                {
                    return true;
                }
            }
            return false;
        }
    }

    false
}

pub struct VirtualEnv {}

impl VirtualEnv {
    pub fn new() -> VirtualEnv {
        VirtualEnv {}
    }
}
impl Default for VirtualEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Locator for VirtualEnv {
    fn get_name(&self) -> &'static str {
        "VirtualEnv"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::VirtualEnv]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if is_virtualenv(env) {
            let version = match env.version {
                Some(ref v) => Some(v.clone()),
                None => match &env.prefix {
                    Some(prefix) => version::from_creator_for_virtual_env(prefix),
                    None => None,
                },
            };
            let mut symlinks = vec![];
            if let Some(ref prefix) = env.prefix {
                symlinks.append(&mut find_executables(prefix));
            }
            Some(
                PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::VirtualEnv))
                    .executable(Some(env.executable.clone()))
                    .version(version)
                    .prefix(env.prefix.clone())
                    .symlinks(Some(symlinks))
                    .build(),
            )
        } else {
            None
        }
    }

    fn find(&self, _reporter: &dyn Reporter) {
        // There are no common global locations for virtual environments.
        // We expect the user of this class to call `is_compatible`
    }
}
