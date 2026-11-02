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

// TerraForged's bundled `worldgen/cave` resources (`NoiseCave.CODEC`) and the cave
// array `NoiseCaveGenerator.createArray` builds from them.

use super::terraforged_cave_noise::{CaveType, NoiseCave};
use super::terraforged_terrain_asset::decode_module;
use serde_json::{Map, Value};

/// `NoiseCaveGenerator.GLOBAL_CAVE_REPS`: copies of each global cave.
pub const GLOBAL_CAVE_REPS: usize = 2;
/// Offset between the reseeded copies of one cave.
const REP_SEED: i64 = 0xFA90C2;

/// The bundled caves in `ModCaves`' registration order, which is the registry's order.
pub fn current_cave_documents() -> [(&'static str, &'static str); 5] {
    [
        (
            "terraforged:synapse_high",
            text(crate::terraforged_data!("worldgen/cave/synapse_high.json")),
        ),
        (
            "terraforged:synapse_mid",
            text(crate::terraforged_data!("worldgen/cave/synapse_mid.json")),
        ),
        (
            "terraforged:synapse_low",
            text(crate::terraforged_data!("worldgen/cave/synapse_low.json")),
        ),
        (
            "terraforged:mega",
            text(crate::terraforged_data!("worldgen/cave/mega.json")),
        ),
        (
            "terraforged:mega_deep",
            text(crate::terraforged_data!("worldgen/cave/mega_deep.json")),
        ),
    ]
}

/// The caves `NoiseCaveGenerator` carves, in carving order.
pub fn current_caves() -> Result<Vec<NoiseCave>, String> {
    let mut caves = Vec::new();
    for (key, json) in current_cave_documents() {
        caves.push(decode_noise_cave(json).map_err(|error| format!("cave {key:?}: {error}"))?);
    }
    Ok(cave_array(&caves))
}

/// `NoiseCaveGenerator.createArray`: global caves twice, each copy reseeded.
pub fn cave_array(caves: &[NoiseCave]) -> Vec<NoiseCave> {
    let mut array = Vec::new();
    for cave in caves {
        let count = match cave.cave_type() {
            CaveType::Global => GLOBAL_CAVE_REPS,
            CaveType::Unique => 1,
        };
        for copy in 0..count {
            array.push(cave.with_seed(copy as i64 * REP_SEED));
        }
    }
    array
}

/// `NoiseCave.CODEC`: `seed` defaults to 0 and `min_y` to -32.
pub fn decode_noise_cave(json: &str) -> Result<NoiseCave, String> {
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("expected an object")?;
    let module = |name: &str| {
        decode_module(field(object, name)?).map_err(|error| format!("{name}: {error}"))
    };
    let kind = field(object, "type")?
        .as_str()
        .ok_or("type must be a string")?;
    let cave_type =
        CaveType::for_name(kind).ok_or_else(|| format!("unknown cave type {kind:?}"))?;
    Ok(NoiseCave::new(
        optional_int(object, "seed", 0)?,
        cave_type,
        module("elevation")?,
        module("shape")?,
        module("floor")?,
        int(field(object, "size")?, "size")?,
        optional_int(object, "min_y", -32)?,
        int(field(object, "max_y")?, "max_y")?,
    ))
}

fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, String> {
    object
        .get(name)
        .ok_or_else(|| format!("missing field {name:?}"))
}

fn optional_int(object: &Map<String, Value>, name: &str, default: i32) -> Result<i32, String> {
    object
        .get(name)
        .map_or(Ok(default), |value| int(value, name))
}

