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

// TerraForged's bundled `worldgen/cave` resources as `NoiseCave.CODEC` reads them, and the cave array
// `NoiseCaveGenerator.createArray` builds from them (TerraForged 6dd607eb; Reimagined a24d450 has the same codec and
// `createArray`). The five data files are TerraForged Reimagined a24d450's
// (https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11,
// `src/main/resources/default/data/terraforged/worldgen/cave/`, MIT, (c) 2020 TerraForged, (c) 2026 Romoslayer; see
// `LICENSE-TERRAFORGED-REIMAGINED`), bundled unchanged in `data/worldgen/cave/`; 6dd607eb's generated copies are
// byte-identical. The noise graphs go through `terrain_asset.rs`'s `NoiseCodec` decoder.

use super::terraforged_cave_noise::{CaveType, NoiseCave};
use super::terraforged_terrain_asset::decode_noise;
use serde_json::value::RawValue;
use std::collections::HashMap;

/// `NoiseCaveGenerator.GLOBAL_CAVE_REPS`: copies of each global cave.
pub const GLOBAL_CAVE_REPS: usize = 2;
/// `createArray`'s offset between the reseeded copies of one cave: copy `j` is reseeded with `j * 0xFA90C2L`, a
/// `long` (`Module::with_seed` keeps its low 32 bits, as Cereal's `asInt` does).
const REP_SEED: i64 = 0xFA_90C2;

/// The bundled caves as (registry key, JSON) pairs, in the registry's order. A 26.3 data-pack registry registers
/// its entries by `Identifier.compareTo` (path, then namespace: see `identifier_order`), which is the order
/// `NoiseCaveGenerator` iterates; `ModCaves`' registration order (synapse_high, synapse_mid, synapse_low, mega,
/// mega_deep) only applies to TerraForged's data generation.
pub fn current_cave_documents() -> [(&'static str, &'static str); 5] {
    [
        (
            "terraforged:mega",
            text(crate::terraforged_data!("worldgen/cave/mega.json")),
        ),
        (
            "terraforged:mega_deep",
            text(crate::terraforged_data!("worldgen/cave/mega_deep.json")),
        ),
        (
            "terraforged:synapse_high",
            text(crate::terraforged_data!("worldgen/cave/synapse_high.json")),
        ),
        (
            "terraforged:synapse_low",
            text(crate::terraforged_data!("worldgen/cave/synapse_low.json")),
        ),
        (
            "terraforged:synapse_mid",
            text(crate::terraforged_data!("worldgen/cave/synapse_mid.json")),
        ),
    ]
}

/// The caves `NoiseCaveGenerator` carves, in carving order: the bundled caves decoded in registry order, then
/// [`cave_array`]. An `Err` names the cave that failed to decode. Each call decodes and reseeds the graphs again
/// (fresh caches), so build the array once per world, as `NoiseCaveGenerator`'s constructor does, and share it
/// (`NoiseCave` is `Send + Sync`).
pub fn current_caves() -> Result<Vec<NoiseCave>, String> {
    let mut caves = Vec::new();
    for (key, json) in current_cave_documents() {
        caves.push(decode_noise_cave(json).map_err(|error| format!("cave {key:?}: {error}"))?);
    }
    Ok(cave_array(&caves))
}

/// `NoiseCaveGenerator.createArray`: each global cave twice and each unique cave once, in order; copy `j` is
/// `cave.withSeed(j * 0xFA90C2L)` (so even the first copy is rebuilt through Cereal, with seed 0). Panics where
/// the jar's `withSeed` throws (see `Module::with_seed`); decoded graphs never do.
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

