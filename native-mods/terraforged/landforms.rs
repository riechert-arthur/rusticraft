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

use super::{
    terraforged::{Domain, Interpolation, Module, Source, SourceConfig, SourceKind},
    terraforged_engine::{Seed, TerrainSettings},
};

pub struct LandForms {
    settings: TerrainSettings,
}

impl LandForms {
    pub const fn new(settings: TerrainSettings) -> Self {
        Self { settings }
    }

    pub fn steppe(&self, seed: &mut Seed) -> Module {
        let scale_h = java_round(250.0 * self.settings.steppe.horizontal_scale);
        let erosion =
            perlin(seed.next(), scale_h.wrapping_mul(2), 3, 3.75).alpha(Module::constant(0.45));
        let warp_x = perlin(seed.next(), scale_h / 4, 3, 3.0);
        let warp_y = perlin(seed.next(), scale_h / 4, 3, 3.0);
        Module::multiply(vec![perlin(seed.next(), scale_h, 1, 2.0), erosion])
            .warp(Domain::domain_warp(
                warp_x,
                warp_y,
                Module::constant(scale_h as f32 / 4.0),
            ))
            .warp(noise_warp(seed.next(), 256, 1, 200.0))
            .scale(Module::constant(0.08))
            .bias(Module::constant(-0.02))
    }

    pub fn plains(&self, seed: &mut Seed) -> Module {
        let scale_h = java_round(250.0 * self.settings.plains.horizontal_scale);
        let erosion =
            perlin(seed.next(), scale_h.wrapping_mul(2), 3, 3.75).alpha(Module::constant(0.45));
        let warp_x = perlin(seed.next(), scale_h / 4, 3, 3.5);
        let warp_y = perlin(seed.next(), scale_h / 4, 3, 3.5);
        Module::multiply(vec![perlin(seed.next(), scale_h, 1, 2.0), erosion])
            .warp(Domain::domain_warp(
                warp_x,
                warp_y,
                Module::constant(scale_h as f32 / 4.0),
            ))
            .warp(noise_warp(seed.next(), 256, 1, 256.0))
            .scale(Module::constant(
                0.15 * self.settings.general.global_vertical_scale,
            ))
            .bias(Module::constant(-0.02))
    }
}

fn perlin(seed: i32, scale: i32, octaves: usize, lacunarity: f32) -> Module {
    Module::source(Source::new(
        SourceKind::Perlin,
        SourceConfig {
            seed,
            octaves,
            frequency: 1.0 / scale as f32,
            lacunarity,
            interpolation: Interpolation::Curve3,
            ..SourceConfig::default()
        },
    ))
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        perlin(seed, scale, octaves, 2.0),
        perlin(seed.wrapping_add(1), scale, octaves, 2.0),
        Module::constant(strength),
    )
}

