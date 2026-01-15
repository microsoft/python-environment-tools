// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{info, trace};
use pet_conda::Conda;
use pet_core::arch::Architecture;
use pet_core::env::PythonEnv;
use pet_core::os_environment::Environment;
use pet_core::python_environment::{
    PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind,
};
use pet_core::Locator;
use pet_linux_global_python::LinuxGlobalPython;
use pet_mac_commandlinetools::MacCmdLineTools;
use pet_mac_python_org::MacPythonOrg;
use pet_mac_xcode::MacXCode;
use pet_pipenv::PipEnv;
use pet_pixi::Pixi;
use pet_poetry::Poetry;
use pet_pyenv::PyEnv;
use pet_python_utils::env::ResolvedPythonEnv;
use pet_uv::Uv;
use pet_venv::Venv;
use pet_virtualenv::VirtualEnv;
use pet_virtualenvwrapper::VirtualEnvWrapper;
use std::path::PathBuf;
use std::sync::Arc;

pub fn create_locators(
    conda_locator: Arc<Conda>,
    poetry_locator: Arc<Poetry>,
    environment: &dyn Environment,
) -> Arc<Vec<Arc<dyn Locator>>> {
    // NOTE: The order of the items matter.

    let mut locators: Vec<Arc<dyn Locator>> = vec![];

    // 1. Windows store Python
    // 2. Windows registry python
    if cfg!(windows) {
        #[cfg(windows)]
        use pet_windows_registry::WindowsRegistry;
        #[cfg(windows)]
        use pet_windows_store::WindowsStore;
        #[cfg(windows)]
        locators.push(Arc::new(WindowsStore::from(environment)));
        #[cfg(windows)]
        locators.push(Arc::new(WindowsRegistry::from(conda_locator.clone())))
    }
    // 3. Pyenv Python
    locators.push(Arc::new(PyEnv::from(environment, conda_locator.clone())));

    // 4. Pixi
    locators.push(Arc::new(Pixi::new()));

    // 5. Conda Python
    locators.push(conda_locator);

    // 6. Support for Virtual Envs
    // The order of these matter.
    // Basically PipEnv is a superset of VirtualEnvWrapper, which is a superset of Venv, which is a superset of VirtualEnv.
    locators.push(Arc::new(Uv::new()));
    locators.push(poetry_locator);
    locators.push(Arc::new(PipEnv::from(environment)));
    locators.push(Arc::new(VirtualEnvWrapper::from(environment)));
    locators.push(Arc::new(Venv::new()));
    // VirtualEnv is the most generic, hence should be the last.
    locators.push(Arc::new(VirtualEnv::new()));

    // 7. Homebrew Python
    if cfg!(unix) {
        #[cfg(unix)]
        use pet_homebrew::Homebrew;
        #[cfg(unix)]
        let homebrew_locator = Homebrew::from(environment);
        #[cfg(unix)]
        locators.push(Arc::new(homebrew_locator));
    }

    // 8. Global Mac Python
    // 9. CommandLineTools Python & xcode
    if std::env::consts::OS == "macos" {
        locators.push(Arc::new(MacXCode::new()));
        locators.push(Arc::new(MacCmdLineTools::new()));
        locators.push(Arc::new(MacPythonOrg::new()));
    }
    // 10. Global Linux Python
    // All other Linux (not mac, & not windows)
    // THIS MUST BE LAST
    if std::env::consts::OS != "macos" && std::env::consts::OS != "windows" {
        locators.push(Arc::new(LinuxGlobalPython::new()))
    }
    Arc::new(locators)
}

