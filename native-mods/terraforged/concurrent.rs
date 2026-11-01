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

use super::terraforged_river::{Rivermap, current_time_millis};
use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock, mpsc};
use std::thread;

/// Engine 0.3.0's `LazyCallable` (`com.terraforged.engine.concurrent.task`), the deferred value
/// behind `TileGenerator.getTile` (`CallableTile`, `CallableZoomTile`): `create` runs on the first
/// `call`/`get`, and every later caller, on any thread, sees that same value.
///
/// As with Engine's `StampedLock`, `create` runs under an exclusive lock: callers that arrive
/// while it runs wait for it, and so does `is_done`, which then reports whether it succeeded. A
/// `create` that panics (Engine: throws) stores nothing; the next caller, or one already waiting,
/// runs it again. The lock is not reentrant: a `create` that calls `call`, `get` or `is_done` on
/// its own callable never returns (Engine's `StampedLock` hangs there too; std's `Mutex` may
/// instead panic). Engine's `create` may not return null (`Objects.requireNonNull`); here it always
/// returns a value. Dropping the callable drops the value: Engine's `LazyCallable` disposes of
/// nothing either. Its adapters (`adapt`, `adaptComplete`, `callAsync`, `then`) and the timed
/// `get` are not ported.
pub struct LazyCallable<T, F> {
    lock: Mutex<()>,
    value: OnceLock<T>,
    create: F,
}

impl<T, F> LazyCallable<T, F>
where
    F: Fn() -> T,
{
    pub const fn new(create: F) -> Self {
        Self {
            lock: Mutex::new(()),
            value: OnceLock::new(),
            create,
        }
    }

    /// `call()`: the value, created by this caller unless an earlier one stored it. A caller
    /// that finds another one creating it waits, without creating it again.
    pub fn call(&self) -> &T {
        if let Some(value) = self.value.get() {
            return value;
        }
        // Engine has no poisoning: after a throwing `create` the next caller just retries.
        let _write = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(value) = self.value.get() {
            return value;
        }
        let value = (self.create)();
        self.value.get_or_init(|| value)
    }

    /// `get()`: [`Self::call`].
    pub fn get(&self) -> &T {
        self.call()
    }

    /// `cancel(mayInterruptIfRunning)`: always `false`, nothing is cancelled.
    pub const fn cancel(&self, _may_interrupt_if_running: bool) -> bool {
        false
    }

    /// `isCancelled()`: always `false`.
    pub const fn is_cancelled(&self) -> bool {
        false
    }

    /// `isDone()`: whether a value is stored. While another caller is creating it, this waits for
    /// that `create` to end, as Engine's read lock does, then reports whether it stored a value.
    pub fn is_done(&self) -> bool {
        if self.value.get().is_some() {
            return true;
        }
        drop(self.lock.lock().unwrap_or_else(PoisonError::into_inner));
        self.value.get().is_some()
    }
}

/// Engine 0.3.0's `ExpiringEntry` (`com.terraforged.engine.concurrent.cache`): what Engine's `Cache`
/// expires.
pub trait ExpiringEntry {
    /// `getTimestamp()`: the entry's last access, in `System.currentTimeMillis()` milliseconds.
    fn timestamp(&self) -> i64;

    /// `close()`: run when the cache removes the entry; does nothing unless overridden.
    fn close(&self) {}
}

/// Engine 0.3.0's `Disposable.Listener` (`com.terraforged.engine.concurrent`): told when a value is
/// disposed of. A `Tile` calls it from `dispose`, on the disposing thread, holding no lock.
pub trait DisposeListener<T>: Send + Sync {
    /// `onDispose(value)`.
    fn on_dispose(&self, value: &T);
}

/// What Engine's `Future` records of the entry's task: whether it ended and, if it panicked
/// (Engine: threw), its message.
#[derive(Default)]
struct TaskOutcome {
    done: AtomicBool,
    failure: OnceLock<String>,
}

impl TaskOutcome {
    /// Records a panicked task's message, which every later `get`/`call` panics with.
    fn fail(&self, payload: &(dyn std::any::Any + Send)) {
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let _ = self
            .failure
            .set(format!("cache entry task panicked: {message}"));
    }

    /// Panics with the recorded failure, if the task panicked.
    fn rethrow(&self) {
        if let Some(failure) = self.failure.get() {
            panic!("{failure}");
        }
    }
}

/// The entry's task, boxed so that `CacheEntry` can name the [`LazyCallable`] that runs it.
type Task<'a, T> = Box<dyn Fn() -> T + Send + Sync + 'a>;

/// A job queued on a [`ThreadPool`]'s task threads.
type Job = Box<dyn FnOnce() + Send>;

/// The task threads of a [`ThreadPool`]: Engine's fixed `TF-Task` executor, one FIFO queue.
struct TaskThreads {
    threads: i32,
    queue: mpsc::Sender<Job>,
    jobs: Arc<Mutex<mpsc::Receiver<Job>>>,
    started: Mutex<i32>,
}

/// Engine 0.3.0's `ThreadPool` (`com.terraforged.engine.concurrent.thread`) as
/// [`CacheEntry::compute_async`] submits to it: [`ThreadPool::NONE`] (Engine's `EmptyThreadPool`)
/// or [`ThreadPool::create`] (Engine's `BatchingThreadPool`, whose task executor runs the entries).
///
/// Engine's `ThreadPools.create` hands out one shared pool per size and shuts the previous one down;
/// here every `create` is its own pool. Its batch threads are `tile_generator.rs`'s scoped threads,
/// not these. Clones share the task threads; once the last clone is dropped the
/// threads finish the queued tasks and exit (Engine's daemon threads outlive an unreferenced pool).
#[derive(Clone)]
pub struct ThreadPool {
    size: i32,
    tasks: Option<Arc<TaskThreads>>,
}

impl ThreadPool {
    /// `ThreadPools.NONE`: no threads; a task runs on the thread of the entry's first `get`.
    pub const NONE: Self = Self {
        size: 0,
        tasks: None,
    };

    /// `BatchingThreadPool.of(size)` (`ThreadPools.create(size)`): `max(1, size / 2)` task threads,
    /// named `TF-Task-Worker-<n>` and started one per submitted task until all exist (Engine's
    /// `newFixedThreadPool`), taking tasks in submission order.
    pub fn create(size: i32) -> Self {
        let threads = (size / 2).max(1);
        let (queue, jobs) = mpsc::channel();
        Self {
            size: threads.wrapping_add(size.max(2)),
            tasks: Some(Arc::new(TaskThreads {
                threads,
                queue,
                jobs: Arc::new(Mutex::new(jobs)),
                started: Mutex::new(0),
            })),
        }
    }

    /// `size()`: the task threads plus `max(2, size)` batch threads; 0 for [`Self::NONE`].
    pub const fn size(&self) -> i32 {
        self.size
    }