/// Java's `Math.round(float)`: the floor of `value + 0.5` computed exactly (in `f64`, where adding 0.5 to an `f32`
/// never rounds), so 0.49999997 rounds to 0 and odd integers above 2^23 stay odd; NaN gives 0 and out-of-range
/// values saturate, as `as i32` does.
fn java_round(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(module: &Module) -> [u32; 4] {
        [
            module.value(0.0, 0.0).to_bits(),
            module.value(12.25, -98.75).to_bits(),
            module.value(-4096.5, 8192.25).to_bits(),
            module.value(123456.75, -0.125).to_bits(),
        ]
    }

    #[test]
    fn steppe_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(12_345);
        let module = forms.steppe(&mut seed);
        assert_eq!(seed.get(), 12_350);
        assert_eq!(module.min_value().to_bits(), 0xbca3d70a);
        assert_eq!(module.max_value().to_bits(), 0x3d75c28f);
        assert_eq!(
            bits(&module),
            [0x3c343956, 0xbba4ee2e, 0x3beb98d8, 0x3c5e0bfc]
        );
    }

    #[test]
    fn plains_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(-998_877);
        let module = forms.plains(&mut seed);
        assert_eq!(seed.get(), -998_872);
        assert_eq!(module.min_value().to_bits(), 0xbca3d70a);
        assert_eq!(module.max_value().to_bits(), 0x3e020c4b);
        assert_eq!(
            bits(&module),
            [0x3d1765ff, 0x3d7e13ab, 0x3d204c29, 0x3d3263a5]
        );
    }

    #[test]
    fn scaled_forms_match_engine_0_3_0() {
        let mut settings = TerrainSettings::default();
        settings.steppe.horizontal_scale = 0.25;
        settings.plains.horizontal_scale = 1.75;
        settings.general.global_vertical_scale = 0.43;
        let forms = LandForms::new(settings);

        let mut steppe_seed = Seed::new(i32::MAX - 2);
        let steppe = forms.steppe(&mut steppe_seed);
        assert_eq!(steppe_seed.get(), i32::MIN + 2);
        assert_eq!(
            bits(&steppe),
            [0x3c343956, 0x3d0110f9, 0x3c823652, 0x3c7ca83c]
        );

        let mut plains_seed = Seed::new(i32::MIN + 2);
        let plains = forms.plains(&mut plains_seed);
        assert_eq!(plains_seed.get(), i32::MIN + 7);
        assert_eq!(plains.max_value().to_bits(), 0x3d3645a3);
        assert_eq!(
            bits(&plains),
            [0x3ba3a2a0, 0x3b056338, 0xbbfff534, 0x3bcfdf6c]
        );
    }
    /// Engine 0.3.0's `LandForms.steppe`/`plains` (`LandFormsOracle`, kept outside Git): per settings and start seed,
    /// the seed after building, the bounds and an FNV-1a hash of the raw bits of every sample on `GRID` x `GRID`.
    const GRID: [f32; 10] = [
        0.0, 12.25, -98.75, -4096.5, 8192.25, 123456.75, -0.125, -77777.5, 1e6, -3.0e5,
    ];

    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const ORACLE: [(&str, &str, i32, i32, u32, u32, u64); 22] = [
        ("default", "steppe", 12345, 12350, 0xbca3d70a, 0x3d75c28f, 0x639d1d83e3e0f2eb),
        ("default", "plains", 12345, 12350, 0xbca3d70a, 0x3e020c4b, 0x6e4af950d6b9b37f),
        ("default", "steppe", -998877, -998872, 0xbca3d70a, 0x3d75c28f, 0x7a376023496a4361),
        ("default", "plains", -998877, -998872, 0xbca3d70a, 0x3e020c4b, 0x70dc912c2a72e694),
        ("modterrains", "steppe", 9712416, 9712421, 0xbca3d70a, 0x3d75c28f, 0x8f72e0ca682b02ca),
        ("modterrains", "plains", 9712416, 9712421, 0xbca3d70a, 0x3e051eb9, 0x8a73fd5503b12098),
        ("scaled", "steppe", 2147483645, -2147483646, 0xbca3d70a, 0x3d75c28f, 0xb918d2033648e902),
        ("scaled", "plains", 2147483645, -2147483646, 0xbca3d70a, 0x3d3645a3, 0xc38b849bb3dd7d69),
        ("scaled", "steppe", -2147483646, -2147483641, 0xbca3d70a, 0x3d75c28f, 0xd66522f75e6e4e3c),
        ("scaled", "plains", -2147483646, -2147483641, 0xbca3d70a, 0x3d3645a3, 0xf74f6abddcc0f328),
        ("halfdown", "steppe", 7, 12, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("halfdown", "plains", 7, 12, 0xbca3d70a, 0x3e051eb9, 0xff17844fedc4a4c5),
        ("oddbig", "steppe", -7, -2, 0xbca3d70a, 0x3d75c28f, 0xba1362b74aa359d2),
        ("oddbig", "plains", -7, -2, 0xbca3d70a, 0x3e8f5c29, 0x3e41276435ef31ee),
        ("tiny", "steppe", 0, 5, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("tiny", "plains", 0, 5, 0xbca3d70a, 0xbca3d70a, 0xff17844fedc4a4c5),
        ("negative", "steppe", 99, 104, 0xbca3d70a, 0x3d75c28f, 0x40e88a51c53467e4),
        ("negative", "plains", 99, 104, 0xbca3d70a, 0xbe2e147b, 0x91a54a45a82a6b97),
        ("nan", "steppe", 5, 10, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("nan", "plains", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("huge", "steppe", 3, 8, 0xbca3d70a, 0x3d75c28f, 0x0a645dffde6efa79),
        ("huge", "plains", 3, 8, 0xbca3d70a, 0x6ff2568d, 0xdada775cdb02f309),
    ];

    fn oracle_settings(name: &str) -> TerrainSettings {
        let (steppe, plains, vertical) = match name {
            "default" => return TerrainSettings::default(),
            "modterrains" => (1.0, 1.0, 1.0),
            "scaled" => (0.25, 1.75, 0.43),
            // 250 * h == 0.49999997: Math.round gives 0, so the frequency is infinite.
            "halfdown" => (f32::from_bits(0x3b03126e), f32::from_bits(0x3b03126e), 1.0),
            // 250 * h == 8388609: Math.round keeps the odd integer.
            "oddbig" => (f32::from_bits(0x47031270), f32::from_bits(0x47031270), 2.0),
            "tiny" => (0.006, 0.01, 0.0),
            "negative" => (-1.3, -0.5, -1.0),
            "nan" => (f32::NAN, f32::NAN, f32::NAN),
            "huge" => (1e30, 1e30, 1e30),
            _ => unreachable!("{name}"),
        };
        let mut settings = TerrainSettings::default();
        settings.steppe.horizontal_scale = steppe;
        settings.plains.horizontal_scale = plains;
        settings.general.global_vertical_scale = vertical;
        settings
    }

    #[test]
    fn grids_match_engine_0_3_0() {
        for (name, kind, start, after, min, max, hash) in ORACLE {
            let forms = LandForms::new(oracle_settings(name));
            let mut seed = Seed::new(start);
            let module = match kind {
                "steppe" => forms.steppe(&mut seed),
                _ => forms.plains(&mut seed),
            };
            // NaN settings make Scale multiply two NaNs; which NaN's bits come out is up to the compiler (HotSpot
            // returns the scale's +NaN, a debug Rust build the source's -NaN), so only NaN-ness is checked there.
            let nan_times_nan = name == "nan" && kind == "plains";
            let mut fnv = 0xcbf2_9ce4_8422_2325_u64;
            for x in GRID {
                for y in GRID {
                    let mut value = module.value(x, y);
                    if nan_times_nan {
                        assert!(value.is_nan(), "{name} {kind} at {x} {y}: {value}");
                        value = f32::from_bits(0x7fc0_0000);
                    }
                    for byte in value.to_bits().to_le_bytes() {
                        fnv = (fnv ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
            let got = (
                seed.get(),
                module.min_value().to_bits(),
                module.max_value().to_bits(),
                fnv,
            );
            assert_eq!(got, (after, min, max, hash), "{name} {kind} {start}");
        }
    }
}
