// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! A generic caching abstraction for locators.
//!
//! This module provides a thread-safe cache implementation that can be used
//! by various locators to cache environments, managers, and other data.

use std::{
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    sync::RwLock,
};

use crate::{manager::EnvManager, python_environment::PythonEnvironment};

/// A thread-safe cache that stores key-value pairs.
///
/// Uses `RwLock` to allow concurrent reads while ensuring exclusive writes.
/// This is more efficient than `Mutex` when reads are more frequent than writes.
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

    /// Returns the value associated with the key, if present.
    pub fn get(&self, key: &K) -> Option<V> {
        self.cache.read().unwrap().get(key).cloned()
    }

    /// Returns true if the cache contains the given key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.cache.read().unwrap().contains_key(key)
    }

    /// Inserts a key-value pair into the cache.
    pub fn insert(&self, key: K, value: V) {
        self.cache.write().unwrap().insert(key, value);
    }

    /// Returns the value for the given key if present, otherwise computes it
    /// using the provided function, inserts it, and returns the value.
    ///
    /// If the function returns `None`, nothing is inserted and `None` is returned.
    pub fn get_or_insert_with<F>(&self, key: K, f: F) -> Option<V>
    where
        K: Clone,
        F: FnOnce() -> Option<V>,
    {
        // Check read lock first for fast path
        if let Some(v) = self.cache.read().unwrap().get(&key) {
            return Some(v.clone());
        }

        // Compute the value outside the lock
        let value = f()?;

        // Insert with write lock
        let mut cache = self.cache.write().unwrap();
        // Double-check in case another thread inserted while we were computing
        if let Some(v) = cache.get(&key) {
            return Some(v.clone());
        }
        cache.insert(key, value.clone());
        Some(value)
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }

    /// Returns a clone of all values in the cache.
    pub fn values(&self) -> Vec<V> {
        self.cache.read().unwrap().values().cloned().collect()
    }

    /// Returns a clone of the entire cache as a HashMap.
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

/// A thread-safe cached value that can be lazily computed.
///
/// Uses `RwLock` to allow concurrent reads while ensuring exclusive writes.
/// This is useful for caching values that are expensive to compute.
pub struct CachedValue<V> {
    value: RwLock<Option<V>>,
}

impl<V: Clone> CachedValue<V> {
    /// Creates a new empty cached value.
    pub fn new() -> Self {
        Self {
            value: RwLock::new(None),
        }
    }

    /// Returns the cached value if present.
    pub fn get(&self) -> Option<V> {
        self.value.read().unwrap().clone()
    }

    /// Sets the cached value.
    pub fn set(&self, value: V) {
        *self.value.write().unwrap() = Some(value);
    }

    /// Returns the cached value if present, otherwise computes it using the
    /// provided function, caches it, and returns it.
    pub fn get_or_compute<F>(&self, f: F) -> V
    where
        F: FnOnce() -> V,
    {
        // Fast path: check if already computed
        if let Some(v) = self.value.read().unwrap().clone() {
            return v;
        }

        // Compute the value
        let computed = f();

        // Store it
        let mut value = self.value.write().unwrap();
        // Double-check in case another thread computed while we were computing
        if let Some(v) = value.clone() {
            return v;
        }
        *value = Some(computed.clone());
        computed
    }

    /// Clears the cached value.
    pub fn clear(&self) {
        self.value.write().unwrap().take();
    }
}

impl<V: Clone> Default for CachedValue<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for caching a list of Python environments.
pub type EnvironmentListCache = CachedValue<Vec<PythonEnvironment>>;

/// Type alias for caching Python environments by path.
pub type EnvironmentCache = LocatorCache<PathBuf, PythonEnvironment>;