/// `NoiseCave.CODEC` on Minecraft 26.3's DFU over the strict JSON a data-pack registry reads:
///
/// - `type` (`CaveType.CODEC`): a JSON string, read with `CaveType.forName` (any case; an unknown name is an error,
///   where Java's `valueOf` throws).
/// - `seed` and `min_y` (optional, default 0 and -32), `size` and `max_y` (`Codec.INT`): a JSON number, read with
///   Gson's `LazilyParsedNumber.intValue` (`int`, else `(int)` of a `long`, else `BigDecimal.intValue`: truncated
///   toward zero, low 32 bits). Strings, booleans, objects and arrays are errors; `null` counts as absent (so a
///   `null` optional field takes its default). Unlike 1.19's DFU, which 6dd607eb ran on, an invalid optional
///   field is an error, not its default.
/// - `elevation`, `shape` and `floor` (`NoiseCodec.CODEC`): see [`decode_noise`].
///
/// Other fields are ignored; a duplicate key keeps its last value, as Gson's `JsonObject` does.
pub fn decode_noise_cave(json: &str) -> Result<NoiseCave, String> {
    let object: HashMap<String, &RawValue> =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    let field = |name: &str| {
        object
            .get(name)
            .map(|raw| raw.get())
            .filter(|text| *text != "null")
    };
    let required = |name: &str| field(name).ok_or_else(|| format!("missing field {name:?}"));
    let int = |name: &str, default: Option<i32>| match (field(name), default) {
        (None, Some(default)) => Ok(default),
        (None, None) => Err(format!("missing field {name:?}")),
        (Some(text), _) => {
            int_value(text).ok_or_else(|| format!("{name} must be a number: {text}"))
        }
    };
    let module =
        |name: &str| decode_noise(required(name)?).map_err(|error| format!("{name}: {error}"));
    let kind: String =
        serde_json::from_str(required("type")?).map_err(|_| "type must be a string".to_owned())?;
    let cave_type =
        CaveType::for_name(&kind).ok_or_else(|| format!("unknown cave type {kind:?}"))?;
    Ok(NoiseCave::new(
        int("seed", Some(0))?,
        cave_type,
        module("elevation")?,
        module("shape")?,
        module("floor")?,
        int("size", None)?,
        int("min_y", Some(-32))?,
        int("max_y", None)?,
    ))
}