    /// `submit(callable)` on the task threads, starting one more if not all exist yet.
    fn submit(tasks: &TaskThreads, job: Job) {
        tasks
            .queue
            .send(job)
            .expect("the task threads hold the queue");
        let mut started = tasks.started.lock().unwrap_or_else(PoisonError::into_inner);
        if *started < tasks.threads {
            *started += 1;
            let jobs = Arc::clone(&tasks.jobs);
            thread::Builder::new()
                .name(format!("TF-Task-Worker-{started}"))
                .spawn(move || {
                    loop {
                        let job = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        match job {
                            Ok(job) => job(),
                            Err(_) => break,
                        }
                    }
                })
                .expect("failed to start a TF-Task thread");
        }
    }
}

/// Engine 0.3.0's `CacheEntry` (`com.terraforged.engine.concurrent.cache`): a [`LazyCallable`] over a
/// task, plus the last-access timestamp Engine's `Cache` expires it by.
///
/// From [`Self::new`], the task runs on the first `get`/`call`, on the calling thread, like Engine's
/// `CacheEntry` over a task its first `task.get()` runs; [`Self::compute_async`] and [`Self::then`]
/// run it on a [`ThreadPool`]. As in [`LazyCallable`], callers arriving while it runs wait for it
/// and all see the same value. Except on [`ThreadPool::NONE`], the task runs at most once: like a
/// failed Engine `Future`, a task that panicked makes every later `get`/`call` panic too, without
/// running it again, and `is_done` reports it done.
/// A task that calls `get` or `call` on its own entry never returns, as with [`LazyCallable`].
///
/// The timestamp is `System.currentTimeMillis()` (`river.rs`'s [`current_time_millis`]) at
/// construction, and again at the start of every `get`, before the task runs; `call`, `is_done`
/// and `timestamp` leave it as is (Engine overrides only `get`). It is a Java `long` stored as
/// read, so it never wraps.
pub struct CacheEntry<'a, T> {
    timestamp: AtomicI64,
    clock: fn() -> i64,
    outcome: Arc<TaskOutcome>,
    value: LazyCallable<T, Task<'a, T>>,
    /// What `close` runs on the value ([`Self::closing`]).
    on_close: Option<fn(&T)>,
}

impl<'a, T> CacheEntry<'a, T> {
    /// `CacheEntry.supply(task)` for a task that runs on the first `get`.
    pub fn new(task: impl Fn() -> T + Send + Sync + 'a) -> Self {
        Self::with_clock(task, current_time_millis)
    }

    /// [`Self::new`] reading the time from `clock` in place of `System.currentTimeMillis()`.
    pub fn with_clock(task: impl Fn() -> T + Send + Sync + 'a, clock: fn() -> i64) -> Self {
        let outcome = Arc::new(TaskOutcome::default());
        let record = Arc::clone(&outcome);
        // `create()`: `task.get()`. The callable only calls this again after a panic, so the task
        // itself runs once; later calls repeat its failure, as Engine's failed `Future` rethrows.
        let create = move || {
            record.rethrow();
            let result = catch_unwind(AssertUnwindSafe(&task));
            if let Err(payload) = &result {
                record.fail(&**payload);
            }
            record.done.store(true, Ordering::Release);
            result.unwrap_or_else(|payload| resume_unwind(payload))
        };
        Self::from_task(Box::new(create), outcome, clock)
    }

    fn from_task(create: Task<'a, T>, outcome: Arc<TaskOutcome>, clock: fn() -> i64) -> Self {
        Self {
            timestamp: AtomicI64::new(clock()),
            clock,
            outcome,
            value: LazyCallable::new(create),
            on_close: None,
        }
    }

    /// The entry with `close` as its value's `close()`: Engine's `CacheEntry.close` closes a value
    /// that is `SafeCloseable` (or `AutoCloseable`); Rust has no `instanceof`, so the entry's maker
    /// names it (the tile cache passes [`Tile::close`](super::terraforged_tile::Tile::close)).
    #[must_use]
    pub fn closing(mut self, close: fn(&T)) -> Self {
        self.on_close = Some(close);
        self
    }

    /// `get()`: stamps the access time, then returns the value, running the task first if no caller
    /// has yet.
    pub fn get(&self) -> &T {
        self.timestamp.store((self.clock)(), Ordering::Release);
        self.value.get()
    }

    /// `call()`: the value, like [`Self::get`] but without stamping the access time.
    pub fn call(&self) -> &T {
        self.value.call()
    }

    /// `cancel(mayInterruptIfRunning)`: always `false`, nothing is cancelled.
    pub const fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        self.value.cancel(may_interrupt_if_running)
    }

    /// `isCancelled()`: always `false`.
    pub const fn is_cancelled(&self) -> bool {
        self.value.is_cancelled()
    }

    /// `isDone()`: `task.isDone()`, whether the task has ended, by returning or by panicking. It does
    /// not wait for a task another thread is running (Engine's `Future.isDone` does not either).
    pub fn is_done(&self) -> bool {
        self.outcome.done.load(Ordering::Acquire)
    }
}

impl<T: Send + 'static> CacheEntry<'static, T> {
    /// `CacheEntry.computeAsync(task, executor)`: the entry of `executor.submit(task)`.
    ///
    /// On a [`ThreadPool::create`] pool the task starts on a task thread now; `is_done` is `false`
    /// until it ends (returning or panicking), and `get`/`call` wait for it. A panicked task makes
    /// every `get`/`call` panic, without running it again (Engine's failed `Future`). On
    /// [`ThreadPool::NONE`] (Engine's `adaptComplete`) `is_done` is `true` at once, the task runs on
    /// the first `get`/`call`'s thread, and a task that panicked runs again on the next one. The
    /// timestamp is the clock after the submit, then each `get`'s, as for [`Self::new`].
    pub fn compute_async(
        task: impl Fn() -> T + Send + Sync + 'static,
        executor: &ThreadPool,
    ) -> Self {
        Self::compute_async_with_clock(task, executor, current_time_millis)
    }

    /// [`Self::compute_async`] reading the time from `clock` in place of `System.currentTimeMillis()`.
    pub fn compute_async_with_clock(
        task: impl Fn() -> T + Send + Sync + 'static,
        executor: &ThreadPool,
        clock: fn() -> i64,
    ) -> Self {
        let outcome = Arc::new(TaskOutcome::default());
        let Some(tasks) = &executor.tasks else {
            outcome.done.store(true, Ordering::Release);
            return Self::from_task(Box::new(task), outcome, clock);
        };
        let record = Arc::clone(&outcome);
        let (sender, receiver) = mpsc::sync_channel(1);
        ThreadPool::submit(
            tasks,
            Box::new(move || {
                let result = catch_unwind(AssertUnwindSafe(&task));
                if let Err(payload) = &result {
                    record.fail(&**payload);
                }
                record.done.store(true, Ordering::Release);
                let _ = sender.send(result);
            }),
        );
        let failed = Arc::clone(&outcome);
        let receiver = Mutex::new(receiver);
        // `create()`: `task.get()`, waiting for the task thread. The callable calls this again only
        // after a panic, which then repeats the task's failure.
        let create = move || {
            failed.rethrow();
            let result = receiver
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv();
            match result {
                Ok(result) => result.unwrap_or_else(|payload| resume_unwind(payload)),
                Err(_) => {
                    failed.rethrow();
                    unreachable!("the task thread sends its outcome once")
                }
            }
        };
        Self::from_task(Box::new(create), outcome, clock)
    }
}

