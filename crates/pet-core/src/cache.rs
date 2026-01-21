// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Generic caching abstraction for locators.
//!
//! Provides a thread-safe cache wrapper that consolidates common caching patterns
//! used across multiple locators in the codebase.

use std::{collections::HashMap, hash::Hash, path::PathBuf, sync::RwLock};

use crate::{manager::EnvManager, python_environment::PythonEnvironment};

/// A thread-safe cache that stores key-value pairs using RwLock for concurrent access.
///
/// This cache uses read-write locks to allow multiple concurrent readers while
/// ensuring exclusive access for writers. Values must implement Clone to be
/// returned from the cache.
pub struct LocatorCache<K, V> {
    cache: RwLock<HashMap<K, V>>,
}

impl<K: Eq + Hash, V: Clone> LocatorCache<K, V> {
    /// Creates a new empty cache.
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Returns a cloned value for the given key if it exists in the cache.
    pub fn get(&self, key: &K) -> Option<V> {
        self.cache.read().unwrap().get(key).cloned()
    }

    /// Checks if the cache contains the given key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.cache.read().unwrap().contains_key(key)
    }

    /// Inserts a key-value pair into the cache.
    ///
    /// Returns the previous value if the key was already present.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.cache.write().unwrap().insert(key, value)
    }

    /// Inserts multiple key-value pairs into the cache atomically.
    ///
    /// This method acquires a single write lock for all insertions, which is more
    /// efficient than calling `insert` multiple times when inserting many entries.
    pub fn insert_many(&self, entries: impl IntoIterator<Item = (K, V)>) {
        let mut cache = self.cache.write().unwrap();
        for (key, value) in entries {
            cache.insert(key, value);
        }
    }

    /// Returns a cloned value for the given key if it exists, otherwise computes
    /// and inserts the value using the provided closure.
    ///
    /// This method first checks with a read lock, then upgrades to a write lock
    /// if the value needs to be computed and inserted.
    #[must_use]
    pub fn get_or_insert_with<F>(&self, key: K, f: F) -> Option<V>
    where
        F: FnOnce() -> Option<V>,
        K: Clone,
    {
        // First check with read lock
        {
            let cache = self.cache.read().unwrap();
            if let Some(value) = cache.get(&key) {
                return Some(value.clone());
            }
        }

        // Compute the value (outside of any lock)
        if let Some(value) = f() {
            // Acquire write lock and insert
            let mut cache = self.cache.write().unwrap();
            // Double-check in case another thread inserted while we were computing
            if let Some(existing) = cache.get(&key) {
                return Some(existing.clone());
            }
            cache.insert(key, value.clone());
            Some(value)
        } else {
            None
        }
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }

    /// Returns all values in the cache as a vector.
    pub fn values(&self) -> Vec<V> {
        self.cache.read().unwrap().values().cloned().collect()
    }

    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.cache.read().unwrap().len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.read().unwrap().is_empty()
    }

    /// Returns all entries in the cache as a HashMap.
    pub fn clone_map(&self) -> HashMap<K, V>
    where
        K: Clone,
    {
        self.cache.read().unwrap().clone()
    }
}

impl<K: Eq + Hash, V: Clone> Default for LocatorCache<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for caching Python environments by their path.
pub type EnvironmentCache = LocatorCache<PathBuf, PythonEnvironment>;

/// Type alias for caching environment managers by their path.
pub type ManagerCache = LocatorCache<PathBuf, EnvManager>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_get_and_insert() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        assert!(cache.get(&"key1".to_string()).is_none());
        assert!(!cache.contains_key(&"key1".to_string()));

        cache.insert("key1".to_string(), 42);

        assert_eq!(cache.get(&"key1".to_string()), Some(42));
        assert!(cache.contains_key(&"key1".to_string()));
    }

    #[test]
    fn test_cache_get_or_insert_with() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        // First call should compute and insert
        let result = cache.get_or_insert_with("key1".to_string(), || Some(42));
        assert_eq!(result, Some(42));

        // Second call should return cached value
        let result = cache.get_or_insert_with("key1".to_string(), || Some(100));
        assert_eq!(result, Some(42));

        // Test with None return
        let result = cache.get_or_insert_with("key2".to_string(), || None);
        assert!(result.is_none());
        assert!(!cache.contains_key(&"key2".to_string()));
    }

    #[test]
    fn test_cache_clear() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        cache.insert("key1".to_string(), 42);
        cache.insert("key2".to_string(), 100);

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_values() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        cache.insert("key1".to_string(), 42);
        cache.insert("key2".to_string(), 100);

        let mut values = cache.values();
        values.sort();
        assert_eq!(values, vec![42, 100]);
    }

    #[test]
    fn test_cache_insert_many() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        let entries = vec![
            ("key1".to_string(), 42),
            ("key2".to_string(), 100),
            ("key3".to_string(), 200),
        ];

        cache.insert_many(entries);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&"key1".to_string()), Some(42));
        assert_eq!(cache.get(&"key2".to_string()), Some(100));
        assert_eq!(cache.get(&"key3".to_string()), Some(200));
    }
}
