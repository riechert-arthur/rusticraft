/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
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

// `/tf regen`'s generation half. Java drops the area's `ChunkHolder`s and lets the next
// `chunkSource.tick` load them again; here the area's columns and proto chunks leave the
// streaming, and the next `update` generates them again.

use super::{BinaryHeap, Done, Job, Streaming};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Before `/tf regen` deletes the area from the save: no load or generation of its columns
/// may be queued or running, or one that read the save before the delete would bring the
/// old column back. Queued ones are dropped, running ones are waited for and their
/// results dropped (others wait for the next `update`, as in `Streaming::save`). False
/// when that work, or a decoration or save `Streaming::save` waited for, did not finish
/// in time: then nothing may be deleted.
pub fn settle(streaming: &mut Streaming, keys: &HashSet<(i32, i32)>) -> bool {
    if streaming.decorating > 0 || !streaming.finishing.is_empty() || streaming.saving > 0 {
        return false;
    }
    let ours = |done: &Done| match done {
        Done::Generated(chunk) => keys.contains(&(chunk.x, chunk.z)),
        Done::Proto(x, z, ..) => keys.contains(&(*x, *z)),
        _ => false,
    };
    {
        let mut queue = streaming.shared.queue.lock().unwrap();
        let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *queue)
            .into_vec()
            .into_iter()
            .partition(|q| matches!(q.job, Job::Generate(x, z) if keys.contains(&(x, z))));
        *queue = BinaryHeap::from(kept);
        for q in dropped {
            if let Job::Generate(x, z) = q.job {
                streaming.generating.remove(&(x, z));
            }
        }
    }
    let mut deferred = std::mem::take(&mut streaming.deferred);
    deferred.retain(|done| {
        let drop = ours(done);
        if let (true, Done::Generated(chunk)) = (drop, done) {
            streaming.generating.remove(&(chunk.x, chunk.z));
        } else if let (true, Done::Proto(x, z, ..)) = (drop, done) {
            streaming.generating.remove(&(*x, *z));
        }
        !drop
    });
    streaming.deferred = deferred;
    let deadline = Instant::now() + Duration::from_secs(30);
    while streaming.generating.iter().any(|key| keys.contains(key)) {
        let left = deadline.saturating_duration_since(Instant::now());
        match streaming.shared.done.recv_timeout(left) {
            Some(Done::Generated(chunk)) if keys.contains(&(chunk.x, chunk.z)) => {
                streaming.generating.remove(&(chunk.x, chunk.z));
            }
            Some(Done::Proto(x, z, ..)) if keys.contains(&(x, z)) => {
                streaming.generating.remove(&(x, z));
            }
            Some(other) => streaming.deferred.push(other),
            None => return false,
        }
    }
    true
}

/// After the delete: the area's proto chunks, and what the streaming knows of them, are
/// dropped, so the scan generates the area again (`Streaming::forget` drops the columns).
pub fn discard(streaming: &mut Streaming, keys: &HashSet<(i32, i32)>) {
    for key in keys {
        streaming.protos.remove(key);
        streaming.written.remove(key);
        streaming.decorated.remove(key);
        streaming.light_views.remove(key);
        streaming.light_snapshots.remove(key);
    }
    streaming.generation_scan.exhausted = false;
}