impl<T: Send + Sync + 'static> CacheEntry<'static, T> {
    /// `then(executor, function)`: `computeAsync(() -> function.apply(this.get()), executor)`, so
    /// the new entry's task gets this entry (stamping its access time, and panicking if it failed)
    /// on the executor's thread. The new entry reads this entry's clock.
    pub fn then<V: Send + 'static>(
        self: &Arc<Self>,
        executor: &ThreadPool,
        function: impl Fn(&T) -> V + Send + Sync + 'static,
    ) -> CacheEntry<'static, V> {
        let parent = Arc::clone(self);
        CacheEntry::compute_async_with_clock(move || function(parent.get()), executor, self.clock)
    }
}

impl<T> ExpiringEntry for CacheEntry<'_, T> {
    fn timestamp(&self) -> i64 {
        self.timestamp.load(Ordering::Acquire)
    }

    /// `close()`: closes the value with [`Self::closing`]'s function, if the task has produced it
    /// (Engine reads the callable's value, still `null` while the task has not run or is running).
    fn close(&self) {
        if let (Some(close), Some(value)) = (self.on_close, self.value.value.get()) {
            close(value);
        }
    }
}

/// `Rivermap implements ExpiringEntry`: its creation time; `close` does nothing.
impl ExpiringEntry for Rivermap {
    fn timestamp(&self) -> i64 {
        Rivermap::timestamp(self)
    }
}

/// The map behind [`BoundedLongMap`]: the values and their keys in insertion order (Engine's
/// `Long2ObjectLinkedOpenHashMap`).
struct LongMapState<T> {
    values: HashMap<i64, Arc<T>>,
    order: VecDeque<i64>,
}

/// Engine 0.3.0's `StampedBoundLongMap` (`com.terraforged.engine.concurrent.cache.map`), the map
/// behind [`Cache`]: `long` keys in insertion order under one read-write lock. Only
/// `compute_if_absent` keeps the bound: a miss on a map holding `capacity` or more values first
/// drops the oldest insertion, without closing it, even if another thread inserted the key in the
/// meantime (Engine's order). Hits, `get` and `put` of a present key keep the order, and `put`
/// adds without a bound. As in Engine, the factory runs under the write lock (other callers,
/// readers too, wait for it), so a factory that uses its own map never returns.
pub struct BoundedLongMap<T> {
    capacity: usize,
    state: RwLock<LongMapState<T>>,
}

impl<T> BoundedLongMap<T> {
    /// `new StampedBoundLongMap(size)`. Capacity 0 is kept as in Engine: the first miss on an empty
    /// map panics, where Engine's `removeFirst` throws `NoSuchElementException`.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: RwLock::new(LongMapState {
                values: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            }),
        }
    }

    // A factory or predicate that panicked under the lock changed nothing after its eviction, so a
    // poisoned map is still consistent (Engine's lock has no poisoning).
    fn read(&self) -> std::sync::RwLockReadGuard<'_, LongMapState<T>> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, LongMapState<T>> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.read().values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `clear()`.
    pub fn clear(&self) {
        let mut state = self.write();
        state.values.clear();
        state.order.clear();
    }

    /// `remove(key)`, returning the removed value so that the caller can act on it after the lock
    /// is released, as Engine's `remove(key, consumer)` does.
    pub fn remove(&self, key: i64) -> Option<Arc<T>> {
        let mut state = self.write();
        let value = state.values.remove(&key);
        if value.is_some() {
            state.order.retain(|entry| *entry != key);
        }
        value
    }

    /// `removeIf(predicate)`: removes, in insertion order, every value the predicate accepts, without
    /// closing it, and returns how many.
    pub fn remove_if(&self, predicate: impl Fn(&T) -> bool) -> usize {
        let mut state = self.write();
        let LongMapState { values, order } = &mut *state;
        let before = values.len();
        order.retain(|key| {
            let expired = values.get(key).is_some_and(|value| predicate(value));
            if expired {
                values.remove(key);
            }
            !expired
        });
        before - values.len()
    }

    /// `put(key, value)`: replaces a present key's value in place, else appends.
    pub fn put(&self, key: i64, value: T) {
        let mut state = self.write();
        if state.values.insert(key, Arc::new(value)).is_none() {
            state.order.push_back(key);
        }
    }

    /// `get(key)`.
    pub fn get(&self, key: i64) -> Option<Arc<T>> {
        self.read().values.get(&key).cloned()
    }

    /// `computeIfAbsent(key, func)`: a read-locked lookup, then under the write lock the bound's
    /// eviction and `computeIfAbsent`, which runs `factory(key)` only if the key is still absent.
    pub fn compute_if_absent(&self, key: i64, factory: impl FnOnce(i64) -> T) -> Arc<T> {
        if let Some(value) = self.get(key) {
            return value;
        }
        let mut state = self.write();
        if state.values.len() >= self.capacity {
            let first = state
                .order
                .pop_front()
                .expect("BoundedLongMap: compute_if_absent on an empty map of capacity 0");
            state.values.remove(&first);
        }
        if let Some(value) = state.values.get(&key) {
            return Arc::clone(value);
        }
        let value = Arc::new(factory(key));
        state.values.insert(key, Arc::clone(&value));
        state.order.push_back(key);
        value
    }

    /// `map(key, factory, mapper)`: `mapper` applied to `compute_if_absent(key, factory)`, after the
    /// lock is released.
    pub fn map<V>(
        &self,
        key: i64,
        factory: impl FnOnce(i64) -> T,
        mapper: impl FnOnce(&T) -> V,
    ) -> V {
        mapper(&self.compute_if_absent(key, factory))
    }
}

/// Engine 0.3.0's expiring `Cache` (`com.terraforged.engine.concurrent.cache`): [`ExpiringEntry`]
/// values by `long` key in a [`BoundedLongMap`], swept of the entries not used for `lifetime`
/// milliseconds.
///
/// The sweep is Engine's `run()`: `timeout = now - lifetime` in Java `long` arithmetic (it wraps:
/// a lifetime of `i64::MAX` from a negative clock gives a timeout near `i64::MAX`), then every
/// entry with `getTimestamp() < timeout` (strictly) is removed without being closed. Entries still
/// computing are swept like the others: a holder keeps its `Arc` and can still get the value, and
/// the next `compute_if_absent` makes a new entry. Only `remove` closes the entry, after releasing
/// the map's lock.
///
/// Engine's constructor schedules `run()` on its `CacheManager` scheduler thread every `interval`
/// milliseconds, the first one an interval after construction. That thread is not ported: here a
/// `compute_if_absent` (or `map`) on a cache whose last sweep (or construction) is at least an
/// interval away on the clock runs the sweep first, on the calling thread, once per interval
/// across threads. A cache nobody computes in is not swept; the map's capacity still bounds it.
/// Swept entries are recomputed on the next lookup, so when a sweep runs never changes the values
/// the cache hands out.
pub struct Cache<T> {
    name: String,
    map: BoundedLongMap<T>,
    lifetime_millis: i64,
    interval_millis: i64,
    /// Engine's `volatile long timeout`.
    timeout: AtomicI64,
    /// The clock at the last sweep (at construction until the first one).
    last_sweep: AtomicI64,
    clock: fn() -> i64,
}

