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

// `/tf regen`'s storage half: `chunkMap.write(pos, null)` for each chunk. The deletes go
// through the region writer's queue like every other write, after the writes queued before
// them, and loads see them at once. Java never touches `entities/` or `poi/`: the area's
// entities stay saved and come back with the chunks generated in its place (Horizon's
// `Persistent::finish` loads a generated chunk's saved entities), and its POIs are
// revalidated when the new chunks save (`resolve_poi` keeps only the POIs their blocks hold).

use super::{Result, SaveDir, Tag};

/// The chunks' records in `region/`: what makes them generate again.
pub fn delete_chunks(save: &SaveDir, keys: &[(i32, i32)]) -> Result<()> {
    save.write_records(
        "region",
        keys.iter().map(|key| (*key, None)).collect(),
        false,
    )
}

/// The area's entities, taken out of memory (`Game::entity_chunks`, one list per key), saved
/// to `entities/` as their chunks unloading would: a resident chunk's list is all it holds;
/// a chunk that is not resident keeps what it has saved, after it whatever stood in it.
pub fn keep_entities(
    save: &SaveDir,
    held: Vec<((i32, i32), Vec<Tag>)>,
    resident: impl Fn((i32, i32)) -> bool,
) -> Result<()> {
    let records = held
        .into_iter()
        .filter_map(|(key, mut entities)| {
            if resident(key) {
                return Some((key, entities));
            }
            if entities.is_empty() {
                return None;
            }
            let mut saved = save.load_entities(key.0, key.1);
            saved.append(&mut entities);
            Some((key, saved))
        })
        .collect();
    save.save_entities_later(records)
}

