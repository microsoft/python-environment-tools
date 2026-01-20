// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::{trace, warn};
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::{
    env::ResolvedPythonEnv,
    fs_cache::{delete_cache_file, get_cache_from_file, store_cache_in_file},
};

lazy_static! {
    static ref CACHE: CacheImpl = CacheImpl::new(None);
}

pub trait CacheEntry: Send + Sync {
    fn get(&self) -> Option<ResolvedPythonEnv>;
    fn store(&self, environment: ResolvedPythonEnv);
    fn track_symlinks(&self, symlinks: Vec<PathBuf>);
}

pub fn clear_cache() -> io::Result<()> {
    CACHE.clear()
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

pub type LockableCacheEntry = Arc<Mutex<Box<dyn CacheEntry>>>;

/// Cache of Interpreter details for a given executable.
/// Uses in memory cache as well as a file cache as backing store.
struct CacheImpl {
    cache_dir: Arc<Mutex<Option<PathBuf>>>,
    locks: Mutex<HashMap<PathBuf, LockableCacheEntry>>,
}

impl CacheImpl {
    fn new(cache_dir: Option<PathBuf>) -> CacheImpl {
        CacheImpl {
            cache_dir: Arc::new(Mutex::new(cache_dir)),
            locks: Mutex::new(HashMap::<PathBuf, LockableCacheEntry>::new()),
        }
    }

    fn get_cache_directory(&self) -> Option<PathBuf> {
        self.cache_dir.lock().unwrap().clone()
    }

    /// Once a cache directory has been set, you cannot change it.
    /// No point supporting such a scenario.
    fn set_cache_directory(&self, cache_dir: PathBuf) {
        if let Some(cache_dir) = self.cache_dir.lock().unwrap().clone() {
            warn!(
                "Cache directory has already been set to {:?}. Cannot change it now.",
                cache_dir
            );
            return;
        }
        trace!("Setting cache directory to {:?}", cache_dir);
        self.cache_dir.lock().unwrap().replace(cache_dir);
    }
    fn clear(&self) -> io::Result<()> {
        trace!("Clearing cache");
        self.locks.lock().unwrap().clear();
        if let Some(cache_directory) = self.cache_dir.lock().unwrap().clone() {
            std::fs::remove_dir_all(cache_directory)
        } else {
            Ok(())
        }
    }
    fn create_cache(&self, executable: PathBuf) -> LockableCacheEntry {
        let cache_directory = self.cache_dir.lock().unwrap().clone();
        match self.locks.lock().unwrap().entry(executable.clone()) {
            Entry::Occupied(lock) => lock.get().clone(),
            Entry::Vacant(lock) => {
                let cache = Box::new(CacheEntryImpl::create(cache_directory.clone(), executable))
                    as Box<dyn CacheEntry + 'static>;
                lock.insert(Arc::new(Mutex::new(cache))).clone()
            }
        }
    }
}

type FilePathWithMTimeCTime = (PathBuf, SystemTime, SystemTime);

struct CacheEntryImpl {
    cache_directory: Option<PathBuf>,
    executable: PathBuf,
    envoronment: Arc<Mutex<Option<ResolvedPythonEnv>>>,
    /// List of known symlinks to this executable.
    symlinks: Arc<Mutex<Vec<FilePathWithMTimeCTime>>>,
}
impl CacheEntryImpl {
    pub fn create(cache_directory: Option<PathBuf>, executable: PathBuf) -> impl CacheEntry {
        CacheEntryImpl {
            cache_directory,
            executable,
            envoronment: Arc::new(Mutex::new(None)),
            symlinks: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn verify_in_memory_cache(&self) {
        // Check if any of the exes have changed since we last cached this.
        for symlink_info in self.symlinks.lock().unwrap().iter() {
            if let Ok(metadata) = symlink_info.0.metadata() {
                if metadata.modified().ok() != Some(symlink_info.1)
                    || metadata.created().ok() != Some(symlink_info.2)
                {
                    trace!(
                        "Symlink {:?} has changed since we last cached it. original mtime & ctime {:?}, {:?}, current mtime & ctime {:?}, {:?}",
                        symlink_info.0,
                        symlink_info.1,
                        symlink_info.2,
                        metadata.modified().ok(),
                        metadata.created().ok()
                    );
                    self.envoronment.lock().unwrap().take();
                    if let Some(cache_directory) = &self.cache_directory {
                        delete_cache_file(cache_directory, &self.executable);
                    }
                }
            }
        }
    }
}

impl CacheEntry for CacheEntryImpl {
    fn get(&self) -> Option<ResolvedPythonEnv> {
        self.verify_in_memory_cache();

        // New scope to drop lock immediately after we have the value.
        {
            if let Some(env) = self.envoronment.lock().unwrap().clone() {
                return Some(env);
            }
        }

        if let Some(ref cache_directory) = self.cache_directory {
            let (env, mut symlinks) = get_cache_from_file(cache_directory, &self.executable)?;
            self.envoronment.lock().unwrap().replace(env.clone());
            let mut locked_symlinks = self.symlinks.lock().unwrap();
            locked_symlinks.clear();
            locked_symlinks.append(&mut symlinks);
            Some(env)
        } else {
            None
        }
    }

    fn store(&self, environment: ResolvedPythonEnv) {
        // Get hold of the mtimes and ctimes of the symlinks.
        let mut symlinks = vec![];
        for symlink in environment.symlinks.clone().unwrap_or_default().iter() {
            if let Ok(metadata) = symlink.metadata() {
                // We only care if we have the information
                if let (Some(modified), Some(created)) =
                    (metadata.modified().ok(), metadata.created().ok())
                {
                    symlinks.push((symlink.clone(), modified, created));
                }
            }
        }

        symlinks.sort();
        symlinks.dedup();

        {
            let mut locked_symlinks = self.symlinks.lock().unwrap();
            locked_symlinks.clear();
            locked_symlinks.append(&mut symlinks.clone());
        }
        self.envoronment
            .lock()
            .unwrap()
            .replace(environment.clone());

        trace!("Caching interpreter info for {:?}", self.executable);

        if let Some(ref cache_directory) = self.cache_directory {
            store_cache_in_file(cache_directory, &self.executable, &environment, symlinks)
        }
    }

    fn track_symlinks(&self, symlinks: Vec<PathBuf>) {
        self.verify_in_memory_cache();

        // If we have already seen this symlink, then we do not need to do anything.
        let known_symlinks: HashSet<PathBuf> = self
            .symlinks
            .lock()
            .unwrap()
            .clone()
            .iter()
            .map(|x| x.0.clone())
            .collect();

        if symlinks.iter().all(|x| known_symlinks.contains(x)) {
            return;
        }

        if let Some(ref cache_directory) = self.cache_directory {
            if let Some((mut env, _)) = get_cache_from_file(cache_directory, &self.executable) {
                let mut all_symlinks = vec![];
                all_symlinks.append(&mut env.symlinks.clone().unwrap_or_default());
                all_symlinks.append(&mut symlinks.clone());
                all_symlinks.sort();
                all_symlinks.dedup();

                // Chech whether the details in the cache are the same as the ones we are about to cache.

                env.symlinks = Some(all_symlinks);
                trace!("Updating cache for {:?} with new symlinks", self.executable);
                self.store(env);
            } else {
                // Unlikely scenario.
            }
        }
    }
}