impl<T: ExpiringEntry> Cache<T> {
    /// `new Cache(name, capacity, expireTime, interval, MILLISECONDS)` over a `StampedBoundLongMap`
    /// (Engine's default; its default capacity is 256).
    ///
    /// # Panics
    ///
    /// If `interval_millis` is not positive, where Engine's `scheduleAtFixedRate` throws
    /// `IllegalArgumentException`.
    pub fn new(
        name: impl Into<String>,
        capacity: usize,
        lifetime_millis: i64,
        interval_millis: i64,
    ) -> Self {
        Self::with_clock(
            name,
            capacity,
            lifetime_millis,
            interval_millis,
            current_time_millis,
        )
    }

    /// [`Self::new`] reading the time from `clock` in place of `System.currentTimeMillis()`.
    pub fn with_clock(
        name: impl Into<String>,
        capacity: usize,
        lifetime_millis: i64,
        interval_millis: i64,
        clock: fn() -> i64,
    ) -> Self {
        assert!(
            interval_millis > 0,
            "Cache: the sweep interval must be positive (Engine throws IllegalArgumentException)"
        );
        Self {
            name: name.into(),
            map: BoundedLongMap::new(capacity),
            lifetime_millis,
            interval_millis,
            timeout: AtomicI64::new(0),
            last_sweep: AtomicI64::new(clock()),
            clock,
        }
    }

    /// `getName()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `remove(key)`: removes the key's entry and closes it, outside the map's lock.
    pub fn remove(&self, key: i64) {
        if let Some(entry) = self.map.remove(key) {
            entry.close();
        }
    }

    /// `get(key)`: the key's entry, if cached; never sweeps.
    pub fn get(&self, key: i64) -> Option<Arc<T>> {
        self.map.get(key)
    }

    /// `computeIfAbsent(key, func)`: the key's entry, made by `factory(key)` under the map's write
    /// lock on a miss (see [`BoundedLongMap::compute_if_absent`]); runs a due sweep first.
    pub fn compute_if_absent(&self, key: i64, factory: impl FnOnce(i64) -> T) -> Arc<T> {
        self.run_if_due();
        self.map.compute_if_absent(key, factory)
    }

    /// `map(key, func, mapper)`: `mapper` applied to `compute_if_absent(key, factory)`.
    pub fn map<V>(
        &self,
        key: i64,
        factory: impl FnOnce(i64) -> T,
        mapper: impl FnOnce(&T) -> V,
    ) -> V {
        mapper(&self.compute_if_absent(key, factory))
    }

    /// `run()`: the sweep, now; returns how many entries it removed.
    pub fn run(&self) -> usize {
        self.run_at((self.clock)())
    }

    /// `test(entry)`: whether the last sweep's timeout expires `entry`.
    pub fn test(&self, entry: &T) -> bool {
        entry.timestamp() < self.timeout.load(Ordering::Acquire)
    }

    fn run_at(&self, now: i64) -> usize {
        self.timeout
            .store(now.wrapping_sub(self.lifetime_millis), Ordering::Release);
        self.map.remove_if(|entry| self.test(entry))
    }

