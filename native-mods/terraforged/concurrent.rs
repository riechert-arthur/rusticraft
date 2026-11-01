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

use std::sync::OnceLock;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
}
