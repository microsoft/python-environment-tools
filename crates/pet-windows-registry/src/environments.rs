// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use pet_conda::CondaLocator;
#[cfg(windows)]
use pet_core::reporter::Reporter;
#[cfg(windows)]
use pet_core::{
    arch::Architecture,
    manager::EnvManager,
    python_environment::{PythonEnvironmentBuilder, PythonEnvironmentKind},
    LocatorResult,
};
#[cfg(windows)]
use pet_windows_store::is_windows_app_folder_in_program_files;
#[cfg(windows)]
use std::{path::PathBuf, sync::Arc};
#[cfg(windows)]
use winreg::RegKey;

#[cfg(windows)]
pub fn get_registry_pythons(
    conda_locator: &Arc<dyn CondaLocator>,
    reporter: &Option<&dyn Reporter>,
) -> LocatorResult {
    use log::{trace, warn};

    let mut environments = vec![];
    let mut managers: Vec<EnvManager> = vec![];

    struct RegistryKey {
        pub name: &'static str,
        pub key: winreg::RegKey,
    }
    let search_keys = [
        RegistryKey {
            name: "HKLM",
            key: winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE),
        },
        RegistryKey {
            name: "HKCU",
            key: winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER),
        },
    ];
    for (name, key) in search_keys.iter().map(|f| (f.name, &f.key)) {
        match key.open_subkey("Software\\Python") {
            Ok(python_key) => {
                for company in python_key.enum_keys().filter_map(Result::ok) {
                    trace!("Searching {}\\Software\\Python\\{}", name, company);
                    match python_key.open_subkey(&company) {
                        Ok(company_key) => {
                            let result = get_registry_pythons_from_key_for_company(
                                name,
                                &company_key,
                                &company,
                                conda_locator,
                                reporter,
                            );
                            managers.extend(result.managers);
                            environments.extend(result.environments);
                        }
                        Err(err) => {
                            warn!(
                                "Failed to open {}\\Software\\Python\\{}, {:?}",
                                name, company, err
                            );
                        }
                    }
                }
            }
            Err(err) => {
                warn!("Failed to open {}\\Software\\Python, {:?}", name, err)
            }
        }
    }
    LocatorResult {
        environments,
        managers,
    }
}

#[cfg(windows)]
fn get_registry_pythons_from_key_for_company(
    key_container: &str,
    company_key: &RegKey,
    company: &str,
    conda_locator: &Arc<dyn CondaLocator>,
    reporter: &Option<&dyn Reporter>,
) -> LocatorResult {
    use log::{trace, warn};
    use pet_conda::utils::is_conda_env;
    use pet_fs::path::norm_case;

    let mut environments = vec![];
    // let company_display_name: Option<String> = company_key.get_value("DisplayName").ok();
    for installed_python in company_key.enum_keys().filter_map(Result::ok) {
        match company_key.open_subkey(installed_python.clone()) {
            Ok(installed_python_key) => {
                match installed_python_key.open_subkey("InstallPath") {
                    Ok(install_path_key) => {
                        let env_path: String =
                            install_path_key.get_value("").ok().unwrap_or_default();
                        if env_path.is_empty() {
                            warn!(
                                "Install path is empty {}\\Software\\Python\\{}\\{}",
                                key_container, company, installed_python
                            );
                            continue;
                        }
                        let env_path = norm_case(PathBuf::from(env_path));
                        if is_windows_app_folder_in_program_files(&env_path) {
                            trace!(
                                "Found Python ({}) in {}\\Software\\Python\\{}\\{}, but skipping as this is a Windows Store Python",
                                env_path.to_str().unwrap_or_default(),
                                key_container,
                                company,
                                installed_python,
                            );
                            continue;
                        }
                        trace!(
                            "Found Python ({}) in {}\\Software\\Python\\{}\\{}",
                            env_path.to_str().unwrap_or_default(),
                            key_container,
                            company,
                            installed_python,
                        );

                        // Possible this is a conda install folder.
                        if is_conda_env(&env_path) {
                            if let Some(reporter) = reporter {
                                conda_locator.find_and_report(*reporter, &env_path);
                            }
                            continue;
                        }

                        let env_path = if env_path.exists() {
                            Some(env_path)
                        } else {
                            None
                        };
                        let executable: String = install_path_key
                            .get_value("ExecutablePath")
                            .ok()
                            .unwrap_or_default();
                        if executable.is_empty() {
                            warn!(
                                "Executable is empty {}\\Software\\Python\\{}\\{}\\ExecutablePath",
                                key_container, company, installed_python
                            );
                            continue;
                        }
                        let executable = norm_case(PathBuf::from(executable));
                        if !executable.exists() {
                            warn!(
                                "Python executable ({}) file not found for {}\\Software\\Python\\{}\\{}",
                                executable.to_str().unwrap_or_default(),
                                key_container,
                                company,
                                installed_python
                            );
                            continue;
                        }
                        let version: String = installed_python_key
                            .get_value("Version")
                            .ok()
                            .unwrap_or_default();
                        let architecture: String = installed_python_key
                            .get_value("SysArchitecture")
                            .ok()
                            .unwrap_or_default();
                        let display_name: String = installed_python_key
                            .get_value("DisplayName")
                            .ok()
                            .unwrap_or_default();

                        let env = PythonEnvironmentBuilder::new(Some(
                            PythonEnvironmentKind::WindowsRegistry,
                        ))
                        .display_name(Some(display_name))
                        .executable(Some(executable.clone()))
                        .version(if version.is_empty() {
                            None
                        } else {
                            Some(version)
                        })
                        .prefix(env_path)
                        .arch(if architecture.contains("32") {
                            Some(Architecture::X86)
                        } else if architecture.contains("64") {
                            Some(Architecture::X64)
                        } else {
                            None
                        })
                        .build();

                        if let Some(reporter) = reporter {
                            reporter.report_environment(&env);
                        }
                        environments.push(env);
                    }
                    Err(err) => {
                        warn!(
                            "Failed to open {}\\Software\\Python\\{}\\{}\\InstallPath, {:?}",
                            key_container, company, installed_python, err
                        );
                    }
                }
            }
            Err(err) => {
                warn!(
                    "Failed to open {}\\Software\\Python\\{}\\{}, {:?}",
                    key_container, company, installed_python, err
                );
            }
        }
    }

    LocatorResult {
        environments,
        managers: vec![],
    }
}