/// Identify the Python environment using the locators.
/// search_path : Generally refers to original folder that was being searched when the env was found.
pub fn identify_python_environment_using_locators(
    env: &PythonEnv,
    locators: &[Arc<dyn Locator>],
    global_env_search_paths: &[PathBuf],
) -> Option<PythonEnvironment> {
    let executable = env.executable.clone();
    trace!(
        "Identifying Python environment using locators: {:?}",
        executable
    );
    if let Some(env) = locators.iter().fold(
        None,
        |e, loc| if e.is_some() { e } else { loc.try_from(env) },
    ) {
        return Some(env);
    }
    trace!(
        "Failed to identify Python environment using locators, now trying to resolve: {:?}",
        executable
    );

    // Yikes, we have no idea what this is.
    // Lets get the actual interpreter info and try to figure this out.
    // We try to get the interpreter info, hoping that the real exe returned might be identifiable.
    if let Some(resolved_env) = ResolvedPythonEnv::from(&executable) {
        let env = resolved_env.to_python_env();
        if let Some(env) =
            locators.iter().fold(
                None,
                |e, loc| if e.is_some() { e } else { loc.try_from(&env) },
            )
        {
            trace!("Env ({:?}) in Path resolved as {:?}", executable, env.kind);
            // TODO: Telemetry point.
            // As we had to spawn earlier.
            return Some(env);
        } else {
            // We have no idea what this is.
            // We have check all of the resolvers.
            // Telemetry point, failed to identify env here.
            let mut fallback_kind = None;

            // If one of the symlinks are in the PATH variable, then we can treat this as a GlobalPath kind.
            let symlinks = [
                resolved_env.symlinks.clone().unwrap_or_default(),
                vec![resolved_env.executable.clone(), executable.clone()],
            ]
            .concat();
            for symlink in symlinks {
                if let Some(bin) = symlink.parent() {
                    if global_env_search_paths.contains(&bin.to_path_buf()) {
                        fallback_kind = Some(PythonEnvironmentKind::GlobalPaths);
                        break;
                    }
                }
            }
            info!(
                "Env ({:?}) in Path resolved as {:?} and reported as {:?}",
                executable, resolved_env, fallback_kind
            );
            return Some(create_unknown_env(resolved_env, fallback_kind));
        }
    }
    None
}

fn create_unknown_env(
    resolved_env: ResolvedPythonEnv,
    fallback_category: Option<PythonEnvironmentKind>,
) -> PythonEnvironment {
    // Combine symlinks from resolved_env (which includes the original executable path
    // and the resolved path) with any additional symlinks found in the bin directory.
    // This is important on Windows where scoop and similar tools use shim executables
    // that redirect to the real Python installation.
    let mut symlinks = resolved_env.symlinks.clone().unwrap_or_default();
    if let Some(additional_symlinks) = find_symlinks(&resolved_env.executable) {
        for symlink in additional_symlinks {
            if !symlinks.contains(&symlink) {
                symlinks.push(symlink);
            }
        }
    }
    symlinks.sort();
    symlinks.dedup();

    PythonEnvironmentBuilder::new(fallback_category)
        .symlinks(Some(symlinks))
        .executable(Some(resolved_env.executable))
        .prefix(Some(resolved_env.prefix))
        .arch(Some(if resolved_env.is64_bit {
            Architecture::X64
        } else {
            Architecture::X86
        }))
        .version(Some(resolved_env.version))
        .build()
}

#[cfg(unix)]
fn find_symlinks(executable: &PathBuf) -> Option<Vec<PathBuf>> {
    // Assume this is a python environment in /usr/bin/python.
    // Now we know there can be other exes in the same directory as well, such as /usr/bin/python3.12 and that could be the same as /usr/bin/python
    // However its possible /usr/bin/python is a symlink to /usr/local/bin/python3.12
    // Either way, if both /usr/bin/python and /usr/bin/python3.12 point to the same exe (what ever it may be),
    // then we know that both /usr/bin/python and /usr/bin/python3.12 are the same python environment.
    // We use canonicalize to get the real path of the symlink.
    // Only used in this case, see notes for resolve_symlink.

    use pet_fs::path::resolve_symlink;
    use pet_python_utils::executable::find_executables;
    use std::fs;

    let real_exe = resolve_symlink(executable).or(fs::canonicalize(executable).ok());

    let bin = executable.parent()?;
    // Make no assumptions that bin is always where exes are in linux
    // No harm in supporting scripts as well.
    if !bin.ends_with("bin") && !bin.ends_with("Scripts") && !bin.ends_with("scripts") {
        return None;
    }

    let mut symlinks = vec![];
    for exe in find_executables(bin) {
        let symlink = resolve_symlink(&exe).or(fs::canonicalize(&exe).ok());
        if symlink == real_exe {
            symlinks.push(exe);
        }
    }
    Some(symlinks)
}

#[cfg(windows)]
#[allow(clippy::ptr_arg)]
fn find_symlinks(_executable: &PathBuf) -> Option<Vec<PathBuf>> {
    // In windows we will need to spawn the Python exe and then get the exes.
    // Lets wait and see if this is necessary.
    None
}
