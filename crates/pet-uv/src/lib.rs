// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_conda::utils::is_conda_env;
use pet_fs::path::{expand_path, norm_case};
use std::{fs, path::PathBuf};

/// Get the UV cache directory.
/// UV uses the following priority order:
/// 1. UV_CACHE_DIR environment variable
/// 2. XDG cache directories on Unix / %LOCALAPPDATA% on Windows
/// 3. Platform-specific cache directories
fn get_uv_cache_dir(
    uv_cache_dir_env_var: Option<String>,
    xdg_cache_home: Option<String>,
    user_home: Option<PathBuf>,
) -> Option<PathBuf> {
    // 1. Check UV_CACHE_DIR environment variable
    if let Some(cache_dir) = uv_cache_dir_env_var {
        let cache_dir = norm_case(expand_path(PathBuf::from(cache_dir)));
        if cache_dir.exists() {
            return Some(cache_dir);
        }
    }

    // 2. Check XDG_CACHE_HOME on Unix
    if let Some(xdg_cache) = xdg_cache_home.map(|d| PathBuf::from(d).join("uv")) {
        if xdg_cache.exists() {
            return Some(xdg_cache);
        }
    }

    // 3. Platform-specific cache directories
    if let Some(home) = user_home {
        let cache_dirs = if cfg!(target_os = "windows") {
            // On Windows: %LOCALAPPDATA%\uv
            vec![home.join("AppData").join("Local").join("uv")]
        } else if cfg!(target_os = "macos") {
            // On macOS: ~/Library/Caches/uv
            vec![home.join("Library").join("Caches").join("uv")]
        } else {
            // On other Unix systems: ~/.cache/uv
            vec![home.join(".cache").join("uv")]
        };

        for cache_dir in cache_dirs {
            if cache_dir.exists() {
                return Some(cache_dir);
            }
        }
    }

    None
}

/// Get UV environment cache directories.
/// UV stores virtual environments in {cache_dir}/environments-v2/
fn get_uv_environment_dirs(
    uv_cache_dir_env_var: Option<String>,
    xdg_cache_home: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut env_dirs = Vec::new();

    if let Some(cache_dir) = get_uv_cache_dir(uv_cache_dir_env_var, xdg_cache_home, user_home) {
        let environments_dir = cache_dir.join("environments-v2");
        if environments_dir.exists() {
            env_dirs.push(environments_dir);
        }
    }

    env_dirs
}

/// List UV virtual environment paths.
/// This function discovers UV cache directories and enumerates the virtual environments within them.
/// It filters out conda environments to avoid conflicts.
pub fn list_uv_virtual_envs_paths(
    uv_cache_dir_env_var: Option<String>,
    xdg_cache_home: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut python_envs: Vec<PathBuf> = vec![];

    for env_cache_dir in get_uv_environment_dirs(uv_cache_dir_env_var, xdg_cache_home, user_home) {
        if let Ok(dirs) = fs::read_dir(&env_cache_dir) {
            python_envs.append(
                &mut dirs
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && !is_conda_env(p))
                    .collect(),
            );
        }
    }

    python_envs.sort();
    python_envs.dedup();

    python_envs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_uv_cache_dir_from_env_var() {
        let temp_dir = std::env::temp_dir().join("test_uv_cache");
        fs::create_dir_all(&temp_dir).unwrap();

        let cache_dir = get_uv_cache_dir(
            Some(temp_dir.to_string_lossy().to_string()),
            None,
            None,
        );

        assert_eq!(cache_dir, Some(temp_dir.clone()));
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_uv_environment_dirs() {
        let temp_dir = std::env::temp_dir().join("test_uv_env");
        let env_dir = temp_dir.join("environments-v2");
        fs::create_dir_all(&env_dir).unwrap();

        let env_dirs = get_uv_environment_dirs(
            Some(temp_dir.to_string_lossy().to_string()),
            None,
            None,
        );

        assert_eq!(env_dirs, vec![env_dir.clone()]);
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_list_uv_virtual_envs_paths() {
        let temp_dir = std::env::temp_dir().join("test_uv_list");
        let env_dir = temp_dir.join("environments-v2");
        let test_env = env_dir.join("test-venv");
        fs::create_dir_all(&test_env).unwrap();

        let envs = list_uv_virtual_envs_paths(
            Some(temp_dir.to_string_lossy().to_string()),
            None,
            None,
        );

        assert!(envs.contains(&test_env));
        fs::remove_dir_all(&temp_dir).ok();
    }
}