    /// The scheduler's tick: sweeps if the clock is an interval or more from the last sweep (either
    /// way, so a clock set back does not stop the sweeps), claiming it so only one caller sweeps.
    fn run_if_due(&self) {
        let now = (self.clock)();
        let last = self.last_sweep.load(Ordering::Acquire);
        if now.abs_diff(last) >= self.interval_millis.unsigned_abs()
            && self
                .last_sweep
                .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            self.run_at(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, mpsc};
    use std::thread;
    use std::time::Duration;

    /// `LazyCallableOracle.java`'s `Probe`: counts its `create` calls, panics (Engine: throws) for
    /// the first `throw_first` of them and, when `gate` is set, signals the first one's start and
    /// waits for a release before finishing it.
    struct Probe {
        creates: AtomicUsize,
        throw_first: usize,
        gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    }

    impl Probe {
        fn new(throw_first: usize) -> Self {
            Self {
                creates: AtomicUsize::new(0),
                throw_first,
                gate: Mutex::new(None),
            }
        }

        fn create(&self) -> i32 {
            let n = self.creates.fetch_add(1, Ordering::SeqCst) + 1;
            let gate = self.gate.lock().unwrap().take();
            if let Some((started, release)) = gate {
                started.send(()).unwrap();
                release.recv().unwrap();
            }
            assert!(n > self.throw_first, "create {n}");
            100 + n as i32
        }

        /// Gates the next `create`: returns its start signal and its release.
        fn gate(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            *self.gate.lock().unwrap() = Some((started_tx, release_rx));
            (started_rx, release_tx)
        }
    }

    fn panics<R>(f: impl FnOnce() -> R) -> bool {
        catch_unwind(AssertUnwindSafe(f)).is_err()
    }

    /// Runs `f` on its own thread and reports whether it is still waiting 300 ms later, as the
    /// oracle checks the thread's state, then releases `release` and returns `f`'s result.
    fn blocked<R: Send>(release: mpsc::Sender<()>, f: impl FnOnce() -> R + Send) -> (bool, R) {
        thread::scope(|scope| {
            let handle = scope.spawn(f);
            thread::sleep(Duration::from_millis(300));
            let waiting = !handle.is_finished();
            release.send(()).unwrap();
            (waiting, handle.join().unwrap())
        })
    }

    /// Replays `LazyCallableOracle.java` (`.local/terraforged/oracles/lazy_callable/`), Engine
    /// 0.3.0's `LazyCallable` as its callers see it: never cancelled, created once for 16
    /// concurrent callers that all see the same value, created again after a throwing `create`,
    /// and `isDone` and `get` waiting for another thread's `create`, then seeing its outcome.
    #[test]
    fn lazy_callable_matches_engine_oracle() {
        let mut out = String::new();
        let probe = Probe::new(0);
        let lazy = LazyCallable::new(|| probe.create());
        let flags = |lazy: &LazyCallable<i32, _>| {
            format!(
                "f {} {} {} {}\n",
                lazy.is_done(),
                lazy.cancel(true),
                lazy.cancel(false),
                lazy.is_cancelled()
            )
        };
        out += &flags(&lazy);
        lazy.get();
        out += &flags(&lazy);

        let probe = Probe::new(0);
        let lazy = LazyCallable::new(|| probe.create());
        let barrier = Barrier::new(16);
        let seen: Vec<usize> = thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        std::ptr::from_ref(lazy.get()) as usize
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let distinct = seen.iter().filter(|&&p| p != seen[0]).count() + 1;
        let creates = probe.creates.load(Ordering::SeqCst);
        out += &format!("c 16 {creates} {distinct}\n");

        let probe = Probe::new(1);
        let lazy = LazyCallable::new(|| probe.create());
        let threw = panics(|| *lazy.get());
        let done_after = lazy.is_done();
        let second = *lazy.get();
        let after_second = probe.creates.load(Ordering::SeqCst);
        let third = *lazy.call();
        let creates = probe.creates.load(Ordering::SeqCst);
        let done = lazy.is_done();
        out +=
            &format!("e {threw} {done_after} {second} {after_second} {third} {creates} {done}\n");

        for throw in 0..=1 {
            let probe = Probe::new(throw);
            let lazy = LazyCallable::new(|| probe.create());
            let (started, release) = probe.gate();
            let (waiting, done) = thread::scope(|scope| {
                scope.spawn(|| panics(|| *lazy.get()));
                started.recv().unwrap();
                blocked(release, || lazy.is_done())
            });
            out += &format!("d {throw} {waiting} {done}\n");
        }

        let probe = Probe::new(1);
        let lazy = LazyCallable::new(|| probe.create());
        let (started, release) = probe.gate();
        let (creator_threw, (waiting, waited)) = thread::scope(|scope| {
            let creator = scope.spawn(|| panics(|| *lazy.get()));
            started.recv().unwrap();
            let waiter = blocked(release, || *lazy.get());
            (creator.join().unwrap(), waiter)
        });
        let creates = probe.creates.load(Ordering::SeqCst);
        out += &format!("w {creator_threw} {waiting} {waited} {creates}\n");
        assert_eq!(out, ORACLE);
    }

    /// A callable is shared between threads whenever its value and `create` are.
    #[test]
    fn lazy_callables_are_shared_between_threads() {
        fn shareable<T: Send + Sync>(_: &T) {}
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let lazy = LazyCallable::new(move || counted.fetch_add(1, Ordering::SeqCst));
        shareable(&lazy);
        assert_eq!(*lazy.get(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// `LazyCallableOracle.java`'s output (`./run.sh` writes it to `out.txt`).
    const ORACLE: &str = "\
f false false false false
f true false false false
c 16 1 1
e true false 102 2 102 2 true
d 0 true true
d 1 true false
w true true 102 2
";

    /// Replays `CacheEntryOracle.java` (`.local/terraforged/oracles/cache_entry/`), Engine 0.3.0's
    /// `CacheEntry` over a task its first `get` runs: never cancelled, done only once the task ran;
    /// the timestamp set at construction and at the start of each `get`, before the task runs, and
    /// left alone by `isDone` and `call`; a throwing task done, failing every later `get`/`call`
    /// without running again; 16 concurrent getters sharing one run; and `isDone` answering at once,
    /// `false`, while another thread runs the task. The clock is injected where the oracle sleeps.
    #[test]
    fn cache_entry_matches_engine_oracle() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        let mut out = String::new();
        let flags = |entry: &CacheEntry<i32>| {
            format!(
                "f {} {} {} {}\n",
                entry.is_done(),
                entry.cancel(true),
                entry.cancel(false),
                entry.is_cancelled()
            )
        };
        let runs = AtomicUsize::new(0);
        let entry = CacheEntry::new(|| 100 + runs.fetch_add(1, Ordering::SeqCst) as i32 + 1);
        out += &flags(&entry);
        let value = *entry.get();
        out += &flags(&entry);
        let runs_now = runs.load(Ordering::SeqCst);
        out += &format!("v {value} {} {} {runs_now}\n", entry.get(), entry.call());

        static SEEN: AtomicI64 = AtomicI64::new(0);
        static ENTRY: OnceLock<CacheEntry<'static, i32>> = OnceLock::new();
        NOW.store(1000, Ordering::SeqCst);
        let entry = ENTRY.get_or_init(|| {
            CacheEntry::with_clock(
                || {
                    let stamp = ENTRY.get().unwrap().timestamp();
                    SEEN.store(stamp, Ordering::SeqCst);
                    7
                },
                now,
            )
        });
        let t0 = entry.timestamp();
        NOW.store(2000, Ordering::SeqCst);
        entry.is_done();
        let done_keeps = entry.timestamp() == t0;
        NOW.store(3000, Ordering::SeqCst);
        entry.get();
        let t1 = entry.timestamp();
        let get_sets = t1 == 3000 && t1 > t0;
        let before_task = SEEN.load(Ordering::SeqCst) == t1;
        NOW.store(4000, Ordering::SeqCst);
        entry.call();
        let call_keeps = entry.timestamp() == t1;
        out += &format!(
            "t {} {done_keeps} {get_sets} {before_task} {call_keeps}\n",
            t0 == 1000
        );

        let runs = AtomicUsize::new(0);
        let bad = CacheEntry::new(|| -> i32 {
            runs.fetch_add(1, Ordering::SeqCst);
            panic!("boom")
        });
        let first = panics(|| *bad.get());
        let done = bad.is_done();
        let second = panics(|| *bad.get());
        let third = panics(|| *bad.call());
        let runs_now = runs.load(Ordering::SeqCst);
        out += &format!(
            "e {first} {done} {second} {third} {runs_now} {}\n",
            bad.is_done()
        );

        let runs = AtomicUsize::new(0);
        let shared = CacheEntry::new(|| {
            runs.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(20));
            Box::new(0_u8)
        });
        let barrier = Barrier::new(16);
        let seen: Vec<usize> = thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        std::ptr::from_ref(&**shared.get()) as usize
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let distinct = seen.iter().filter(|&&p| p != seen[0]).count() + 1;
        out += &format!("c 16 {} {distinct}\n", runs.load(Ordering::SeqCst));

        for throw in 0..=1 {
            let (started_tx, started) = mpsc::channel();
            let (release, release_rx) = mpsc::channel::<()>();
            let (started_tx, release_rx) = (Mutex::new(started_tx), Mutex::new(release_rx));
            let slow = CacheEntry::new(|| {
                started_tx.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                assert!(throw == 0, "thrown");
                5
            });
            let (during, quick) = thread::scope(|scope| {
                let creator = scope.spawn(|| panics(|| *slow.get()));
                started.recv().unwrap();
                let start = std::time::Instant::now();
                let during = slow.is_done();
                let quick = start.elapsed() < Duration::from_millis(200);
                release.send(()).unwrap();
                creator.join().unwrap();
                (during, quick)
            });
            out += &format!("d {throw} {during} {quick} {}\n", slow.is_done());
        }
        assert_eq!(out, CACHE_ENTRY_ORACLE);
    }

    /// The timestamp is the clock's Java `long` as read, never wrapped, and an entry is shared
    /// between threads whenever its value is.
    #[test]
    fn cache_entries_keep_extreme_timestamps_and_are_shared() {
        fn shareable<T: Send + Sync>(_: &T) {}
        let clocks: [fn() -> i64; 3] = [|| i64::MIN, || -1, || i64::MAX];
        for clock in clocks {
            let entry = CacheEntry::with_clock(|| 1, clock);
            shareable(&entry);
            assert_eq!(entry.timestamp(), clock());
            assert_eq!(*entry.get(), 1);
            assert_eq!(entry.timestamp(), clock());
            entry.close();
        }
        let entry = CacheEntry::new(|| 2);
        assert!(entry.timestamp() > 0);
    }

    /// `CacheEntryOracle.java`'s output (`./run.sh` writes it to `out.txt`).
    const CACHE_ENTRY_ORACLE: &str = "\
f false false false false
f true false false false
v 101 101 101 1
t true true true true true
e true true true true 1 true
c 16 1 1
d 0 false true true
d 1 false true true
";

    /// Replays `CacheEntryOracle.java`'s async lines: Engine 0.3.0's `CacheEntry.computeAsync` and
    /// `then` on `ThreadPools.create(4)` and on `ThreadPools.NONE`. On the pool the task runs on a
    /// `TF-Task-Worker` thread (two at once), `isDone` is `false` while it runs and `get` waits for
    /// it; a failure (the task's, or the parent's under `then`) is final. The timestamp is set at
    /// creation and by `get` only; `then`'s task gets the parent, stamping it. On `NONE` the entry is
    /// done at once, the task runs on the first getter's thread and a failure runs it again. The
    /// clock is injected where the oracle sleeps.
    #[test]
    fn async_cache_entry_matches_engine_oracle() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        fn worker() -> bool {
            thread::current()
                .name()
                .is_some_and(|name| name.starts_with("TF-Task-Worker-"))
        }
        let mut out = String::new();
        let pool = ThreadPool::create(4);

        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let (started_tx, release_rx) = (Mutex::new(started_tx), Mutex::new(release_rx));
        let ran = Arc::new(Mutex::new(String::new()));
        let ran_on = Arc::clone(&ran);
        let a = CacheEntry::compute_async(
            move || {
                *ran_on.lock().unwrap() = thread::current().name().unwrap().to_owned();
                started_tx.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                9
            },
            &pool,
        );
        started.recv().unwrap();
        let during = a.is_done();
        let (blocked_get, got) = blocked(release, || *a.get());
        out += &format!(
            "a {during} {blocked_get} {} {} {got} {} {}\n",
            a.is_done(),
            ran.lock().unwrap(),
            a.cancel(true),
            a.is_cancelled()
        );

        let (begun_tx, begun) = mpsc::channel();
        let (go, go_rx) = mpsc::channel::<()>();
        let (begun_tx, go_rx) = (Mutex::new(begun_tx), Arc::new(Mutex::new(go_rx)));
        let begun_tx = Arc::new(begun_tx);
        let busy: Vec<_> = (0..3)
            .map(|_| {
                let (begun_tx, go_rx) = (Arc::clone(&begun_tx), Arc::clone(&go_rx));
                CacheEntry::compute_async(
                    move || {
                        begun_tx.lock().unwrap().send(()).unwrap();
                        go_rx.lock().unwrap().recv().unwrap();
                        worker()
                    },
                    &pool,
                )
            })
            .collect();
        begun.recv().unwrap();
        begun.recv().unwrap();
        // The third task waits for a free task thread: none starts it within 100 ms.
        let at_once = 2 + usize::from(begun.recv_timeout(Duration::from_millis(100)).is_ok());
        for _ in 0..3 {
            go.send(()).unwrap();
        }
        out += &format!("s {at_once}");
        for entry in &busy {
            out += &format!(" {}", entry.get());
        }
        out += "\n";

        NOW.store(1000, Ordering::SeqCst);
        let u = CacheEntry::compute_async_with_clock(|| 3, &pool, now);
        let t0 = u.timestamp();
        NOW.store(2000, Ordering::SeqCst);
        u.call();
        let end_keeps = u.timestamp() == t0;
        NOW.store(3000, Ordering::SeqCst);
        u.get();
        let t1 = u.timestamp();
        NOW.store(4000, Ordering::SeqCst);
        u.call();
        out += &format!(
            "u {} {end_keeps} {} {}\n",
            t0 == 1000,
            t1 == 3000,
            u.timestamp() == t1
        );

        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let x = CacheEntry::compute_async(
            move || -> i32 {
                counted.fetch_add(1, Ordering::SeqCst);
                panic!("boom")
            },
            &pool,
        );
        let first = panics(|| *x.get());
        let second = panics(|| *x.get());
        let third = panics(|| *x.call());
        let runs_now = runs.load(Ordering::SeqCst);
        out += &format!("x {} {first} {second} {third} {runs_now}\n", x.is_done());

        NOW.store(1000, Ordering::SeqCst);
        let parent = Arc::new(CacheEntry::compute_async_with_clock(|| 20, &pool, now));
        parent.get();
        let p0 = parent.timestamp();
        NOW.store(2000, Ordering::SeqCst);
        let child = parent.then(&pool, |value| (value + 1, worker()));
        let (value, on_worker) = *child.get();
        out += &format!(
            "n {value} {on_worker} {} {}\n",
            parent.timestamp() > p0,
            child.is_done()
        );

        let runs = Arc::new(AtomicUsize::new(0));
        let applied = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let bad_parent = Arc::new(CacheEntry::compute_async(
            move || -> i32 {
                counted.fetch_add(1, Ordering::SeqCst);
                panic!("parent")
            },
            &pool,
        ));
        let applies = Arc::clone(&applied);
        let bad_child = bad_parent.then(&pool, move |value| {
            applies.fetch_add(1, Ordering::SeqCst);
            *value
        });
        let (c1, c2) = (panics(|| *bad_child.get()), panics(|| *bad_child.get()));
        let applies = Arc::clone(&applied);
        let throwing = parent.then(&pool, move |_| -> i32 {
            applies.fetch_add(1, Ordering::SeqCst);
            panic!("function")
        });
        let (c3, c4) = (panics(|| *throwing.get()), panics(|| *throwing.get()));
        out += &format!(
            "m {c1} {c2} {} {} {c3} {c4} {} {}\n",
            bad_child.is_done(),
            runs.load(Ordering::SeqCst),
            throwing.is_done(),
            applied.load(Ordering::SeqCst)
        );

        let none = ThreadPool::NONE;
        let caller = thread::current().id();
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let z = CacheEntry::compute_async(
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
                (4, thread::current().id())
            },
            &none,
        );
        let before = z.is_done();
        let runs_before = runs.load(Ordering::SeqCst);
        let (value, ran_on) = *z.get();
        z.get();
        out += &format!(
            "z {before} {runs_before} {value} {} {}\n",
            ran_on == caller,
            runs.load(Ordering::SeqCst)
        );
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let zb = CacheEntry::compute_async(
            move || -> i32 {
                counted.fetch_add(1, Ordering::SeqCst);
                panic!("none")
            },
            &none,
        );
        let (z1, z2, z3) = (
            panics(|| *zb.get()),
            panics(|| *zb.get()),
            panics(|| *zb.call()),
        );
        out += &format!(
            "z {} {z1} {z2} {z3} {}\n",
            zb.is_done(),
            runs.load(Ordering::SeqCst)
        );
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let zp = Arc::new(CacheEntry::compute_async(
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
                6
            },
            &none,
        ));
        let zc = zp.then(&none, |value| (value * 2, thread::current().id()));
        let runs_then = runs.load(Ordering::SeqCst);
        let (value, ran_on) = *zc.get();
        out += &format!(
            "z {} {runs_then} {value} {} {}\n",
            zc.is_done(),
            ran_on == caller,
            runs.load(Ordering::SeqCst)
        );