#[cfg(test)]
mod tests {
    use super::super::{LevelShape, NamedTag, SaveDir, Tag};
    use super::{delete_chunks, keep_entities};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tf-regen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn regen_deletes_are_seen_at_once_win_over_earlier_writes_and_last() {
        let dir = scratch("save");
        let save = SaveDir::new(dir.clone(), LevelShape::OVERWORLD);
        let record = |version| NamedTag {
            name: "".into(),
            tag: Tag::compound([("version", Tag::Int(version))]),
        };
        let entity = Tag::compound([("id", Tag::String("minecraft:cow".into()))]);
        let keys = [(0, 0), (-1, 31), (32, -33)];
        for key in keys {
            save.write_records("region", vec![(key, Some(record(1)))], false)
                .unwrap();
            save.write_records("poi", vec![(key, Some(record(1)))], false)
                .unwrap();
        }
        save.save_entities_later(keys.iter().map(|k| (*k, vec![entity.clone()])).collect())
            .unwrap();
        save.flush().unwrap();
        // A chunk outside the area, and a write queued just before the delete.
        save.write_records("region", vec![((1, 0), Some(record(1)))], false)
            .unwrap();
        save.write_records("region", vec![((0, 0), Some(record(2)))], false)
            .unwrap();
        delete_chunks(&save, &keys).unwrap();
        for (x, z) in keys {
            assert!(save.read_record("region", x, z).is_none());
        }
        save.flush().unwrap();
        // Only `region/` loses the area, as `chunkMap.write(pos, null)`: entities and POIs stay.
        let reopened = SaveDir::new(dir.clone(), LevelShape::OVERWORLD);
        for (x, z) in keys {
            assert!(reopened.read_record("region", x, z).is_none());
            assert!(reopened.read_record("poi", x, z).is_some());
            assert_eq!(reopened.load_entities(x, z), vec![entity.clone()]);
        }
        assert!(reopened.read_record("region", 1, 0).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kept_entities_replace_a_resident_chunks_and_join_an_unloaded_ones() {
        let dir = scratch("entities");
        let save = SaveDir::new(dir.clone(), LevelShape::OVERWORLD);
        let mob = |id: &str| Tag::compound([("id", Tag::String(id.into()))]);
        let keys = [(0, 0), (1, 0), (2, 0), (3, 0)];
        save.save_entities(
            keys.iter()
                .map(|k| (*k, vec![mob("minecraft:cow")]))
                .collect(),
        )
        .unwrap();
        // (0, 0) and (1, 0) are resident: memory is the truth, even when empty. (2, 0) and
        // (3, 0) are not: what they saved stays, a sheep that walked into (3, 0) joins it.
        let held = vec![
            ((0, 0), vec![mob("minecraft:pig")]),
            ((1, 0), Vec::new()),
            ((2, 0), Vec::new()),
            ((3, 0), vec![mob("minecraft:sheep")]),
        ];
        keep_entities(&save, held, |key| key.0 < 2).unwrap();
        save.flush().unwrap();
        let reopened = SaveDir::new(dir.clone(), LevelShape::OVERWORLD);
        assert_eq!(reopened.load_entities(0, 0), vec![mob("minecraft:pig")]);
        assert!(reopened.load_entities(1, 0).is_empty());
        assert_eq!(reopened.load_entities(2, 0), vec![mob("minecraft:cow")]);
        assert_eq!(
            reopened.load_entities(3, 0),
            vec![mob("minecraft:cow"), mob("minecraft:sheep")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole storage side of `/tf regen` on one chunk: an armor stand saved in it comes
    /// back with the chunk generated in its place, same UUID and position, and the bed POI
    /// the old chunk had is dropped once the new one, without the bed, is saved.
    #[test]
    fn a_regenerated_chunk_keeps_its_entities_and_drops_stale_pois() {
        use super::super::{Biomes, Chunk, Persistent, SaveTables, text};
        use crate::streaming::ChunkGenerator;
        let Some(root) = std::env::var_os("HORIZON_TEST_RESOURCES") else {
            return;
        };
        let root = PathBuf::from(root);
        let assets = Arc::new(crate::assets::Assets::load(&root).expect("assets"));
        let biomes = Arc::new(Biomes::load(&root).expect("biomes"));
        let dir = scratch("chunk");
        let save = Arc::new(SaveDir::new(dir.clone(), LevelShape::OVERWORLD));
        let _ = save.tables.set(Arc::new(SaveTables::new(&assets)));
        let registry = assets.registry;
        let bed = {
            let mut s = registry
                .state(registry.default_state("minecraft:red_bed").unwrap())
                .unwrap()
                .clone();
            s.properties.insert("part".into(), "head".into());
            registry.id(&s).unwrap().0
        };
        let stand = Tag::compound([
            ("id", Tag::String("minecraft:armor_stand".into())),
            ("UUID", Tag::IntArray(vec![7, -8, 9, -10])),
            (
                "Pos",
                Tag::List(
                    [52.5f64, 70.0, -27.5]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            ),
        ]);
        let key = (3, -2);
        let mut old = Chunk::empty(key.0, key.1);
        old.set(4, 70, 4, bed);
        save.save_later(&[Arc::new(old)], registry, &biomes)
            .unwrap();
        save.save_entities_later(vec![(key, vec![stand.clone()])])
            .unwrap();
        save.flush().unwrap();
        let homes = |save: &SaveDir| {
            let record = save.read_record("poi", key.0, key.1).expect("poi record");
            let Some(Tag::Compound(sections)) = record.tag.get("Sections") else {
                return 0;
            };
            sections
                .values()
                .filter_map(|section| match section.get("Records") {
                    Some(Tag::List(records)) => Some(records),
                    _ => None,
                })
                .flatten()
                .filter(|r| text(r.get("type")).as_deref() == Some("minecraft:home"))
                .count()
        };
        assert_eq!(homes(&save), 1);
        delete_chunks(&save, &[key]).unwrap();
        let generator = Persistent {
            inner: Arc::new(crate::testgen::TestTerrain::new(registry, &biomes).unwrap()),
            save: save.clone(),
            registry: assets.clone(),
            biomes: biomes.clone(),
        };
        assert!(
            generator.load(key.0, key.1).is_none(),
            "the chunk generates again"
        );
        // TestTerrain's worker is `()`.
        let chunk = generator.finish(generator.proto(&mut (), key.0, key.1));
        assert_eq!(chunk.entities, vec![stand]);
        save.save_later(&[Arc::new(chunk)], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
        assert_eq!(homes(&SaveDir::new(dir.clone(), LevelShape::OVERWORLD)), 0);
        drop(generator);
        drop(save);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
