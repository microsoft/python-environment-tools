// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace};
use pet_fs::path::norm_case;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::env::ResolvedPythonEnv;

type FilePathWithMTimeCTime = (PathBuf, SystemTime, SystemTime);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
    pub environment: ResolvedPythonEnv,
    pub symlinks: Vec<FilePathWithMTimeCTime>,
}

pub fn generate_cache_file(cache_directory: &Path, executable: &PathBuf) -> PathBuf {
    cache_directory.join(format!("{}.3.json", generate_hash(executable)))
}

pub fn delete_cache_file(cache_directory: &Path, executable: &PathBuf) {
    let cache_file = generate_cache_file(cache_directory, executable);
    let _ = fs::remove_file(cache_file);
}

pub fn get_cache_from_file(
    cache_directory: &Path,
    executable: &PathBuf,
) -> Option<(ResolvedPythonEnv, Vec<FilePathWithMTimeCTime>)> {
    let cache_file = generate_cache_file(cache_directory, executable);
    let file = File::open(cache_file.clone()).ok()?;
    let reader = BufReader::new(file);
    let cache: CacheEntry = serde_json::from_reader(reader).ok()?;
    // Account for conflicts in the cache file
    // i.e. the hash generated is same for another file, remember we only take the first 16 chars.
    if !cache
        .environment
        .clone()
        .symlinks
        .unwrap_or_default()
        .contains(executable)
    {
        trace!(
            "Cache file {:?} {:?}, does not match executable {:?} (possible hash collision)",
            cache_file,
            cache.environment,
            executable
        );
        return None;
    }

    // Check if any of the exes have changed since we last cached them.
    let cache_is_valid = cache.symlinks.iter().all(|symlink| {
        if let Ok(metadata) = symlink.0.metadata() {
            metadata.modified().ok() == Some(symlink.1)
                && metadata.created().ok() == Some(symlink.2)
        } else {
            // File may have been deleted.
            false
        }
    });

    if cache_is_valid {
        trace!("Using cache from {:?} for {:?}", cache_file, executable);
        Some((cache.environment, cache.symlinks))
    } else {
        let _ = fs::remove_file(cache_file);
        None
    }
}

pub fn store_cache_in_file(
    cache_directory: &Path,
    executable: &PathBuf,
    environment: &ResolvedPythonEnv,
    symlinks_with_times: Vec<FilePathWithMTimeCTime>,
) {
    let cache_file = generate_cache_file(cache_directory, executable);
    match std::fs::create_dir_all(cache_directory) {
        Ok(_) => {
            let cache = CacheEntry {
                environment: environment.clone(),
                symlinks: symlinks_with_times,
            };
            match std::fs::File::create(cache_file.clone()) {
                Ok(file) => {
                    trace!("Caching {:?} in {:?}", executable, cache_file);
                    match serde_json::to_writer_pretty(file, &cache) {
                        Ok(_) => (),
                        Err(err) => error!("Error writing cache file {:?} {:?}", cache_file, err),
                    }
                }
                Err(err) => error!("Error creating cache file {:?} {:?}", cache_file, err),
            }
        }
        Err(err) => error!(
            "Error creating cache directory {:?} {:?}",
            cache_directory, err
        ),
    }
}

fn generate_hash(executable: &PathBuf) -> String {
    let mut hasher = Sha256::new();
    hasher.update(norm_case(executable).to_string_lossy().as_bytes());
    let h_bytes = hasher.finalize();
    // Convert 256 bits => Hext and then take 16 of the hex chars (that should be unique enough)
    // We will handle collisions if they happen.
    format!("{:x}", h_bytes)[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_hash_generation() {
        assert_eq!(
            generate_hash(&PathBuf::from(
                "/Users/donjayamanne/demo/.venvTestInstall1/bin/python3.12"
            )),
            "e72c82125e7281e2"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_hash_generation_upper_case() {
        assert_eq!(
            generate_hash(&PathBuf::from(
                "/Users/donjayamanne/DEMO/.venvTestInstall1/bin/python3.12"
            )),
            "ecb0ee73d6ddfe97"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_hash_generation() {
        assert_eq!(
            generate_hash(&PathBuf::from(
                "C:\\temp\\poetry-folders\\demo-project1".to_string(),
            )),
            "c3694bfb39d7065b"
        );
    }
}
