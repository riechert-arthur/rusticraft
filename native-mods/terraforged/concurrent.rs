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

use std::sync::{Mutex, OnceLock, PoisonError};

/// Engine 0.3.0's `LazyCallable` (`com.terraforged.engine.concurrent.task`), the deferred value
/// behind `TileGenerator.getTile` (`CallableTile`, `CallableZoomTile`): `create` runs on the first
/// `call`/`get`, and every later caller, on any thread, sees that same value.
///
/// As with Engine's `StampedLock`, `create` runs under an exclusive lock: callers that arrive
/// while it runs wait for it, and so does `is_done`, which then reports whether it succeeded. A
/// `create` that panics (Engine: throws) stores nothing; the next caller, or one already waiting,
/// runs it again. Engine's `create` may not return null (`Objects.requireNonNull`); here it always
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
}
