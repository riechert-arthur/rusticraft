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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `GeneratorPreset.applyTerrainOverrides` and `isMountain`, the Terrain page's per-terrain weight and scale overrides.
// TerraForged 6dd607eb and the 1.18.2-0.3.1 jar have no such pass. MIT License, Copyright (c) 2020 TerraForged,
// Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged::Module, terraforged_current_settings::CurrentTerrainSettings,
    terraforged_terrain_blender::TerrainNoise,
};
use std::sync::Arc;

/// Reimagined's `GeneratorPreset.applyTerrainOverrides(terrains, ids, settings)`: `ids[i]` is `terrains[i]`'s registry
/// id as `RegistryKey.entryIds` lists it. The caller zips the two listings of one registry, so their lengths must
/// match: this panics otherwise (Java throws on a shorter id list unless it returns early, and ignores extra ids).
///
/// A mountain width that is not `> 0` (zero, negative, NaN) counts as 1. With no entries, a global vertical scale of
/// exactly 1, fancy mountains on and a width of 1, the input comes back unchanged. Otherwise, in input order: with
/// fancy mountains off, an id containing `ridge` is dropped; an id with no entry, no global scale and no width stretch
/// (width 1, or an id that is not a mountain: `isMountain`'s case-sensitive `mountain`, `dolomites`, `torridonian` or
/// `dales`) keeps the same `Arc`. Every other terrain is rebuilt through [`TerrainNoise::new`] (so a noise whose new
/// minimum is below `MIN_NOISE` is raised again, as Java does) with, in Java's order: the entry's weight; its
/// horizontal scale as `freq(1 / max(0.01, h))` (only for `h != 1 && h > 0`, in `double` math as Java); its vertical
/// scale (`!= 1`); its base scale (`!= 1`) as `bias((b - 1) * 0.1).clamp(0, 1)`; the mountain stretch
/// `freq(1 / width)`; the global vertical scale. If every terrain was dropped, the input comes back unchanged.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // Java's `!(stretch > 0F)`: NaN counts as 1 too.
pub fn apply_terrain_overrides(
    terrains: &[Arc<TerrainNoise>],
    ids: &[String],
    settings: &CurrentTerrainSettings,
) -> Vec<Arc<TerrainNoise>> {
    assert_eq!(terrains.len(), ids.len(), "one registry id per terrain");
    let general = settings.general;
    let global_scale = general.global_vertical_scale != 1.0;
    let mut stretch = general.mountain_width;
    if !(stretch > 0.0) {
        stretch = 1.0;
    }
    if settings.terrains.is_empty() && !global_scale && general.fancy_mountains && stretch == 1.0 {
        return terrains.to_vec();
    }

    let mut result = Vec::with_capacity(terrains.len());
    for (terrain, id) in terrains.iter().zip(ids) {
        if !general.fancy_mountains && id.contains("ridge") {
            continue;
        }
        let entry = settings.terrains.get(id);
        let stretched = stretch != 1.0 && is_mountain(id);
        if entry.is_none() && !global_scale && !stretched {
            result.push(Arc::clone(terrain));
            continue;
        }

        let mut noise = terrain.noise().clone();
        let mut weight = terrain.weight();
        if let Some(entry) = entry {
            weight = entry.weight;
            if entry.horizontal_scale != 1.0 && entry.horizontal_scale > 0.0 {
                // `1.0 / Math.max(0.01, horizontalScale)` in double; `freq(double, double)` narrows it to a float
                // constant. The scale is > 0 here, so `max` sees no NaN or signed zero.
                let frequency = frequency(1.0 / 0.01_f64.max(f64::from(entry.horizontal_scale)));
                noise = noise.frequency(frequency.clone(), frequency);
            }
            if entry.vertical_scale != 1.0 {
                noise = noise.scale(Module::constant(entry.vertical_scale));
            }
            if entry.base_scale != 1.0 {
                noise = noise
                    .bias(Module::constant((entry.base_scale - 1.0) * 0.1))
                    .clamp(Module::constant(0.0), Module::constant(1.0));
            }
        }
        if stretched {
            let frequency = frequency(1.0 / f64::from(stretch));
            noise = noise.frequency(frequency.clone(), frequency);
        }
        if global_scale {
            noise = noise.scale(Module::constant(general.global_vertical_scale));
        }
        result.push(Arc::new(TerrainNoise::new(
            terrain.terrain().clone(),
            weight,
            noise,
        )));
    }
    if result.is_empty() {
        terrains.to_vec()
    } else {
        result
    }
}

