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

use super::terraforged_river::current_time_millis;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

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
/// expires (the cache itself is a later layer).
pub trait ExpiringEntry {
    /// `getTimestamp()`: the entry's last access, in `System.currentTimeMillis()` milliseconds.
    fn timestamp(&self) -> i64;

    /// `close()`: run when the cache removes the entry; does nothing unless overridden.
    fn close(&self) {}
}

/// What Engine's `Future` records of the entry's task: whether it ended and, if it panicked
/// (Engine: threw), its message.
#[derive(Default)]
struct TaskOutcome {
    done: AtomicBool,
    failure: OnceLock<String>,
}

/// The entry's task, boxed so that `CacheEntry` can name the [`LazyCallable`] that runs it.
type Task<'a, T> = Box<dyn Fn() -> T + Send + Sync + 'a>;

/// Engine 0.3.0's `CacheEntry` (`com.terraforged.engine.concurrent.cache`): a [`LazyCallable`] over a
/// task, plus the last-access timestamp Engine's `Cache` expires it by.
///
/// The task runs on the first `get`/`call`, on the calling thread, like Engine's `CacheEntry` over a
/// task its first `task.get()` runs (Engine's async `computeAsync`/`then` are a later layer). As in
/// [`LazyCallable`], callers arriving while it runs wait for it and all see the same value. The
/// task runs at most once: like a failed Engine `Future`, a task that panicked makes every later
/// `get`/`call` panic too, without running it again, and `is_done` reports it done.
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
            if let Some(failure) = record.failure.get() {
                panic!("{failure}");
            }
            let result = catch_unwind(AssertUnwindSafe(&task));
            if let Err(payload) = &result {
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let _ = record
                    .failure
                    .set(format!("cache entry task panicked: {message}"));
            }
            record.done.store(true, Ordering::Release);
            result.unwrap_or_else(|payload| resume_unwind(payload))
        };
        Self {
            timestamp: AtomicI64::new(clock()),
            clock,
            outcome,
            value: LazyCallable::new(Box::new(create)),
        }
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

impl<T> ExpiringEntry for CacheEntry<'_, T> {
    fn timestamp(&self) -> i64 {
        self.timestamp.load(Ordering::Acquire)
    }

    /// Engine closes a `SafeCloseable` or `AutoCloseable` value here (a `Tile` hands its cells back to
    /// Engine's resource pools, which are not ported). Here the value is dropped with the entry.
    fn close(&self) {}
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
}