fn int(value: &Value, name: &str) -> Result<i32, String> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("{name} must be an int"))
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged cave JSON is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: [i32; 3] = [0, 1337, -987_654_321];
    const POINTS: [(i32, i32); 5] = [
        (0, 0),
        (37, -91),
        (-1024, 4096),
        (12345, -6789),
        (-30001, 15003),
    ];

    // From tools/terraforged/TerraForgedNoiseCaveOracle.java: array index, seed, x, z,
    // height, cavern size at modifier 0.6, and floor depth for that cavern.
    const CAVES: [(usize, i32, i32, i32, i32, i32, i32); 120] = [
        (6, 0, 0, 0, 24, 18, 18),
        (6, 0, 37, -91, 32, 18, 18),
        (6, 0, -1024, 4096, 20, 0, 0),
        (6, 0, 12345, -6789, 10, 4, 4),
        (6, 0, -30001, 15003, 14, 3, 1),
        (6, 1337, 0, 0, 24, 18, 18),
        (6, 1337, 37, -91, 14, 18, 18),
        (6, 1337, -1024, 4096, 20, 10, 10),
        (6, 1337, 12345, -6789, 19, 18, 18),
        (6, 1337, -30001, 15003, 25, 18, 18),
        (6, -987654321, 0, 0, 24, 18, 18),
        (6, -987654321, 37, -91, 35, 18, 18),
        (6, -987654321, -1024, 4096, 28, 15, 15),
        (6, -987654321, 12345, -6789, 29, 0, 0),
        (6, -987654321, -30001, 15003, 28, 0, 0),
        (7, 0, 0, 0, 8, 21, 21),
        (7, 0, 37, -91, 13, 21, 21),
        (7, 0, -1024, 4096, 1, 3, 2),
        (7, 0, 12345, -6789, 7, 21, 14),
        (7, 0, -30001, 15003, 11, 21, 21),
        (7, 1337, 0, 0, 8, 21, 21),
        (7, 1337, 37, -91, 6, 21, 21),
        (7, 1337, -1024, 4096, 6, 12, 12),
        (7, 1337, 12345, -6789, 7, 21, 21),
        (7, 1337, -30001, 15003, 8, 21, 21),
        (7, -987654321, 0, 0, 8, 21, 21),
        (7, -987654321, 37, -91, 2, 21, 21),
        (7, -987654321, -1024, 4096, 5, 19, 19),
        (7, -987654321, 12345, -6789, 18, 4, 4),
        (7, -987654321, -30001, 15003, 10, 21, 19),
        (0, 0, 0, 0, 239, 6, 6),
        (0, 0, 37, -91, 278, 0, 0),
        (0, 0, -1024, 4096, 254, 0, 0),
        (0, 0, 12345, -6789, 263, 0, 0),
        (0, 0, -30001, 15003, 259, 0, 0),
        (0, 1337, 0, 0, 239, 6, 6),
        (0, 1337, 37, -91, 299, 0, 0),
        (0, 1337, -1024, 4096, 273, 0, 0),
        (0, 1337, 12345, -6789, 279, 0, 0),
        (0, 1337, -30001, 15003, 254, 0, 0),
        (0, -987654321, 0, 0, 239, 6, 6),
        (0, -987654321, 37, -91, 160, 0, 0),
        (0, -987654321, -1024, 4096, 199, 0, 0),
        (0, -987654321, 12345, -6789, 189, 2, 2),
        (0, -987654321, -30001, 15003, 235, 0, 0),
        (1, 0, 0, 0, 239, 6, 6),
        (1, 0, 37, -91, 270, 0, 0),
        (1, 0, -1024, 4096, 327, 0, 0),
        (1, 0, 12345, -6789, 259, 0, 0),
        (1, 0, -30001, 15003, 265, 0, 0),
        (1, 1337, 0, 0, 239, 6, 6),
        (1, 1337, 37, -91, 215, 0, 0),
        (1, 1337, -1024, 4096, 204, 0, 0),
        (1, 1337, 12345, -6789, 246, 0, 0),
        (1, 1337, -30001, 15003, 203, 0, 0),
        (1, -987654321, 0, 0, 239, 6, 6),
        (1, -987654321, 37, -91, 319, 1, 1),
        (1, -987654321, -1024, 4096, 172, 0, 0),
        (1, -987654321, 12345, -6789, 189, 0, 0),
        (1, -987654321, -30001, 15003, 209, 1, 1),
        (2, 0, 0, 0, 127, 9, 9),
        (2, 0, 37, -91, 134, 4, 4),
        (2, 0, -1024, 4096, 138, 2, 2),
        (2, 0, 12345, -6789, 143, 0, 0),
        (2, 0, -30001, 15003, 108, 2, 2),
        (2, 1337, 0, 0, 127, 9, 9),
        (2, 1337, 37, -91, 75, 0, 0),
        (2, 1337, -1024, 4096, 94, 1, 1),
        (2, 1337, 12345, -6789, 113, 0, 0),
        (2, 1337, -30001, 15003, 129, 2, 2),
        (2, -987654321, 0, 0, 127, 9, 9),
        (2, -987654321, 37, -91, 134, 0, 0),
        (2, -987654321, -1024, 4096, 163, 0, 0),
        (2, -987654321, 12345, -6789, 138, 0, 0),
        (2, -987654321, -30001, 15003, 105, 2, 2),
        (3, 0, 0, 0, 127, 9, 9),
        (3, 0, 37, -91, 181, 0, 0),
        (3, 0, -1024, 4096, 152, 0, 0),
        (3, 0, 12345, -6789, 78, 0, 0),
        (3, 0, -30001, 15003, 101, 0, 0),
        (3, 1337, 0, 0, 127, 9, 9),
        (3, 1337, 37, -91, 165, 0, 0),
        (3, 1337, -1024, 4096, 101, 0, 0),
        (3, 1337, 12345, -6789, 170, 0, 0),
        (3, 1337, -30001, 15003, 115, 0, 0),
        (3, -987654321, 0, 0, 127, 9, 9),
        (3, -987654321, 37, -91, 95, 4, 4),
        (3, -987654321, -1024, 4096, 146, 2, 2),
        (3, -987654321, 12345, -6789, 103, 0, 0),
        (3, -987654321, -30001, 15003, 72, 0, 0),
        (4, 0, 0, 0, 47, 10, 10),
        (4, 0, 37, -91, 14, 0, 0),
        (4, 0, -1024, 4096, 84, 3, 2),
        (4, 0, 12345, -6789, 60, 0, 0),
        (4, 0, -30001, 15003, 58, 2, 2),
        (4, 1337, 0, 0, 47, 10, 10),
        (4, 1337, 37, -91, 62, 0, 0),
        (4, 1337, -1024, 4096, 81, 0, 0),
        (4, 1337, 12345, -6789, 52, 0, 0),
        (4, 1337, -30001, 15003, 40, 3, 3),
        (4, -987654321, 0, 0, 47, 10, 10),
        (4, -987654321, 37, -91, 74, 2, 2),
        (4, -987654321, -1024, 4096, 27, 1, 1),
        (4, -987654321, 12345, -6789, 44, 0, 0),
        (4, -987654321, -30001, 15003, 39, 3, 3),
        (5, 0, 0, 0, 47, 10, 10),
        (5, 0, 37, -91, 77, 0, 0),
        (5, 0, -1024, 4096, 29, 0, 0),
        (5, 0, 12345, -6789, 64, 0, 0),
        (5, 0, -30001, 15003, 40, 0, 0),
        (5, 1337, 0, 0, 47, 10, 10),
        (5, 1337, 37, -91, 58, 0, 0),
        (5, 1337, -1024, 4096, 88, 0, 0),
        (5, 1337, 12345, -6789, 55, 0, 0),
        (5, 1337, -30001, 15003, 47, 0, 0),
        (5, -987654321, 0, 0, 47, 10, 10),
        (5, -987654321, 37, -91, 90, 0, 0),
        (5, -987654321, -1024, 4096, 31, 0, 0),
        (5, -987654321, 12345, -6789, 51, 0, 0),
        (5, -987654321, -30001, 15003, 57, 2, 2),
    ];

    #[test]
    fn bundled_caves_match_terraforged_source() {
        let caves = current_caves().unwrap();
        assert_eq!(caves.len(), 8);
        assert_eq!(CAVES.len(), caves.len() * SEEDS.len() * POINTS.len());
        for (index, seed, x, z, height, cavern, floor) in CAVES {
            let cave = &caves[index];
            assert_eq!(
                cave.height(seed, x, z),
                Ok(height),
                "{index} {seed} {x} {z}"
            );
            assert_eq!(
                cave.cavern_size(seed, x, z, 0.6),
                Ok(cavern),
                "{index} {seed} {x} {z}"
            );
            assert_eq!(
                cave.floor_depth(seed, x, z, cavern),
                Ok(floor),
                "{index} {seed} {x} {z}"
            );
        }
    }

    #[test]
    fn cave_defaults_and_errors() {
        let cave = decode_noise_cave(
            r#"{"type":"unique","size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},
            "shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}"#,
        )
        .unwrap();
        assert_eq!(cave.seed(), 0);
        assert_eq!(cave.height(0, 0, 0), Ok(-11));
        assert!(decode_noise_cave(r#"{"type":"sideways"}"#).is_err());
    }
}