        out += &format!(
            "p {} {} {} {}\n",
            pool.size(),
            none.size(),
            ThreadPool::create(1).size(),
            ThreadPool::create(7).size()
        );
        assert_eq!(out, ASYNC_CACHE_ENTRY_ORACLE);
    }

    /// `CacheEntryOracle.java`'s async lines, after [`CACHE_ENTRY_ORACLE`]'s in `out.txt`.
    const ASYNC_CACHE_ENTRY_ORACLE: &str = "\
a false true true TF-Task-Worker-1 9 false false
s 2 true true true
u true true true true
x true true true true 1
n 21 true true true
m true true true 1 true true true 1
z true 0 4 true 1
z true true true true 3
z true 0 12 true 1
p 6 0 3 10
";

    /// An expiring `Cache` of async entries sweeps one whose task still runs; its holder still gets
    /// the value, and the next lookup makes a new entry. Extreme pool sizes keep Java's int math.
    #[test]
    fn caches_sweep_running_async_entries() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        let pool = ThreadPool::create(2);
        let cache: Cache<CacheEntry<'static, i32>> = Cache::with_clock("T", 4, 60, 20, now);
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let held = cache.compute_if_absent(1, |_| {
            CacheEntry::compute_async_with_clock(
                move || {
                    released.lock().unwrap().recv().unwrap();
                    42
                },
                &pool,
                now,
            )
        });
        NOW.store(100, Ordering::SeqCst);
        let other = cache.compute_if_absent(2, |_| {
            CacheEntry::compute_async_with_clock(|| 7, &pool, now)
        });
        assert!(cache.get(1).is_none());
        assert!(!held.is_done());
        release.send(()).unwrap();
        assert_eq!(*held.get(), 42);
        assert_eq!(*other.get(), 7);
        let again = cache.compute_if_absent(1, |_| {
            CacheEntry::compute_async_with_clock(|| 43, &pool, now)
        });
        assert_eq!(*again.get(), 43);

        assert_eq!(
            ThreadPool::create(i32::MAX).size(),
            (i32::MAX / 2).wrapping_add(i32::MAX)
        );
        assert_eq!(ThreadPool::create(i32::MIN).size(), 3);
        assert_eq!(
            *CacheEntry::compute_async(|| 5, &ThreadPool::create(i32::MAX)).get(),
            5
        );
    }
    /// `CacheOracle.java`'s `Entry`: a fixed timestamp and a count of its `close` calls.
    struct TestEntry {
        id: i64,
        timestamp: i64,
        closes: AtomicUsize,
    }

    impl TestEntry {
        fn new(id: i64, timestamp: i64) -> Self {
            Self {
                id,
                timestamp,
                closes: AtomicUsize::new(0),
            }
        }

        fn closes(&self) -> usize {
            self.closes.load(Ordering::SeqCst)
        }
    }

    impl ExpiringEntry for TestEntry {
        fn timestamp(&self) -> i64 {
            self.timestamp
        }

        fn close(&self) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Which of `candidates` the cache holds, as `CacheOracle.keys` prints them.
    fn keys<T: ExpiringEntry>(cache: &Cache<T>, candidates: &[i64]) -> String {
        let held: Vec<String> = candidates
            .iter()
            .filter(|key| cache.get(**key).is_some())
            .map(i64::to_string)
            .collect();
        if held.is_empty() {
            "-".to_owned()
        } else {
            held.join(",")
        }
    }

    const DAY: i64 = 86_400_000;

    /// Engine 0.3.0's `Cache` over `StampedBoundLongMap` against `CacheOracle.java` (in
    /// `.local/terraforged/oracles/cache/`), with the oracle's injected clock: the sweep's wrapping
    /// timeout and strict `<`, the bound's eviction order, `map`, `remove`'s single close, a swept
    /// entry whose task never ran, the factory under the write lock, and capacity 0.
    #[test]
    fn cache_matches_engine_oracle() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        let mut out = String::new();
        let cases: [(i64, i64); 8] = [
            (1000, 60),
            (1000, 0),
            (1000, -5),
            (1000, i64::MAX),
            (-10, i64::MAX),
            (1000, i64::MIN),
            (i64::MIN, 1),
            (i64::MAX, -1),
        ];
        for (at, life) in cases {
            NOW.store(0, Ordering::SeqCst);
            let cache = Cache::with_clock("S", 16, life, DAY, now);
            let timeout = at.wrapping_sub(life);
            let stamps = [
                timeout.wrapping_sub(1),
                timeout,
                timeout.wrapping_add(1),
                i64::MIN,
                i64::MAX,
            ];
            let entries: Vec<_> = (0..)
                .zip(stamps)
                .map(|(id, stamp)| cache.compute_if_absent(id, |key| TestEntry::new(key, stamp)))
                .collect();
            NOW.store(at, Ordering::SeqCst);
            let removed = cache.run();
            let closes: usize = entries.iter().map(|entry| entry.closes()).sum();
            let held = keys(&cache, &[0, 1, 2, 3, 4]);
            out += &format!("s {at} {life} {timeout} {removed} {held} {closes}\n");
        }

        NOW.store(0, Ordering::SeqCst);
        let cache = Cache::with_clock("E", 3, 60, DAY, now);
        let calls = AtomicUsize::new(0);
        let mut made: Vec<Arc<TestEntry>> = Vec::new();
        for key in [1, 2, 3, 2, 4, 1, 5, 5] {
            let entry = cache.compute_if_absent(key, |id| {
                calls.fetch_add(1, Ordering::SeqCst);
                TestEntry::new(id, 0)
            });
            let held = keys(&cache, &[1, 2, 3, 4, 5]);
            let calls = calls.load(Ordering::SeqCst);
            out += &format!("e {key} {} {calls} {held}\n", entry.id);
            if !made.iter().any(|seen| Arc::ptr_eq(seen, &entry)) {
                made.push(entry);
            }
        }
        // Every entry made, the evicted ones too: eviction closes none.
        let closes: usize = made.iter().map(|entry| entry.closes()).sum();
        let mapped = cache.map(4, |_| unreachable!(), |entry| entry.id * 10);
        out += &format!("m {mapped} {closes}\n");
        let four = cache.get(4).unwrap();
        cache.remove(4);
        cache.remove(4);
        cache.remove(99);
        let held = keys(&cache, &[1, 2, 3, 4, 5]);
        out += &format!("r {} {held} {}\n", four.closes(), cache.get(99).is_none());
        let six = cache.compute_if_absent(6, |id| TestEntry::new(id, 0));
        let seven = cache.compute_if_absent(7, |id| TestEntry::new(id, 0));
        let held = keys(&cache, &[1, 2, 3, 4, 5, 6, 7]);
        out += &format!("r {held} {} {}\n", six.closes(), seven.closes());

        NOW.store(0, Ordering::SeqCst);
        let runs = AtomicUsize::new(0);
        let cache: Cache<CacheEntry<'_, i32>> = Cache::with_clock("U", 4, 60, DAY, now);
        let made = AtomicUsize::new(0);
        let held = cache.compute_if_absent(9, |_| {
            made.fetch_add(1, Ordering::SeqCst);
            CacheEntry::with_clock(
                || {
                    runs.fetch_add(1, Ordering::SeqCst);
                    42
                },
                now,
            )
        });
        let done_before = held.is_done();
        NOW.store(i64::MAX, Ordering::SeqCst);
        let removed = cache.run();
        let gone = cache.get(9).is_none();
        let again = cache.compute_if_absent(9, |_| {
            made.fetch_add(1, Ordering::SeqCst);
            CacheEntry::with_clock(|| 43, now)
        });
        out += &format!(
            "u {done_before} {removed} {gone} {} {} {}\n",
            !Arc::ptr_eq(&again, &held),
            made.load(Ordering::SeqCst),
            runs.load(Ordering::SeqCst)
        );
        // Not in the oracle's line: the swept entry's holder still gets its value.
        assert_eq!(*held.get(), 42);
        assert_eq!(*again.get(), 43);

        NOW.store(0, Ordering::SeqCst);
        let cache = Cache::with_clock("L", 4, 60, DAY, now);
        cache.compute_if_absent(1, |id| TestEntry::new(id, 0));
        let (in_factory, factory_started) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let cache = &cache;
        let (waited, found) = thread::scope(|scope| {
            let maker = scope.spawn(move || {
                cache.compute_if_absent(2, |id| {
                    in_factory.send(()).unwrap();
                    released.recv().unwrap();
                    TestEntry::new(id, 0)
                });
            });
            factory_started.recv().unwrap();
            let (read_done, read) = mpsc::channel();
            scope.spawn(move || read_done.send(cache.get(1).is_some()).unwrap());
            let waited = read.recv_timeout(Duration::from_millis(300)).is_err();
            release.send(()).unwrap();
            let found = read.recv_timeout(Duration::from_secs(10)).unwrap();
            maker.join().unwrap();
            (waited, found)
        });
        out += &format!("l {waited} {found}\n");

        let zero = catch_unwind(|| {
            Cache::with_clock("Z", 0, 60, DAY, now)
                .compute_if_absent(1, |id| TestEntry::new(id, 0));
        });
        out += &format!(
            "x cap0 {}\n",
            if zero.is_err() {
                "NoSuchElementException"
            } else {
                "ok"
            }
        );
        for interval in [0, -1] {
            let made = catch_unwind(|| Cache::<TestEntry>::with_clock("I", 4, 60, interval, now));
            let result = if made.is_err() {
                "IllegalArgumentException"
            } else {
                "ok"
            };
            out += &format!("x interval {interval} {result}\n");
        }
        assert_eq!(out, CACHE_ORACLE);
    }

    /// `CacheOracle.java`'s output (`./run.sh` writes it to `out.txt`).
    const CACHE_ORACLE: &str = "\
s 1000 60 940 2 1,2,4 0
s 1000 0 1000 2 1,2,4 0
s 1000 -5 1005 2 1,2,4 0
s 1000 9223372036854775807 -9223372036854774807 2 1,2,4 0
s -10 9223372036854775807 9223372036854775799 2 1,2,4 0
s 1000 -9223372036854775808 -9223372036854774808 2 1,2,4 0
s -9223372036854775808 1 9223372036854775807 3 1,4 0
s 9223372036854775807 -1 -9223372036854775808 0 0,1,2,3,4 0
e 1 1 1 1
e 2 2 2 1,2
e 3 3 3 1,2,3
e 2 2 3 1,2,3
e 4 4 4 2,3,4
e 1 1 5 1,3,4
e 5 5 6 1,4,5
e 5 5 6 1,4,5
m 40 0
r 1 1,5 true
r 5,6,7 0 0
u false 1 true true 2 0
l true true
x cap0 NoSuchElementException
x interval 0 IllegalArgumentException
x interval -1 IllegalArgumentException
";

    /// Engine's scheduler, ported as a sweep that `compute_if_absent` runs once the clock is an
    /// interval or more from the last sweep (or construction, or after the clock is set back), and
    /// that `get` never runs.
    #[test]
    fn cache_sweeps_once_per_interval_on_compute() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        NOW.store(10_000, Ordering::SeqCst);
        let cache = Cache::with_clock("T", 8, 100, 1_000, now);
        cache.compute_if_absent(1, |id| TestEntry::new(id, 0));
        NOW.store(10_999, Ordering::SeqCst);
        cache.compute_if_absent(2, |id| TestEntry::new(id, 10_999));
        assert!(cache.get(1).is_some(), "no sweep before an interval");
        NOW.store(11_000, Ordering::SeqCst);
        assert!(cache.get(1).is_some(), "get never sweeps");
        cache.compute_if_absent(2, |_| unreachable!());
        assert!(cache.get(1).is_none(), "a hit runs the due sweep too");
        assert!(cache.get(2).is_some());
        // The next sweep is due an interval after this one, not after construction.
        cache.compute_if_absent(3, |id| TestEntry::new(id, 0));
        NOW.store(11_999, Ordering::SeqCst);
        cache.compute_if_absent(4, |id| TestEntry::new(id, 0));
        assert!(cache.get(3).is_some());
        // A clock set back an interval or more sweeps as well.
        NOW.store(10_000, Ordering::SeqCst);
        cache.map(5, |id| TestEntry::new(id, 10_000), |entry| entry.id);
        assert_eq!(keys(&cache, &[1, 2, 3, 4, 5]), "2,5");
        // A sweep removes without closing; `run` sweeps at once.
        NOW.store(20_000, Ordering::SeqCst);
        let two = cache.get(2).unwrap();
        assert_eq!(cache.run(), 2);
        assert_eq!(two.closes(), 0);
        assert!(cache.test(&TestEntry::new(0, 19_899)));
        assert!(!cache.test(&TestEntry::new(0, 19_900)));
        assert_eq!(cache.name(), "T");
    }

    /// `Rivermap` is an `ExpiringEntry` by its creation time, and a cache of them (Engine's
    /// `RiverCache` holds one) is shared between threads.
    #[test]
    fn rivermaps_expire_by_creation_time() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW.load(Ordering::SeqCst)
        }
        fn shareable<T: Send + Sync>(_: &T) {}
        use super::super::terraforged_river::GenWarp;
        let cache = Cache::with_clock("RiverCache", 32, 300_000, 60_000, now);
        shareable(&cache);
        let map = cache.compute_if_absent(7, |_| {
            Rivermap::new(1, 2, Vec::new(), GenWarp::new(1, 3000))
        });
        let made = Rivermap::timestamp(&map);
        assert_eq!(ExpiringEntry::timestamp(&*map), made);
        NOW.store(made + 300_000, Ordering::SeqCst);
        assert_eq!(cache.run(), 0);
        NOW.store(made + 300_001, Ordering::SeqCst);
        assert_eq!(cache.run(), 1);

        let map = BoundedLongMap::new(2);
        map.put(1, 10);
        map.put(2, 20);
        map.put(3, 30);
        map.put(1, 11);
        assert_eq!(map.len(), 3);
        assert_eq!(map.compute_if_absent(4, |_| 40).as_ref(), &40);
        assert!(map.get(1).is_none(), "put keeps a replaced key's place");
        assert_eq!(map.remove(2).as_deref(), Some(&20));
        assert_eq!(map.remove_if(|value| *value >= 30), 2);
        assert!(map.is_empty());
        map.put(5, 50);
        map.clear();
        assert!(map.is_empty());
    }
}
