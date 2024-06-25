// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{trace, warn};
use pet_conda::Conda;
use pet_core::arch::Architecture;
use pet_core::os_environment::EnvironmentApi;
use pet_core::python_environment::{
    PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory,
};
use pet_core::Locator;
use pet_linux_global_python::LinuxGlobalPython;
use pet_mac_commandlinetools::MacCmdLineTools;
use pet_mac_python_org::MacPythonOrg;
use pet_mac_xcode::MacXCode;
use pet_pipenv::PipEnv;
use pet_poetry::Poetry;
use pet_pyenv::PyEnv;
use pet_python_utils::env::{PythonEnv, ResolvedPythonEnv};
use pet_venv::Venv;
use pet_virtualenv::VirtualEnv;
use pet_virtualenvwrapper::VirtualEnvWrapper;
use std::sync::Arc;

pub fn create_locators(conda_locator: Arc<Conda>) -> Arc<Vec<Arc<dyn Locator>>> {
    // NOTE: The order of the items matter.
    let environment = EnvironmentApi::new();

    let mut locators: Vec<Arc<dyn Locator>> = vec![];

    // 1. Windows store Python
    // 2. Windows registry python
    if cfg!(windows) {
        #[cfg(windows)]
        use pet_windows_registry::WindowsRegistry;
        #[cfg(windows)]
        use pet_windows_store::WindowsStore;
        #[cfg(windows)]
        locators.push(Arc::new(WindowsStore::from(&environment)));
        #[cfg(windows)]
        locators.push(Arc::new(WindowsRegistry::from(conda_locator.clone())))
    }
    // 3. Pyenv Python
    locators.push(Arc::new(PyEnv::from(&environment, conda_locator.clone())));
    // 4. Homebrew Python
    if cfg!(unix) {
        #[cfg(unix)]
        use pet_homebrew::Homebrew;
        #[cfg(unix)]
        let homebrew_locator = Homebrew::from(&environment);
        #[cfg(unix)]
        locators.push(Arc::new(homebrew_locator));
    }
    // 5. Conda Python
    locators.push(conda_locator);
    // 6. Support for Virtual Envs
    // The order of these matter.
    // Basically PipEnv is a superset of VirtualEnvWrapper, which is a superset of Venv, which is a superset of VirtualEnv.
    locators.push(Arc::new(Poetry::from(&environment)));
    locators.push(Arc::new(PipEnv::from(&environment)));
    locators.push(Arc::new(VirtualEnvWrapper::from(&environment)));
    locators.push(Arc::new(Venv::new()));
    // VirtualEnv is the most generic, hence should be the last.
    locators.push(Arc::new(VirtualEnv::new()));

    // 7. Global Mac Python
    // 8. CommandLineTools Python & xcode
    if std::env::consts::OS == "macos" {
        locators.push(Arc::new(MacXCode::new()));
        locators.push(Arc::new(MacCmdLineTools::new()));
        locators.push(Arc::new(MacPythonOrg::new()));
    }
    // 9. Global Linux Python
    // All other Linux (not mac, & not windows)
    // THIS MUST BE LAST
    if std::env::consts::OS != "macos" && std::env::consts::OS != "windows" {
        locators.push(Arc::new(LinuxGlobalPython::new()))
    }
    Arc::new(locators)
}

pub fn identify_python_environment_using_locators(
    env: &PythonEnv,
    locators: &[Arc<dyn Locator>],
    fallback_category: Option<PythonEnvironmentCategory>,
) -> Option<PythonEnvironment> {
    let executable = env.executable.clone();
    if let Some(env) = locators
        .iter()
        .fold(None, |e, loc| if e.is_some() { e } else { loc.from(env) })
    {
        return Some(env);
    }

    // Yikes, we have no idea what this is.
    // Lets get the actual interpreter info and try to figure this out.
    // We try to get the interpreter info, hoping that the real exe returned might be identifiable.
    if let Some(resolved_env) = ResolvedPythonEnv::from(&executable) {
        let env = resolved_env.to_python_env();
        if let Some(env) = locators
            .iter()
            .fold(None, |e, loc| if e.is_some() { e } else { loc.from(&env) })
        {
            trace!(
                "Unknown Env ({:?}) in Path resolved as {:?}",
                executable,
                env.category
            );
            // TODO: Telemetry point.
            // As we had to spawn earlier.
            return Some(env);
        } else {
            // We have no idea what this is.
            // We have check all of the resolvers.
            // Telemetry point, failed to identify env here.
            warn!(
                "Unknown Env ({:?}) in Path resolved as {:?} and reported as Unknown",
                executable, resolved_env
            );
            let env = PythonEnvironmentBuilder::new(
                fallback_category.unwrap_or(PythonEnvironmentCategory::Unknown),
            )
            .executable(Some(resolved_env.executable))
            .prefix(Some(resolved_env.prefix))
            .arch(Some(if resolved_env.is64_bit {
                Architecture::X64
            } else {
                Architecture::X86
            }))
            .version(Some(resolved_env.version))
            .build();
            return Some(env);
        }
    }
    None
}