/// Gson's `LazilyParsedNumber.intValue` on a JSON number's text: `Integer.parseInt`, then `(int) Long.parseLong`,
/// then `BigDecimal.intValue()` (the integer part, truncated toward zero, keeps its low 32 bits). `None` for
/// anything but a number, and past Gson's `NumberLimits` (over 10,000 characters, or a scale of 10,000 or more
/// either way), where `parseBigDecimal` throws.
fn int_value(text: &str) -> Option<i32> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if !unsigned.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if let Ok(value) = text.parse::<i64>() {
        return Some(value as i32);
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(e) => (&unsigned[..e], unsigned[e + 1..].parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let scale = (fraction.len() as i64).saturating_sub(exponent);
    if text.len() > 10_000 || scale.unsigned_abs() >= 10_000 {
        return None;
    }
    // The value is `digits * 10^-scale`: drop `scale` digits, or append `-scale` zeros, modulo 2^32.
    let digits = format!("{whole}{fraction}");
    let kept = (digits.len() as i64 - scale).max(0) as usize;
    let mut value = 0u32;
    for index in 0..kept {
        let digit = digits.as_bytes().get(index).map_or(0, |digit| digit - b'0');
        value = value.wrapping_mul(10).wrapping_add(u32::from(digit));
    }
    let value = value as i32;
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged cave JSON is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_biome_registry::identifier_order;
    use super::super::terraforged_cave_noise::tests::{RUN_SEEDS, fold_cave};
    use super::*;
    use std::fmt::Write;

    /// `.local/terraforged/oracles/cave_assets/cases.txt`: edge-case cave documents (name, tab, JSON).
    const CASES: &str = r#"# name<TAB>cave JSON (one per line). Shared graph: a Map over a seeded Simplex, a Const shape and floor.
defaults	{"type":"unique","size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},"shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}
upper_type	{"type":"GLOBAL","seed":7,"size":12,"min_y":-8,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
dotless_i	{"type":"unıque","size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},"shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}
unknown_type	{"type":"sideways","size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},"shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}
number_type	{"type":1,"size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},"shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}
missing_type	{"size":3,"max_y":10,"elevation":{"type":"Const","value":0.5},"shape":{"type":"Const","value":1},"floor":{"type":"Const","value":0}}
fraction_ints	{"type":"global","seed":3.7,"size":12.9,"min_y":-8.5,"max_y":40.99,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
exponent_ints	{"type":"global","seed":1e3,"size":1.2E1,"min_y":-8e0,"max_y":4e1,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
wide_ints	{"type":"global","seed":4294967297,"size":12,"min_y":-2147483649,"max_y":9223372036854775807,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
huge_ints	{"type":"global","seed":18446744073709551621,"size":12,"min_y":-1e30,"max_y":1e300,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
big_fraction	{"type":"global","seed":-4294967298.75,"size":12,"min_y":-8,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
exponent_limit	{"type":"global","seed":1e10000,"size":12,"min_y":-8,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
bool_ints	{"type":"global","seed":true,"size":12,"min_y":false,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
string_seed	{"type":"global","seed":"5","size":12,"min_y":-8,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
string_size	{"type":"global","size":"12","max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
null_optional	{"type":"global","seed":null,"size":12,"min_y":null,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
null_size	{"type":"global","size":null,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
object_seed	{"type":"global","seed":{},"size":12,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
missing_max_y	{"type":"global","size":12,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
missing_floor	{"type":"global","size":12,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75}}
number_floor	{"type":"global","size":12,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":0.25}
null_floor	{"type":"global","size":12,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":null}
unsupported_floor	{"type":"global","size":12,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Line"}}
duplicate_keys	{"type":"unique","size":3,"size":20,"max_y":10,"max_y":64,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
extra_fields	{"type":"global","size":12,"max_y":40,"name":"x","elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25},"extra":[1,2]}
array_top	[1,2,3]
nested_abs	{"type":"global","size":30,"max_y":40,"elevation":{"type":"Abs","source":{"type":"Abs","source":{"type":"Simplex","seed":3,"frequency":0.01}}},"shape":{"type":"Abs","source":{"type":"Bias","bias":{"type":"Const","value":-0.5},"source":{"type":"Simplex","seed":9,"frequency":0.02}}},"floor":{"type":"Abs","source":{"type":"Bias","bias":-0.5,"source":{"type":"Simplex","seed":9,"frequency":0.02}}}}
abs_no_source	{"type":"global","size":30,"max_y":40,"elevation":{"type":"Abs"},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
negative_range	{"type":"global","size":-4,"min_y":50,"max_y":40,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Const","value":0.75},"floor":{"type":"Const","value":0.25}}
wrapping_range	{"type":"global","size":2147483647,"min_y":-2147483648,"max_y":2147483647,"elevation":{"type":"Simplex","seed":3,"frequency":0.01},"shape":{"type":"Simplex","seed":4,"frequency":0.03},"floor":{"type":"Simplex","seed":5,"frequency":0.05}}
"#;

    /// `CaveAssetsOracle`'s output (`.local/terraforged/oracles/cave_assets/run.sh`): the bundled files decoded by
    /// TerraForged 6dd607eb's own `NoiseCave.CODEC` (Noise2D 2cb4843 specs, Minecraft 26.3's DFU), `createArray`
    /// over them in registry order, and 6dd607eb's seed-first samplers folded at `RUN_SEEDS`; then `CASES`. Run
    /// interpreted and JIT-compiled.
    const CAVE_ORACLE: &str = r#"f mega 5852 unique 30 -16 64
f mega_deep 247699 unique 36 -32 48
f synapse_high 101281 global 11 96 384
f synapse_low 59189 global 18 -32 128
f synapse_mid 70872 global 15 0 256
a 0 5852 unique 30 -16 64 9f79c8ecc3fccc6f 88375b054f3865a b78d0f1c3d2e4ea1 a7b7bd106866c07d b0f5e10cd7df6de7 2b77172ac4cff97d 5d867e6a13d78959 5485cd5f3c03d319
v 0 24,18,18 32,18,18 20,0,0 10,4,4 14,3,1 24,18,18 14,18,18 20,10,10 19,18,18 25,18,18
a 1 247699 unique 36 -32 48 31cf7e96de75aeff bfa7556cebec7dbf 362c9cc57e5b324c 266d32f3a3debceb ef9562f65e6c192f ada824b6eb9c97b1 1b6290133e80191f dbbfbdc3b2ea396e
v 1 8,21,21 13,21,21 1,3,2 7,21,14 11,21,21 8,21,21 6,21,21 6,12,12 7,21,21 8,21,21
a 2 101281 global 11 96 384 4205ce25aca5e7cb e9824631143d706 45d70e5bda4ee73a e07a736865266059 617b13b75f526cf5 87674e5c8c66e128 db82558ac60b0d3b 1b15289a9db503c8
v 2 239,6,6 278,0,0 254,0,0 263,0,0 259,0,0 239,6,6 299,0,0 273,0,0 279,0,0 254,0,0
a 3 101281 global 11 96 384 21ea9acf8bd78502 f5d4cfa8c32ac082 37521c028f911b70 2feb18374c220632 2c400b6cf0904c98 4ea9abd54a23e938 b9536bad13a7d74b c679779ca6548de1
v 3 239,6,6 270,0,0 327,0,0 259,0,0 265,0,0 239,6,6 215,0,0 204,0,0 246,0,0 203,0,0
a 4 59189 global 18 -32 128 4e590b49c04a1263 f864f7f261868007 c370a1edd09c4bd4 cc4ef2d03df19dad 113eb76f8f94afd7 94db852d1f8b94dc 69ca402a8ce95276 dc24228a86ec5e6
v 4 47,10,10 14,0,0 84,3,2 60,0,0 58,2,2 47,10,10 62,0,0 81,0,0 52,0,0 40,3,3
a 5 59189 global 18 -32 128 c510cab76c7d73e 690fd5429d3e290b 825fdf55e6b22851 8ae9734e102c3cb0 cbe29719c5491eac 7ac2368458b91c89 5ccf475ffb512725 902729b0b9cf5606
v 5 47,10,10 77,0,0 29,0,0 64,0,0 40,0,0 47,10,10 58,0,0 88,0,0 55,0,0 47,0,0
a 6 70872 global 15 0 256 c15223a107075a6d 46cc32f95a472945 74982374c1f6c0f2 9d98ce22a4863338 57d4f2e267e01477 cd0949112918b1ce 2623b017dce6580b cf19dd82ae487ef8
v 6 127,9,9 134,4,4 138,2,2 143,0,0 108,2,2 127,9,9 75,0,0 94,1,1 113,0,0 129,2,2
a 7 70872 global 15 0 256 a1d478090e7896f 8110f9c0a1d52300 c708f1ea31907616 286ef7e5deab5a b1a9d0d37bc46457 bfee8cec2b1fa478 e01fdc83340e105 5bdd8e27bca8dba3
v 7 127,9,9 181,0,0 152,0,0 78,0,0 101,0,0 127,9,9 165,0,0 101,0,0 170,0,0 115,0,0
case defaults ok 0 unique 3 -32 10 ba2719fa3fdd8705 ba2719fa3fdd8705
case upper_type ok 7 global 12 -8 40 cf2cbfc9e4a67a1e bb436738c36ca80a
case dotless_i ok 0 unique 3 -32 10 ba2719fa3fdd8705 ba2719fa3fdd8705
case unknown_type err
case number_type err
case missing_type err
case fraction_ints ok 3 global 12 -8 40 cf2cbfc9e4a67a1e bb436738c36ca80a
case exponent_ints ok 1000 global 12 -8 40 cf2cbfc9e4a67a1e bb436738c36ca80a
case wide_ints ok 1 global 12 2147483647 -1 e4c19e7b3e2d3ba5 e4c19e7b3e2d3ba5
case huge_ints ok 5 global 12 -1073741824 0 b6d51395fd7e53f9 a0fdc9bc1698e9e8
case big_fraction ok -2 global 12 -8 40 cf2cbfc9e4a67a1e bb436738c36ca80a
case exponent_limit err
case bool_ints err
case string_seed err
case string_size err
case null_optional ok 0 global 12 -32 40 9e180b5aaeda8be8 d1be856cc6fe035a
case null_size err
case object_seed err
case missing_max_y err
case missing_floor err
case number_floor err
case null_floor err
case unsupported_floor err
case duplicate_keys ok 0 unique 20 -32 64 d33e2b76b87057b2 379bcd3f2333e4cc
case extra_fields ok 0 global 12 -32 40 9e180b5aaeda8be8 d1be856cc6fe035a
case array_top err
case nested_abs ok 0 global 30 -32 40 118f2d8b22d6b865 8aee4196c4542069
case abs_no_source err
case negative_range ok 0 global -4 50 40 419fd092d741d8a5 419fd092d741d8a5
case wrapping_range ok 0 global 2147483647 -2147483648 2147483647 42bf317dae069250 60eb914d32b9b106
"#;

    const POINTS: [(i32, i32); 5] = [
        (0, 0),
        (37, -91),
        (-1024, 4096),
        (12345, -6789),
        (-30001, 15003),
    ];

    fn describe(cave: &NoiseCave) -> String {
        format!(
            "{} {} {} {} {}",
            cave.seed(),
            cave.cave_type().name(),
            cave.size(),
            cave.min_y(),
            cave.max_y()
        )
    }

    #[test]
    fn bundled_caves_match_terraforged_6dd607eb() {
        let mut out = String::new();
        let mut caves = Vec::new();
        for (key, json) in current_cave_documents() {
            let cave = decode_noise_cave(json).unwrap();
            let name = key.strip_prefix("terraforged:").unwrap();
            writeln!(out, "f {name} {}", describe(&cave)).unwrap();
            caves.push(cave);
        }
        let array = current_caves().unwrap();
        assert_eq!(array.len(), cave_array(&caves).len());
        for (index, cave) in array.iter().enumerate() {
            write!(out, "a {index} {}", describe(cave)).unwrap();
            for seed in RUN_SEEDS {
                write!(out, " {:x}", fold_cave(cave, seed)).unwrap();
            }
            write!(out, "\nv {index}").unwrap();
            for seed in [0, 1337] {
                for (x, z) in POINTS {
                    let height = cave.height(seed, x, z).unwrap();
                    let cavern = cave.cavern_size(seed, x, z, 0.6).unwrap();
                    let floor = cave.floor_depth(seed, x, z, cavern).unwrap();
                    write!(out, " {height},{cavern},{floor}").unwrap();
                }
            }
            writeln!(out).unwrap();
        }
        for line in CASES
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (name, json) = line.split_once('\t').unwrap();
            match decode_noise_cave(json) {
                Ok(cave) => writeln!(
                    out,
                    "case {name} ok {} {:x} {:x}",
                    describe(&cave),
                    fold_cave(&cave, 0),
                    fold_cave(&cave, 1337)
                ),
                Err(_) => writeln!(out, "case {name} err"),
            }
            .unwrap();
        }
        assert_eq!(out, CAVE_ORACLE);
    }

    #[test]
    fn bundled_caves_are_in_registry_order() {
        let documents = current_cave_documents();
        assert!(
            documents.is_sorted_by(|(left, _), (right, _)| identifier_order(left, right).is_lt())
        );
        let array = current_caves().unwrap();
        let types: Vec<_> = array.iter().map(|cave| cave.cave_type()).collect();
        use CaveType::{Global, Unique};
        assert_eq!(
            types,
            [
                Unique, Unique, Global, Global, Global, Global, Global, Global
            ]
        );
    }

    #[test]
    fn caves_can_be_shared_across_generation_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<NoiseCave>();
        shared::<Vec<NoiseCave>>();
    }

    #[test]
    fn gson_int_values() {
        for (text, value) in [
            ("7", Some(7)),
            ("-0", Some(0)),
            ("3.99", Some(3)),
            ("-3.99", Some(-3)),
            ("1e3", Some(1000)),
            ("12E-1", Some(1)),
            ("0.5e1", Some(5)),
            ("2147483648", Some(i32::MIN)),
            ("-4294967298.75", Some(-2)),
            ("9223372036854775808", Some(0)),
            ("-1e30", Some(-1_073_741_824)),
            ("1e9999", Some(0)),
            ("1e10000", None),
            ("1e-10000", None),
            ("true", None),
            ("\"5\"", None),
            ("{}", None),
        ] {
            assert_eq!(int_value(text), value, "{text}");
        }
    }
}
