// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use pet_fs::path::norm_case;
use sha2::{Digest, Sha256};
use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::env::ResolvedPythonEnv;

lazy_static! {
    static ref CACHE: FSCache = FSCache::new(None);
}

pub trait CacheEntry: Send + Sync {
    fn get(&self) -> Option<ResolvedPythonEnv>;
    fn store(&self, executable: PathBuf, environment: ResolvedPythonEnv);
    fn track_symlinks(&self, executable: PathBuf, symlinks: Option<Vec<PathBuf>>);
}

pub fn create_cache(executable: PathBuf) -> Arc<Mutex<Box<dyn CacheEntry>>> {
    CACHE.create_cache(executable)
}

pub fn get_cache_directory() -> Option<PathBuf> {
    CACHE.get_cache_directory()
}

pub fn set_cache_directory(cache_dir: PathBuf) {
    CACHE.set_cache_directory(cache_dir)
}

pub fn generate_hash(executable: &PathBuf) -> String {
    let mut hasher = Sha256::new();
    hasher.update(norm_case(executable).to_string_lossy().as_bytes());
    let h_bytes = hasher.finalize();
    // Convert 256 bits => Hext and then take 16 of the hex chars (that should be unique enough)
    // We will handle collisions if they happen.
    format!("{:x}", h_bytes)[..16].to_string()
}

pub type LockableCacheEntry = Arc<Mutex<Box<dyn CacheEntry>>>;

struct FSCache {
    cache_dir: Arc<Mutex<Option<PathBuf>>>,
    locks: Mutex<HashMap<PathBuf, LockableCacheEntry>>,
}

impl FSCache {
    pub fn new(cache_dir: Option<PathBuf>) -> FSCache {
        FSCache {
            cache_dir: Arc::new(Mutex::new(cache_dir)),
            locks: Mutex::new(HashMap::<PathBuf, LockableCacheEntry>::new()),
        }
    }

    pub fn get_cache_directory(&self) -> Option<PathBuf> {
        self.cache_dir.lock().unwrap().clone()
    }

    /// Once a cache directory has been set, you cannot change it.
    /// No point supporting such a scenario.
    pub fn set_cache_directory(&self, cache_dir: PathBuf) {
        self.cache_dir.lock().unwrap().replace(cache_dir);
    }
    pub fn create_cache(&self, executable: PathBuf) -> LockableCacheEntry {
        match self.locks.lock().unwrap().entry(executable.clone()) {
            Entry::Occupied(lock) => lock.get().clone(),
            Entry::Vacant(lock) => {
                let cache = Box::new(FSCacheEntry::create()) as Box<(dyn CacheEntry + 'static)>;
                lock.insert(Arc::new(Mutex::new(cache))).clone()
            }
        }
    }
}

struct FSCacheEntry {
    envoronment: Arc<Mutex<Option<ResolvedPythonEnv>>>,
}
impl FSCacheEntry {
    pub fn create() -> impl CacheEntry {
        FSCacheEntry {
            envoronment: Arc::new(Mutex::new(None)),
        }
    }
}

impl CacheEntry for FSCacheEntry {
    fn get(&self) -> Option<ResolvedPythonEnv> {
        self.envoronment.lock().unwrap().clone()
    }

    fn store(&self, _executable: PathBuf, environment: ResolvedPythonEnv) {
        self.envoronment.lock().unwrap().replace(environment);
    }

    fn track_symlinks(&self, _executable: PathBuf, _symlinks: Option<Vec<PathBuf>>) {
        todo!()
    }
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

    // #[test]
    // #[cfg(unix)]
    // fn test_hash_generation_upper_case() {
    //     let hashed_name = generate_env_name(
    //         "new-project",
    //         &"/Users/donjayamanne/temp/POETRY-UPPER/new-PROJECT".into(),
    //     );

    //     assert_eq!(hashed_name, "new-project-TbBV0MKD-py");
    // }

    // #[test]
    // #[cfg(windows)]
    // fn test_hash_generation_windows() {
    //     let hashed_name = generate_env_name(
    //         "demo-project1",
    //         &"C:\\temp\\poetry-folders\\demo-project1".into(),
    //     );

    //     assert_eq!(hashed_name, "demo-project1-f7sQRtG5-py");
    // }
}
