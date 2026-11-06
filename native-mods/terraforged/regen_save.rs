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
// them, and loads see them at once.

use super::{Result, SaveDir};

/// The chunks' records in `region/`: what makes them generate again.
pub fn delete_chunks(save: &SaveDir, keys: &[(i32, i32)]) -> Result<()> {
    save.write_records(
        "region",
        keys.iter().map(|key| (*key, None)).collect(),
        false,
    )
}

/// Their entities and POIs too, which Java keeps (see `/tf regen` in commands.rs).
pub fn delete_chunk_data(save: &SaveDir, keys: &[(i32, i32)]) -> Result<()> {
    save.write_records("poi", keys.iter().map(|key| (*key, None)).collect(), false)?;
    save.save_entities_later(keys.iter().map(|key| (*key, Vec::new())).collect())
}

#[cfg(test)]
mod tests {
    use super::super::{LevelShape, NamedTag, SaveDir, Tag};
    use super::{delete_chunk_data, delete_chunks};

    #[test]
    fn regen_deletes_are_seen_at_once_win_over_earlier_writes_and_last() {
        let dir = std::env::temp_dir().join(format!("tf-regen-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
        delete_chunk_data(&save, &keys).unwrap();
        for (x, z) in keys {
            assert!(save.read_record("region", x, z).is_none());
        }
        save.flush().unwrap();
        let reopened = SaveDir::new(dir.clone(), LevelShape::OVERWORLD);
        for (x, z) in keys {
            assert!(reopened.read_record("region", x, z).is_none());
            assert!(reopened.read_record("poi", x, z).is_none());
            assert!(reopened.load_entities(x, z).is_empty());
        }
        assert!(reopened.read_record("region", 1, 0).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
