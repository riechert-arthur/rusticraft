/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub struct LazyCallable<T, F> {
    value: OnceLock<T>,
    create: F,
}

impl<T, F> LazyCallable<T, F>
where
    F: Fn() -> T,
{
    pub const fn new(create: F) -> Self {
        Self {
            value: OnceLock::new(),
            create,
        }
    }

    pub fn call(&self) -> &T {
        self.value.get_or_init(&self.create)
    }

    pub fn get(&self) -> &T {
        self.call()
    }

    pub const fn cancel(&self, _may_interrupt_if_running: bool) -> bool {
        false
    }

    pub const fn is_cancelled(&self) -> bool {
        false
    }

    pub fn is_done(&self) -> bool {
        self.value.get().is_some()
    }
}

pub trait ExpiringEntry {
    fn timestamp(&self) -> u64;

    fn close(&self) {}
}

pub struct CacheEntry<T> {
    timestamp: AtomicU64,
    value: OnceLock<T>,
    create: Mutex<Option<Box<dyn FnOnce() -> T + Send>>>,
    done: Arc<AtomicBool>,
}

impl<T> CacheEntry<T> {
    pub fn new(create: impl FnOnce() -> T + Send + 'static) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let complete = Arc::clone(&done);
        Self {
            timestamp: AtomicU64::new(current_time_millis()),
            value: OnceLock::new(),
            create: Mutex::new(Some(Box::new(move || {
                let value = create();
                complete.store(true, Ordering::Release);
                value
            }))),
            done,
        }
    }

    pub fn compute_async(create: impl FnOnce() -> T + Send + 'static) -> Self
    where
        T: Send + 'static,
    {
        let done = Arc::new(AtomicBool::new(false));
        let complete = Arc::clone(&done);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let value = create();
            let _ = sender.send(value);
            complete.store(true, Ordering::Release);
        });
        Self {
            timestamp: AtomicU64::new(current_time_millis()),
            value: OnceLock::new(),
            create: Mutex::new(Some(Box::new(move || {
                receiver
                    .recv()
                    .expect("asynchronous cache entry task disconnected")
            }))),
            done,
        }
    }

    pub fn get(&self) -> &T {
        self.timestamp
            .store(current_time_millis(), Ordering::Release);
        self.value.get_or_init(|| {
            self.create
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .expect("cache entry initializer was already consumed")()
        })
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

impl<T> ExpiringEntry for CacheEntry<T> {
    fn timestamp(&self) -> u64 {
        self.timestamp.load(Ordering::Acquire)
    }
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

struct LongMapState<T> {
    values: HashMap<i64, Arc<T>>,
    order: VecDeque<i64>,
}

pub struct BoundedLongMap<T> {
    capacity: usize,
    state: RwLock<LongMapState<T>>,
}

impl<T> BoundedLongMap<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: RwLock::new(LongMapState {
                values: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            }),
        }
    }

    pub fn len(&self) -> usize {
        self.state
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .values
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        state.values.clear();
        state.order.clear();
    }

    pub fn remove(&self, key: i64) -> Option<Arc<T>> {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let value = state.values.remove(&key);
        if value.is_some() {
            state.order.retain(|entry| *entry != key);
        }
        value
    }

    pub fn remove_if(&self, predicate: impl Fn(&T) -> bool) -> usize {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let keys = state
            .values
            .iter()
            .filter_map(|(key, value)| predicate(value).then_some(*key))
            .collect::<Vec<_>>();
        for key in &keys {
            state.values.remove(key);
        }
        state.order.retain(|key| !keys.contains(key));
        keys.len()
    }

    pub fn put(&self, key: i64, value: T) -> Option<Arc<T>> {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let previous = state.values.insert(key, Arc::new(value));
        if previous.is_none() {
            state.order.push_back(key);
        }
        previous
    }

    pub fn get(&self, key: i64) -> Option<Arc<T>> {
        self.state
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .values
            .get(&key)
            .cloned()
    }

    pub fn compute_if_absent(&self, key: i64, factory: impl FnOnce(i64) -> T) -> Arc<T> {
        if let Some(value) = self.get(key) {
            return value;
        }
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if state.values.len() >= self.capacity {
            if let Some(first) = state.order.pop_front() {
                state.values.remove(&first);
            }
        }
        if let Some(value) = state.values.get(&key) {
            return Arc::clone(value);
        }
        let value = Arc::new(factory(key));
        state.values.insert(key, Arc::clone(&value));
        state.order.push_back(key);
        value
    }

    pub fn map<V>(
        &self,
        key: i64,
        factory: impl FnOnce(i64) -> T,
        mapper: impl FnOnce(&T) -> V,
    ) -> V {
        mapper(&self.compute_if_absent(key, factory))
    }
}

pub struct Cache<T> {
    name: String,
    entries: BoundedLongMap<T>,
    lifetime_millis: u64,
    timeout: AtomicU64,
}

