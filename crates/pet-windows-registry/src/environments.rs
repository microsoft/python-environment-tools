// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use pet_conda::CondaLocator;
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
pub fn get_registry_pythons(conda_locator: &Arc<dyn CondaLocator>) -> Option<LocatorResult> {
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
                            if let Some(result) = get_registry_pythons_from_key_for_company(
                                name,
                                &company_key,
                                &company,
                                conda_locator,
                            ) {
                                managers.extend(result.managers);
                                environments.extend(result.environments);
                            }
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
    Some(LocatorResult {
        environments,
        managers,
    })
}

#[cfg(windows)]
fn get_registry_pythons_from_key_for_company(
    key_container: &str,
    company_key: &RegKey,
    company: &str,
    conda_locator: &Arc<dyn CondaLocator>,
) -> Option<LocatorResult> {
    use log::{trace, warn};
    use pet_fs::path::norm_case;

    let mut managers: Vec<EnvManager> = vec![];
    let mut environments = vec![];
    // let company_display_name: Option<String> = company_key.get_value("DisplayName").ok();
    for installed_python in company_key.enum_keys().filter_map(Result::ok) {
        match company_key.open_subkey(installed_python.clone()) {
            Ok(installed_python_key) => {
                match installed_python_key.open_subkey("InstallPath") {
                    Ok(install_path_key) => {
                        let env_path: String =
                            install_path_key.get_value("").ok().unwrap_or_default();
                        let env_path = norm_case(&PathBuf::from(env_path));
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
                        if let Some(conda_result) = conda_locator.find_in(&env_path) {
                            for manager in conda_result.managers {
                                // let mgr = manager.clone();
                                // mgr.company = Some(company.to_string());
                                // mgr.company_display_name = company_display_name.clone();
                                managers.push(manager.clone())
                            }
                            for env in conda_result.environments {
                                // let env = env.clone();
                                // env.company = Some(company.to_string());
                                // env.company_display_name = company_display_name.clone();
                                // if let Some(mgr) = env.manager {
                                //     let mut mgr = mgr.clone();
                                //     // mgr.company = Some(company.to_string());
                                //     // mgr.company_display_name = company_display_name.clone();
                                //     env.manager = Some(mgr);
                                // }
                                environments.push(env.clone());
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
                        let executable = norm_case(&PathBuf::from(executable));
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

                        let env =
                            PythonEnvironmentBuilder::new(PythonEnvironmentKind::WindowsRegistry)
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
                        // env.company = Some(company.to_string());
                        // env.company_display_name = company_display_name.clone();
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

    Some(LocatorResult {
        environments,
        managers,
    })
}
