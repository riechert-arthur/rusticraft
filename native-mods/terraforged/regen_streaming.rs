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
// `chunkSource.tick` load them again; here the area's columns leave the streaming
// (`Streaming::forget`), `Streaming::regenerate` drops its proto chunks and has its edge
// decorated against the chunks kept around it, and the next `update` generates it again.
// What `settle` waits for first is below.

use super::{BinaryHeap, Done, Job, Streaming};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Before `/tf regen` deletes the area from the save: no load or generation of its columns
/// may be queued or running, or one that read the save before the delete would bring the
/// old column back. Queued ones are dropped, running ones are waited for and their
/// results dropped (others wait for the next `update`, as in `Streaming::save`). False
/// when that work, or a decoration or save `Streaming::save` waited for, did not finish
/// in time: then nothing may be deleted. An edge decoration (`Done::DecoratedEdge`) counts
/// in `decorating` like any other, so none is out while the area settles.
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

#[cfg(test)]
mod tests {
    use super::super::{Chunk, ChunkGenerator, Job, Proto, Streaming, WorkerContext};
    use super::settle;
    use std::collections::HashSet;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    /// Loads that answer an empty column once `open`; each records its key when it starts.
    #[derive(Default)]
    struct Gated {
        state: Mutex<(bool, Vec<(i32, i32)>)>,
        changed: Condvar,
    }
    impl Gated {
        fn wait_started(&self, count: usize) -> Vec<(i32, i32)> {
            let state = self.state.lock().unwrap();
            let (state, timeout) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(10), |s| s.1.len() < count)
                .unwrap();
            assert!(!timeout.timed_out(), "started: {:?}", state.1);
            state.1.clone()
        }
        fn open(&self) {
            self.state.lock().unwrap().0 = true;
            self.changed.notify_all();
        }
    }
    impl ChunkGenerator for Gated {
        type Worker = ();
        fn worker(&self) {}
        fn load(&self, x: i32, z: i32) -> Option<Chunk> {
            let mut state = self.state.lock().unwrap();
            state.1.push((x, z));
            self.changed.notify_all();
            while !state.0 {
                state = self.changed.wait(state).unwrap();
            }
            Some(Chunk::empty(x, z))
        }
        fn proto(&self, _: &mut (), _: i32, _: i32) -> Proto {
            Box::new(())
        }
        fn finish(&self, _: Proto) -> Chunk {
            unreachable!("nothing is decorated")
        }
    }

    fn context() -> crate::Result<Arc<WorkerContext>> {
        use crate::{
            assets::Assets, biome::Biomes, blocks::BlockFacts, light::LightTables,
            mesher::MeshTables,
        };
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES"),
        );
        let assets = Arc::new(Assets::load(&root)?);
        let render = Arc::new(assets.reload(&root)?);
        let facts = BlockFacts::load(&root, assets.registry)?;
        let tables = Arc::new(MeshTables::new(
            &assets,
            &render,
            &facts,
            crate::models::load_visibility(&root, assets.registry)?,
        )?);
        Ok(Arc::new(WorkerContext {
            light: Arc::new(LightTables::load(&root, assets.registry, &facts)?),
            tables,
            assets,
            render,
            biomes: crate::biome::BiomeTable::new(Arc::new(Biomes::load(&root)?)),
        }))
    }

    /// A load of the area that read the save before the delete must not bring the old column
    /// back: a running one is waited for and its column dropped, a queued one never runs, and
    /// loads elsewhere go on.
    #[test]
    #[ignore = "requires prepared models/textures in HORIZON_TEST_RESOURCES"]
    fn settle_drops_the_areas_loads_and_keeps_the_others() -> crate::Result<()> {
        let generator = Arc::new(Gated::default());
        let mut streaming = Streaming::new(generator.clone(), context()?, 1);
        let (running, queued, elsewhere) = ((0, 0), (1, 0), (5, 5));
        streaming.generating.insert(running);
        streaming.submit(Some(running), 0, Job::Generate(running.0, running.1));
        assert_eq!(generator.wait_started(1), [running]);
        for key in [queued, elsewhere] {
            streaming.generating.insert(key);
            streaming.submit(Some(key), 0, Job::Generate(key.0, key.1));
        }
        let gate = generator.clone();
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            gate.open();
        });
        let area: HashSet<(i32, i32)> = [running, queued].into_iter().collect();
        assert!(settle(&mut streaming, &area));
        opener.join().unwrap();
        assert!(!streaming.generating.contains(&running));
        assert!(!streaming.generating.contains(&queued));
        assert!(streaming.generating.contains(&elsewhere));
        // Only the load outside the area runs after the one that was running.
        assert_eq!(generator.wait_started(2), [running, elsewhere]);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(generator.state.lock().unwrap().1, [running, elsewhere]);
        let keys: Vec<(i32, i32)> = area.iter().copied().collect();
        streaming.regenerate(&keys);
        assert!(!streaming.generation_scan.exhausted);
        Ok(())
    }

    /// Work `Streaming::save` waited for and did not see finish: nothing may be deleted.
    #[test]
    #[ignore = "requires prepared models/textures in HORIZON_TEST_RESOURCES"]
    fn settle_refuses_while_a_decoration_is_out() -> crate::Result<()> {
        let generator = Arc::new(Gated::default());
        generator.open();
        let mut streaming = Streaming::new(generator, context()?, 1);
        streaming.decorating = 1;
        assert!(!settle(&mut streaming, &HashSet::from([(0, 0)])));
        streaming.decorating = 0;
        assert!(settle(&mut streaming, &HashSet::from([(0, 0)])));
        Ok(())
    }
}