/// Type alias for caching environment managers by path.
pub type ManagerCache = LocatorCache<PathBuf, EnvManager>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_insert_and_get() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();
        cache.insert("key1".to_string(), 42);

        assert_eq!(cache.get(&"key1".to_string()), Some(42));
        assert_eq!(cache.get(&"key2".to_string()), None);
    }

    #[test]
    fn test_cache_contains_key() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();
        cache.insert("key1".to_string(), 42);

        assert!(cache.contains_key(&"key1".to_string()));
        assert!(!cache.contains_key(&"key2".to_string()));
    }

    #[test]
    fn test_cache_get_or_insert_with() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();

        // First call should compute and insert
        let result = cache.get_or_insert_with("key1".to_string(), || Some(42));
        assert_eq!(result, Some(42));

        // Second call should return cached value
        let result = cache.get_or_insert_with("key1".to_string(), || Some(100));
        assert_eq!(result, Some(42)); // Should still be 42

        // Test with None return
        let result = cache.get_or_insert_with("key2".to_string(), || None);
        assert_eq!(result, None);
        assert!(!cache.contains_key(&"key2".to_string()));
    }

    #[test]
    fn test_cache_clear() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();
        cache.insert("key1".to_string(), 42);
        cache.insert("key2".to_string(), 43);

        assert!(cache.contains_key(&"key1".to_string()));
        cache.clear();
        assert!(!cache.contains_key(&"key1".to_string()));
        assert!(!cache.contains_key(&"key2".to_string()));
    }

    #[test]
    fn test_cache_values() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();
        cache.insert("key1".to_string(), 42);
        cache.insert("key2".to_string(), 43);

        let mut values = cache.values();
        values.sort();
        assert_eq!(values, vec![42, 43]);
    }

    #[test]
    fn test_cache_clone_map() {
        let cache: LocatorCache<String, i32> = LocatorCache::new();
        cache.insert("key1".to_string(), 42);
        cache.insert("key2".to_string(), 43);

        let map = cache.clone_map();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"key1".to_string()), Some(&42));
        assert_eq!(map.get(&"key2".to_string()), Some(&43));
    }

    #[test]
    fn test_cached_value_get_or_compute() {
        let cached: CachedValue<i32> = CachedValue::new();

        // Initially empty
        assert_eq!(cached.get(), None);

        // First call should compute and cache
        let result = cached.get_or_compute(|| 42);
        assert_eq!(result, 42);

        // Second call should return cached value
        let result = cached.get_or_compute(|| 100);
        assert_eq!(result, 42); // Should still be 42

        // get() should also return cached value
        assert_eq!(cached.get(), Some(42));
    }

    #[test]
    fn test_cached_value_clear() {
        let cached: CachedValue<i32> = CachedValue::new();

        cached.get_or_compute(|| 42);
        assert_eq!(cached.get(), Some(42));

        cached.clear();
        assert_eq!(cached.get(), None);

        // After clear, should recompute
        let result = cached.get_or_compute(|| 100);
        assert_eq!(result, 100);
    }

    #[test]
    fn test_cached_value_set() {
        let cached: CachedValue<i32> = CachedValue::new();

        // Initially empty
        assert_eq!(cached.get(), None);

        // Set a value
        cached.set(42);
        assert_eq!(cached.get(), Some(42));

        // Set overwrites existing value
        cached.set(100);
        assert_eq!(cached.get(), Some(100));
    }

    #[test]
    fn test_cache_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(LocatorCache::<i32, i32>::new());
        let num_threads = 10;
        let operations_per_thread = 100;

        // Spawn multiple threads that concurrently read and write
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let cache = Arc::clone(&cache);
                thread::spawn(move || {
                    for i in 0..operations_per_thread {
                        let key = (thread_id * operations_per_thread + i) % 50;
                        // Mix of reads and writes
                        if i % 2 == 0 {
                            cache.insert(key, thread_id * 1000 + i);
                        } else {
                            let _ = cache.get(&key);
                        }
                    }
                })
            })
            .collect();

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Verify the cache is still functional after concurrent access
        cache.insert(999, 999);
        assert_eq!(cache.get(&999), Some(999));
    }

    #[test]
    fn test_cached_value_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let cached = Arc::new(CachedValue::<i32>::new());
        let num_threads = 10;

        // Spawn multiple threads that concurrently try to compute/set values
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let cached = Arc::clone(&cached);
                thread::spawn(move || {
                    for i in 0..100 {
                        if i % 3 == 0 {
                            cached.set(thread_id * 1000 + i);
                        } else if i % 3 == 1 {
                            let _ = cached.get();
                        } else {
                            let _ = cached.get_or_compute(|| thread_id * 1000 + i);
                        }
                    }
                })
            })
            .collect();

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Verify the cached value is still accessible after concurrent access
        cached.set(12345);
        assert_eq!(cached.get(), Some(12345));
    }
}
