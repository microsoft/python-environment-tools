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
    fs_cache::{
        delete_cache_file, executable_cache_key, executable_cache_key_from, get_cache_from_file,
        store_cache_in_file,
    },
};

lazy_static! {
    static ref CACHE: CacheImpl = CacheImpl::new(None);
}

pub trait CacheEntry: Send + Sync {
    fn get(&self) -> Option<ResolvedPythonEnv>;
    fn get_for_executable(&self, executable: &std::path::Path) -> Option<ResolvedPythonEnv> {
        self.get()
            .map(|environment| environment.for_executable_alias(executable))
    }
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
        self.cache_dir
            .lock()
            .expect("cache_dir mutex poisoned")
            .clone()
    }

    /// Once a cache directory has been set, you cannot change it.
    /// No point supporting such a scenario.
    fn set_cache_directory(&self, cache_dir: PathBuf) {
        if let Some(cache_dir) = self
            .cache_dir
            .lock()
            .expect("cache_dir mutex poisoned")
            .clone()
        {
            warn!(
                "Cache directory has already been set to {:?}. Cannot change it now.",
                cache_dir
            );
            return;
        }
        trace!("Setting cache directory to {:?}", cache_dir);
        self.cache_dir
            .lock()
            .expect("cache_dir mutex poisoned")
            .replace(cache_dir);
    }
    fn clear(&self) -> io::Result<()> {
        trace!("Clearing cache");
        self.locks.lock().expect("locks mutex poisoned").clear();
        if let Some(cache_directory) = self
            .cache_dir
            .lock()
            .expect("cache_dir mutex poisoned")
            .clone()
        {
            std::fs::remove_dir_all(cache_directory)
        } else {
            Ok(())
        }
    }
    fn create_cache(&self, executable: PathBuf) -> LockableCacheEntry {
        let cache_key = executable_cache_key(&executable);
        let cache_directory = self
            .cache_dir
            .lock()
            .expect("cache_dir mutex poisoned")
            .clone();
        match self
            .locks
            .lock()
            .expect("locks mutex poisoned")
            .entry(cache_key.clone())
        {
            Entry::Occupied(lock) => lock.get().clone(),
            Entry::Vacant(lock) => {
                let cache = Box::new(CacheEntryImpl::create(cache_directory.clone(), cache_key))
                    as Box<dyn CacheEntry + 'static>;
                lock.insert(Arc::new(Mutex::new(cache))).clone()
            }
        }
    }
}

/// Represents a file path with its modification time and optional creation time.
/// Creation time (ctime) is optional because many Linux filesystems (ext4, etc.)
/// don't support file creation time, causing metadata.created() to return Err.
/// See: https://github.com/microsoft/python-environment-tools/issues/223
type FilePathWithMTimeCTime = (PathBuf, SystemTime, Option<SystemTime>);

fn current_dir_for_aliases(aliases: &[PathBuf]) -> Option<PathBuf> {
    aliases
        .iter()
        .any(|alias| alias.is_relative())
        .then(std::env::current_dir)
        .transpose()
        .ok()
        .flatten()
}

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
        let cache_is_valid = self
            .symlinks
            .lock()
            .expect("symlinks mutex poisoned")
            .iter()
            .all(|symlink_info| {
                if let Ok(metadata) = symlink_info.0.metadata() {
                    let mtime_changed = metadata.modified().ok() != Some(symlink_info.1);
                    let ctime_changed = match symlink_info.2 {
                        Some(stored_ctime) => metadata.created().ok() != Some(stored_ctime),
                        None => false,
                    };
                    !mtime_changed && !ctime_changed
                } else {
                    false
                }
            });

        if !cache_is_valid {
            trace!(
                "Tracked executable changed or disappeared for {:?}",
                self.executable
            );
            self.envoronment
                .lock()
                .expect("envoronment mutex poisoned")
                .take();
            if let Some(cache_directory) = &self.cache_directory {
                delete_cache_file(cache_directory, &self.executable);
            }
        }
    }
}

impl CacheEntry for CacheEntryImpl {
    fn get(&self) -> Option<ResolvedPythonEnv> {
        self.verify_in_memory_cache();

        // New scope to drop lock immediately after we have the value.
        {
            if let Some(env) = self
                .envoronment
                .lock()
                .expect("envoronment mutex poisoned")
                .clone()
            {
                return Some(env);
            }
        }

        if let Some(ref cache_directory) = self.cache_directory {
            let (env, mut symlinks) = get_cache_from_file(cache_directory, &self.executable)?;
            self.envoronment
                .lock()
                .expect("envoronment mutex poisoned")
                .replace(env.clone());
            let mut locked_symlinks = self.symlinks.lock().expect("symlinks mutex poisoned");
            locked_symlinks.clear();
            locked_symlinks.append(&mut symlinks);
            Some(env)
        } else {
            None
        }
    }