impl<T: ExpiringEntry> Cache<T> {
    pub fn new(name: impl Into<String>, capacity: usize, lifetime_millis: u64) -> Self {
        Self {
            name: name.into(),
            entries: BoundedLongMap::new(capacity),
            lifetime_millis,
            timeout: AtomicU64::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn remove(&self, key: i64) {
        if let Some(entry) = self.entries.remove(key) {
            entry.close();
        }
    }

    pub fn get(&self, key: i64) -> Option<Arc<T>> {
        self.entries.get(key)
    }

    pub fn compute_if_absent(&self, key: i64, factory: impl FnOnce(i64) -> T) -> Arc<T> {
        self.entries.compute_if_absent(key, factory)
    }

    pub fn map<V>(
        &self,
        key: i64,
        factory: impl FnOnce(i64) -> T,
        mapper: impl FnOnce(&T) -> V,
    ) -> V {
        self.entries.map(key, factory, mapper)
    }

    pub fn run(&self) -> usize {
        self.run_at(current_time_millis())
    }

    pub fn run_at(&self, now: u64) -> usize {
        let timeout = now.saturating_sub(self.lifetime_millis);
        self.timeout.store(timeout, Ordering::Release);
        self.entries.remove_if(|entry| entry.timestamp() < timeout)
    }

    pub fn timeout(&self) -> u64 {
        self.timeout.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[test]
    fn lazy_callable_matches_engine_0_3_0() {
        let calls = AtomicUsize::new(0);
        let lazy = LazyCallable::new(|| {
            calls.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert!(!lazy.is_done());
        assert!(!lazy.cancel(true));
        assert!(!lazy.is_cancelled());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert_eq!(*lazy.get(), 42));
            }
        });
        assert!(lazy.is_done());
        assert_eq!(*lazy.call(), 42);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cache_entry_matches_engine_0_3_0() {
        let calls = Arc::new(AtomicUsize::new(0));
        let create_calls = Arc::clone(&calls);
        let entry = CacheEntry::new(move || {
            create_calls.fetch_add(1, Ordering::Relaxed);
            42
        });
        let created = entry.timestamp();

        assert!(!entry.is_done());
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_eq!(*entry.get(), 42);
        assert!(entry.is_done());
        assert!(entry.timestamp() > created);
        assert_eq!(*entry.get(), 42);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        entry.close();
    }

    #[test]
    fn asynchronous_cache_entry_matches_engine_0_3_0() {
        let (release, wait) = mpsc::sync_channel(1);
        let entry = CacheEntry::compute_async(move || {
            wait.recv().unwrap();
            73
        });
        assert!(!entry.is_done());
        release.send(()).unwrap();
        while !entry.is_done() {
            std::thread::yield_now();
        }
        assert_eq!(*entry.get(), 73);
    }

    struct TestEntry {
        id: i32,
        timestamp: u64,
        closed: AtomicBool,
    }

    impl TestEntry {
        fn new(id: i32, timestamp: u64) -> Self {
            Self {
                id,
                timestamp,
                closed: AtomicBool::new(false),
            }
        }
    }

    impl ExpiringEntry for TestEntry {
        fn timestamp(&self) -> u64 {
            self.timestamp
        }

        fn close(&self) {
            self.closed.store(true, Ordering::Release);
        }
    }

    #[test]
    fn bounded_long_map_matches_engine_0_3_0() {
        let map = BoundedLongMap::new(3);
        map.put(1, 10);
        map.put(2, 20);
        map.put(3, 30);
        assert_eq!(map.compute_if_absent(4, |_| 40).as_ref(), &40);
        assert!(map.get(1).is_none());
        assert_eq!(map.map(2, |_| unreachable!(), |value| *value), 20);
        assert_eq!(map.remove_if(|value| *value >= 30), 2);
        assert_eq!(map.len(), 1);

        map.put(5, 50);
        map.put(6, 60);
        map.put(7, 70);
        assert_eq!(map.len(), 4);
        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn expiring_cache_matches_engine_0_3_0() {
        let cache = Cache::new("TileCache", 3, 60);
        assert_eq!(cache.name(), "TileCache");
        let removed = cache.compute_if_absent(1, |_| TestEntry::new(1, 900));
        cache.remove(1);
        assert!(removed.closed.load(Ordering::Acquire));

        cache.compute_if_absent(2, |_| TestEntry::new(2, 939));
        cache.compute_if_absent(3, |_| TestEntry::new(3, 940));
        cache.compute_if_absent(4, |_| TestEntry::new(4, 1_000));
        assert_eq!(cache.run_at(1_000), 1);
        assert_eq!(cache.timeout(), 940);
        assert!(cache.get(2).is_none());
        assert_eq!(cache.get(3).unwrap().id, 3);
        assert_eq!(cache.map(4, |_| unreachable!(), |entry| entry.id), 4);
    }
}