/// `Source.constant(double)` as `Module.freq(double, double)` builds it: `(float) value`.
fn frequency(value: f64) -> Module {
    Module::constant(value as f32)
}

/// `GeneratorPreset.isMountain(id)` (`String.contains`, case-sensitive).
fn is_mountain(id: &str) -> bool {
    id.contains("mountain")
        || id.contains("dolomites")
        || id.contains("torridonian")
        || id.contains("dales")
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::{Interpolation, Source, SourceConfig, SourceKind},
        terraforged_engine::{TerrainFormSettings, TerrainRegistry},
    };
    use super::*;

    /// `.local/terraforged/oracles/terrain_overrides/run.sh`: Reimagined a24d450's `applyTerrainOverrides` and
    /// `isMountain` (copied verbatim) on the real `TerrainNoise` over Noise2D 2cb4843, one line per settings case:
    /// the number of terrains returned, then per terrain the input index if it is the same instance (else `new`), its
    /// terrain, weight, noise min and max (float bits) and an FNV-1a fold of seed-first samples (canonical NaN).
    const ORACLE: &str = "\
default 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 88f546f78cee65c0 | new mountains 3f400000 00000000 3f800000 87a5edaa50ad09cd | new mountains 3fc00000 00000000 3f800000 db7d64b7a44d8be5 | new mountains 40000000 00000000 3f800000 20e30ad61e3b83b4 | new hills 3fc00000 00000000 3f800000 a35b3330372b8cbb | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 f469eed892e15fde | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
neutral 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
width0 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
widthNaN 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
widthNeg 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
widthNegZero 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
width2 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 670ef89d2984bc3b | new mountains 3f400000 00000000 3f800000 e72846ec3bd9e6e8 | new mountains 3fc00000 00000000 3f800000 39be5e7599346919 | new mountains 40000000 00000000 3f800000 6a359351a350e1e0 | new hills 3fc00000 00000000 3f800000 9324a52549c39a5f | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 7ff003e415b1b031 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
width6 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 153a6b112e727837 | new mountains 3f400000 00000000 3f800000 f8d018836f4ac42b | new mountains 3fc00000 00000000 3f800000 a207f08ace176975 | new mountains 40000000 00000000 3f800000 46678f302ae07936 | new hills 3fc00000 00000000 3f800000 b7ee9cdaf56b77b4 | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 f24a98c55c4effce | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
width0.3 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 f9cfc02165e344d8 | new mountains 3f400000 00000000 3f800000 44540e41c39e27c6 | new mountains 3fc00000 00000000 3f800000 53d363a3bc3b4159 | new mountains 40000000 00000000 3f800000 b30c3d41665b6ea7 | new hills 3fc00000 00000000 3f800000 84fae03d5f955dbe | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 7bd233219f819fd7 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
widthMin 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 c9785580884c9475 | new mountains 3f400000 00000000 3f800000 4694109d944b0935 | new mountains 3fc00000 00000000 3f800000 4694109d944b0935 | new mountains 40000000 00000000 3f800000 4694109d944b0935 | new hills 3fc00000 00000000 3f800000 4694109d944b0935 | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 7c89e18a33e71df5 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
widthInf 11 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 cd8edd1d44fd5285 | new mountains 3f400000 00000000 3f800000 27fd5374654b0935 | new mountains 3fc00000 00000000 3f800000 cd8edd1d44fd5285 | new mountains 40000000 00000000 3f800000 d005545e43f636e9 | new hills 3fc00000 00000000 3f800000 594bad8dc12b3b44 | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 5bad915aa096b905 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
noFancy 9 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | 2 mountains 40200000 00000000 3f800000 a6cc84359cac3129 | 4 mountains 3fc00000 00000000 3f800000 e9471c363bbb4229 | 5 mountains 40000000 00000000 3f800000 56d0ecb448af3167 | 6 hills 3fc00000 00000000 3f800000 915352c5074b8a5d | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | 8 mountain_chain 3e99999a 00000000 3f800000 62424e9290d512c8 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9
noFancyDefault 9 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 88f546f78cee65c0 | new mountains 3fc00000 00000000 3f800000 db7d64b7a44d8be5 | new mountains 40000000 00000000 3f800000 20e30ad61e3b83b4 | new hills 3fc00000 00000000 3f800000 a35b3330372b8cbb | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 f469eed892e15fde | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9
noFancyOnlyRidges 2 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
noFancyOnlyRidgesNeutral 2 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | 10 plateau 3f800000 3f19999a 3f19999a aa996aa29e35bc85
empty 0
global 11 | new flats 3f800000 3e666667 3e666667 ef3af711e7c8d071 | new hills 40000000 00000000 3f800000 e427cca807258ba9 | new mountains 40200000 00000000 3f800000 8034c033089ef516 | new mountains 3f400000 00000000 3f800000 cb7f644c6b8b8ba2 | new mountains 3fc00000 00000000 3f800000 7f7f2f38e2d81791 | new mountains 40000000 00000000 3f800000 bc3b07b48f89c5cc | new hills 3fc00000 00000000 3f800000 43a388054ab4814b | new flats 3f800000 00000000 3f800000 e083c977c72dd657 | new mountain_chain 3e99999a 00000000 3f800000 8096ff1b40d36023 | new mountains 3fa00000 00000000 3f800000 c84ea81a04cf7e85 | new plateau 3f800000 3ee66667 3ee66667 060cfe66e4c8d071
globalNaN 11 | new flats 3f800000 7fc00000 7fc00000 4694109d944b0935 | new hills 40000000 7fc00000 7fc00000 4694109d944b0935 | new mountains 40200000 7fc00000 7fc00000 4694109d944b0935 | new mountains 3f400000 7fc00000 7fc00000 4694109d944b0935 | new mountains 3fc00000 7fc00000 7fc00000 4694109d944b0935 | new mountains 40000000 7fc00000 7fc00000 4694109d944b0935 | new hills 3fc00000 7fc00000 7fc00000 4694109d944b0935 | new flats 3f800000 7fc00000 7fc00000 4694109d944b0935 | new mountain_chain 3e99999a 7fc00000 7fc00000 4694109d944b0935 | new mountains 3fa00000 7fc00000 7fc00000 4694109d944b0935 | new plateau 3f800000 7fc00000 7fc00000 4694109d944b0935
global2Width3 11 | new flats 3f800000 3f19999a 3f19999a aa996aa29e35bc85 | new hills 40000000 00000000 3f800000 0ca6cdeb97476b25 | new mountains 40200000 00000000 3f800000 d365350cdac9a646 | new mountains 3f400000 00000000 3f800000 efb7ab87d70415f9 | new mountains 3fc00000 00000000 3f800000 27fd5374654b0935 | new mountains 40000000 00000000 3f800000 aaaa1885f2a90e02 | new hills 3fc00000 00000000 3f800000 8b2acd83af76727a | new flats 3f800000 00000000 3f800000 5c589c756d307ee8 | new mountain_chain 3e99999a 00000000 3f800000 12d5af245f9c7661 | new mountains 3fa00000 00000000 3f800000 37e8f1632452b5d5 | new plateau 3f800000 3f99999a 3f99999a 5855c07a5c35bc85
entries 11 | new flats 00000000 3e99999a 3e99999a af86cb2d4435bc85 | new hills 40400000 00000000 3f800000 acd86c3b56a410a1 | new mountains 40200000 00000000 3f800000 68e725c5f17425f0 | 3 mountains 3f400000 00000000 3f800000 1164d455ce566680 | new mountains 3fc00000 00000000 3f800000 a6eb79d3093bd299 | new mountains 40000000 00000000 3f800000 40deae2c8899f20b | new hills 3fc00000 00000000 3f800000 b62d516e68f636e9 | new flats 7fc00000 00000000 3f800000 4694109d944b0935 | new mountain_chain 3e99999a 00000000 3f800000 27fd5374654b0935 | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9 | new plateau 3f800000 00000000 3f800000 27fd5374654b0935
entriesAll 11 | new flats 3f800000 3ec00000 3ec00000 46eda8f0c84b0935 | new hills 3f800000 00000000 3f800000 36116ba24de98925 | new mountains 40000000 00000000 3f800000 c64cf8116fe56375 | new mountains 40800000 00000000 3f800000 b06f7c5212872adb | new mountains 3fc00000 00000000 3f800000 5a6452a018c3cb45 | new mountains 40000000 00000000 3f800000 1c97a10971c5c81e | new hills 3fc00000 00000000 3f800000 41b501ca3e73b566 | new flats 3f800000 00000000 3f800000 6d5fe783b115a214 | new mountain_chain 3e99999a 00000000 3f800000 393b20433755c94f | new mountains 3fa00000 00000000 3f800000 b02292a3ec6bf12d | new plateau 3f800000 3f400000 3f400000 cde89548da4b0935
entriesNoFancy 9 | 0 flats 3f800000 3e99999a 3e99999a af86cb2d4435bc85 | 1 hills 40000000 00000000 3f800000 32dcf24feea08bc5 | new mountains 40200000 00000000 3f800000 2c9cb534425e4f3d | new mountains 3fc00000 00000000 3f800000 17f384bb927b5e59 | new mountains 40000000 00000000 3f800000 70994fc3f2ad2c72 | new hills 3f800000 00000000 3f800000 96e4df2e663b18e6 | 7 flats 3f800000 00000000 3f800000 367e5d44db33acfd | new mountain_chain 3e99999a 00000000 3f800000 9d6d193f64f2f8ab | 9 mountains 3fa00000 00000000 3f800000 3563e107b5c1d7f9
";

    const IDS: [&str; 11] = [
        "flatlands",
        "hills_1",
        "mountains_1",
        "mountains_ridge_1",
        "dolomites",
        "torridonian",
        "dales",
        "steppe",
        "othermod:mountain_peaks",
        "Mountains_2",
        "plateau_ridge",
    ];
    const TERRAINS: [&str; 11] = [
        "flats",
        "hills",
        "mountains",
        "mountains",
        "mountains",
        "mountains",
        "hills",
        "flats",
        "mountain_chain",
        "mountains",
        "plateau",
    ];
    const WEIGHTS: [f32; 11] = [1.0, 2.0, 2.5, 0.75, 1.5, 2.0, 1.5, 1.0, 0.3, 1.25, 1.0];
    const SEEDS: [i32; 4] = [0, 17, -91, i32::MAX];
    const POINTS: [(f32, f32); 5] = [
        (0.0, 0.0),
        (0.125, -0.75),
        (350.0, -175.0),
        (-511.5, 93.25),
        (12345.6, -6543.2),
    ];

    fn source(kind: SourceKind, seed: i32, frequency: f32, octaves: usize, gain: f32) -> Module {
        Module::source(Source::new(
            kind,
            SourceConfig {
                seed,
                octaves,
                gain,
                frequency,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    }

    fn noise(i: usize) -> Module {
        match i {
            0 => Module::constant(0.3),
            1 => source(SourceKind::Perlin, 11, 1.0 / 200.0, 3, 0.5),
            2 => source(SourceKind::Simplex, 12, 1.0 / 300.0, 4, 0.5),
            3 => source(SourceKind::Ridge, 13, 1.0 / 250.0, 2, 0.975),
            4 => source(SourceKind::Perlin2, 14, 1.0 / 180.0, 2, 0.5),
            5 => source(SourceKind::Billow, 15, 1.0 / 220.0, 3, 0.5),
            6 => source(SourceKind::Cubic, 16, 1.0 / 160.0, 2, 0.5),
            7 => source(SourceKind::Simplex2, 17, 1.0 / 400.0, 3, 0.5),
            8 => source(SourceKind::SimplexRidge, 18, 1.0 / 350.0, 2, 0.975),
            9 => source(SourceKind::Perlin, 19, 1.0 / 128.0, 2, 0.5),
            _ => Module::constant(0.6),
        }
    }

    fn entry(weight: f32, vertical: f32, horizontal: f32, base: f32) -> TerrainFormSettings {
        TerrainFormSettings {
            weight,
            base_scale: base,
            vertical_scale: vertical,
            horizontal_scale: horizontal,
        }
    }

    fn case(
        registry: &TerrainRegistry,
        name: &str,
        inputs: &[usize],
        setup: impl FnOnce(&mut CurrentTerrainSettings),
    ) -> String {
        let terrains: Vec<_> = inputs
            .iter()
            .map(|&i| {
                let terrain = registry.get_by_name(TERRAINS[i]).unwrap();
                Arc::new(TerrainNoise::new(terrain, WEIGHTS[i], noise(i)))
            })
            .collect();
        let ids: Vec<_> = inputs.iter().map(|&i| IDS[i].to_owned()).collect();
        let mut settings = CurrentTerrainSettings::default();
        setup(&mut settings);
        let result = apply_terrain_overrides(&terrains, &ids, &settings);
        let mut line = format!("{name} {}", result.len());
        for terrain in &result {
            let same = terrains
                .iter()
                .position(|input| Arc::ptr_eq(input, terrain))
                .map_or("new".to_owned(), |k| inputs[k].to_string());
            let input = registry.get_by_name(terrain.terrain().name()).unwrap();
            assert!(
                terrain.terrain().is_same(&input),
                "{name}: world registry's terrain"
            );
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            for seed in SEEDS {
                for (x, z) in POINTS {
                    let value = terrain.noise().try_value_seed_first(seed, x, z).unwrap();
                    let bits = if value.is_nan() {
                        0x7fc0_0000
                    } else {
                        value.to_bits()
                    };
                    hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            line += &format!(
                " | {same} {} {:08x} {:08x} {:08x} {hash:016x}",
                terrain.terrain().name(),
                terrain.weight().to_bits(),
                terrain.noise().min_value().to_bits(),
                terrain.noise().max_value().to_bits(),
            );
        }
        line + "\n"
    }

    fn cases() -> String {
        let registry = TerrainRegistry::new();
        let all: Vec<usize> = (0..11).collect();
        let width = |w: f32| move |s: &mut CurrentTerrainSettings| s.general.mountain_width = w;
        let mut out = String::new();
        let mut run =
            |name: &str, inputs: &[usize], setup: &dyn Fn(&mut CurrentTerrainSettings)| {
                out += &case(&registry, name, inputs, setup);
            };
        run("default", &all, &|_| {});
        run("neutral", &all, &width(1.0));
        run("width0", &all, &width(0.0));
        run("widthNaN", &all, &width(f32::NAN));
        run("widthNeg", &all, &width(-2.0));
        run("widthNegZero", &all, &width(-0.0));
        run("width2", &all, &width(2.0));
        run("width6", &all, &width(6.0));
        run("width0.3", &all, &width(0.3));
        run("widthMin", &all, &width(f32::from_bits(1)));
        run("widthInf", &all, &width(f32::INFINITY));
        run("noFancy", &all, &|s| {
            s.general.fancy_mountains = false;
            s.general.mountain_width = 1.0;
        });
        run("noFancyDefault", &all, &|s| {
            s.general.fancy_mountains = false
        });
        run("noFancyOnlyRidges", &[3, 10], &|s| {
            s.general.fancy_mountains = false
        });
        run("noFancyOnlyRidgesNeutral", &[3, 10], &|s| {
            s.general.fancy_mountains = false;
            s.general.mountain_width = 1.0;
        });
        run("empty", &[], &|s| s.general.global_vertical_scale = 0.5);
        run("global", &all, &|s| {
            s.general.global_vertical_scale = 0.75;
            s.general.mountain_width = 1.0;
        });
        run("globalNaN", &all, &|s| {
            s.general.global_vertical_scale = f32::NAN;
            s.general.mountain_width = 1.0;
        });
        run("global2Width3", &all, &|s| {
            s.general.global_vertical_scale = 2.0;
            s.general.mountain_width = 3.0;
        });
        run("entries", &all, &|s| {
            s.general.mountain_width = 1.0;
            let nan = f32::NAN;
            for (id, e) in [
                ("flatlands", entry(0.0, 1.0, 1.0, 1.0)),
                ("hills_1", entry(3.0, 0.8, 4.0, 1.5)),
                ("dolomites", entry(1.5, 1.0, 0.005, 1.0)),
                ("torridonian", entry(2.0, 1.3, 0.0, 1.0)),
                ("dales", entry(1.5, 0.0, -1.0, 0.5)),
                ("steppe", entry(nan, nan, nan, nan)),
                (
                    "othermod:mountain_peaks",
                    entry(0.3, 1.0, f32::INFINITY, 2.0),
                ),
                ("mountains_1", entry(2.5, 1.0, 0.3, 0.1)),
                ("plateau_ridge", entry(1.0, 2.0, 5.0, 0.0)),
                ("MOUNTAINS_2", entry(9.0, 2.0, 2.0, 2.0)),
                ("missing", entry(9.0, 2.0, 2.0, 2.0)),
            ] {
                s.terrains.insert(id.to_owned(), e);
            }
        });
        run("entriesAll", &all, &|s| {
            s.general.global_vertical_scale = 1.25;
            s.general.mountain_width = 2.5;
            s.terrains
                .insert("mountains_1".into(), entry(2.0, 0.6, 2.5, 1.2));
            s.terrains
                .insert("hills_1".into(), entry(1.0, 1.0, 1.0, 1.0));
            s.terrains
                .insert("mountains_ridge_1".into(), entry(4.0, 1.1, 0.7, 0.9));
        });
        run("entriesNoFancy", &all, &|s| {
            s.general.fancy_mountains = false;
            s.general.mountain_width = 1.5;
            s.terrains
                .insert("mountains_ridge_1".into(), entry(4.0, 1.1, 0.7, 0.9));
            s.terrains.insert("dales".into(), entry(1.0, 1.0, 1.0, 1.0));
        });
        out
    }

    #[test]
    fn overrides_match_reimagined_java() {
        let rust = cases();
        for (rust, java) in rust.lines().zip(ORACLE.lines()) {
            assert_eq!(rust, java);
        }
        assert_eq!(rust, ORACLE);
    }
}