    fn store(&self, environment: ResolvedPythonEnv) {
        // Get hold of the mtimes and ctimes of the symlinks.
        let aliases = environment.symlinks.clone().unwrap_or_default();
        let current_dir = current_dir_for_aliases(&aliases);
        let mut symlinks = vec![];
        for alias in &aliases {
            let symlink = executable_cache_key_from(alias, current_dir.as_deref());
            if let Ok(metadata) = symlink.metadata() {
                // We require mtime, but ctime is optional (not available on all Linux filesystems)
                // See: https://github.com/microsoft/python-environment-tools/issues/223
                if let Ok(modified) = metadata.modified() {
                    let created = metadata.created().ok(); // May be None on Linux
                    symlinks.push((symlink, modified, created));
                }
            }
        }

        symlinks.sort();
        symlinks.dedup();

        {
            let mut locked_symlinks = self.symlinks.lock().expect("symlinks mutex poisoned");
            locked_symlinks.clear();
            locked_symlinks.append(&mut symlinks.clone());
        }
        self.envoronment
            .lock()
            .expect("envoronment mutex poisoned")
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
            .expect("symlinks mutex poisoned")
            .clone()
            .iter()
            .map(|x| x.0.clone())
            .collect();
        let current_dir = current_dir_for_aliases(&symlinks);
        if symlinks
            .iter()
            .map(|alias| executable_cache_key_from(alias, current_dir.as_deref()))
            .all(|key| known_symlinks.contains(&key))
        {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir_in;

    fn environment(executable: PathBuf, aliases: Vec<PathBuf>) -> ResolvedPythonEnv {
        ResolvedPythonEnv {
            executable,
            prefix: PathBuf::from("prefix"),
            version: "3.12.0".to_string(),
            is64_bit: true,
            symlinks: Some(aliases),
        }
    }

    fn aliases() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let current_dir = std::env::current_dir().unwrap();
        let temp_dir = tempdir_in(&current_dir).unwrap();
        let absolute = temp_dir.path().join("python");
        std::fs::write(&absolute, "python").unwrap();
        let relative = absolute.strip_prefix(&current_dir).unwrap().to_path_buf();
        (temp_dir, relative, absolute)
    }

    #[test]
    fn relative_and_absolute_aliases_share_in_memory_entry() {
        let (_temp_dir, relative, absolute) = aliases();
        let cache = CacheImpl::new(None);

        let relative_entry = cache.create_cache(relative);
        let absolute_entry = cache.create_cache(absolute);

        assert!(Arc::ptr_eq(&relative_entry, &absolute_entry));
    }

    #[test]
    fn cache_hit_uses_current_alias_and_preserves_shorter_aliases() {
        let (_temp_dir, relative, absolute) = aliases();
        let cache = CacheImpl::new(None);
        let entry = cache.create_cache(relative.clone());
        let entry = entry.lock().unwrap();
        entry.store(environment(
            relative.clone(),
            vec![relative.clone(), absolute.clone()],
        ));

        let relative_hit = entry.get_for_executable(&relative).unwrap();
        assert_eq!(relative_hit.executable, relative);

        let absolute_hit = entry.get_for_executable(&absolute).unwrap();
        assert_eq!(absolute_hit.executable, absolute);
        let hit_aliases = absolute_hit.symlinks.unwrap();
        assert!(hit_aliases.contains(&relative));
        assert!(hit_aliases.contains(&absolute));
    }

    #[test]
    fn disk_cache_reuses_relative_entry_for_absolute_alias() {
        let (temp_dir, relative, absolute) = aliases();
        let cache_directory = temp_dir.path().join("cache");
        {
            let cache = CacheImpl::new(Some(cache_directory.clone()));
            let entry = cache.create_cache(relative.clone());
            entry.lock().unwrap().store(environment(
                relative.clone(),
                vec![relative.clone(), absolute.clone()],
            ));
        }

        let cache = CacheImpl::new(Some(cache_directory));
        let entry = cache.create_cache(absolute.clone());
        let hit = entry.lock().unwrap().get_for_executable(&absolute).unwrap();

        assert_eq!(hit.executable, absolute);
        let hit_aliases = hit.symlinks.unwrap();
        assert!(hit_aliases.contains(&relative));
        assert!(hit_aliases.contains(&absolute));
    }

    #[test]
    fn missing_tracked_executable_invalidates_in_memory_entry() {
        let (temp_dir, _relative, absolute) = aliases();
        let cache = CacheImpl::new(Some(temp_dir.path().join("cache")));
        let entry = cache.create_cache(absolute.clone());
        let entry = entry.lock().unwrap();
        entry.store(environment(absolute.clone(), vec![absolute.clone()]));
        assert!(entry.get().is_some());

        std::fs::remove_file(&absolute).unwrap();

        assert!(entry.get().is_none());
    }
}
