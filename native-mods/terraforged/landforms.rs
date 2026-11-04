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
    terraforged::{CurveFunction, Domain, Interpolation, Module, Source, SourceConfig, SourceKind},
    terraforged_engine::{Levels, Seed, TerrainSettings},
};

/// Engine's `LandForms`: builds each landform's noise graph from the terrain settings, the world's `Levels` (only
/// `deepOcean` reads them, through their `water` level) and the `ground` module (`getLandBase`). Only `deepOcean`,
/// `steppe`, `plains`, `hills1`, `hills2`, `dales`, `plateau`, `badlands` and `torridonian` and the two bases are
/// ported so far.
pub struct LandForms {
    settings: TerrainSettings,
    terrain_vertical_scale: f32,
    sea_level: f32,
    ground: Module,
}

impl LandForms {
    pub fn new(settings: TerrainSettings, levels: Levels, ground: Module) -> Self {
        Self {
            terrain_vertical_scale: settings.general.global_vertical_scale,
            sea_level: levels.water,
            settings,
            ground,
        }
    }

    /// `LandForms.getOceanBase()`: `Source.ZERO`.
    pub fn ocean_base(&self) -> Module {
        Module::constant(0.0)
    }

    /// `LandForms.getLandBase()`: the `ground` module the forms were built with.
    pub fn land_base(&self) -> Module {
        self.ground.clone()
    }

    /// `LandForms.deepOcean(int)`: takes a plain `int` seed rather than a `Seed`, pre-increments it six times (wrapping
    /// like Java's `int`) and scales by the water level, `(double) seaLevel * 0.7` in `f64` and `seaLevel * 0.2f` /
    /// `0.15f` in `f32` as in Engine.
    pub fn deep_ocean(&self, mut seed: i32) -> Module {
        seed = seed.wrapping_add(1);
        let hills_source = perlin(seed, 150, 3, 2.0)
            .scale(Module::constant((f64::from(self.sea_level) * 0.7) as f32));
        seed = seed.wrapping_add(1);
        let hills_bias = perlin(seed, 200, 1, 2.0).scale(Module::constant(self.sea_level * 0.2));
        let hills = hills_source.bias(hills_bias);

        seed = seed.wrapping_add(1);
        let canyon_source = perlin(seed, 150, 4, 2.0)
            .power_curve(0.2)
            .invert()
            .scale(Module::constant((f64::from(self.sea_level) * 0.7) as f32));
        seed = seed.wrapping_add(1);
        let canyon_bias = perlin(seed, 170, 1, 2.0).scale(Module::constant(self.sea_level * 0.15));
        let canyons = canyon_source.bias(canyon_bias);

        seed = seed.wrapping_add(1);
        let ocean =
            perlin(seed, 500, 1, 2.0).blend(hills, canyons, 0.6, 0.65, Interpolation::Linear);
        seed = seed.wrapping_add(1);
        ocean.warp(noise_warp(seed, 50, 2, 50.0))
    }

    /// `LandForms.steppe(Seed)`: draws five seeds in Engine's order; ignores the global vertical scale.
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

    /// `LandForms.plains(Seed)`: draws five seeds in Engine's order; scales by `0.15 * globalVerticalScale`.
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
            .scale(Module::constant(0.15 * self.terrain_vertical_scale))
            .bias(Module::constant(-0.02))
    }

    /// `LandForms.hills1(Seed)`: draws four seeds in Engine's order; scales by `0.6 * globalVerticalScale`.
    pub fn hills1(&self, seed: &mut Seed) -> Module {
        let hills = Module::multiply(vec![
            perlin(seed.next(), 200, 3, 2.0),
            source(SourceKind::Billow, seed.next(), 400, 3, 0.5, 2.0).alpha(Module::constant(0.5)),
        ]);
        hills
            .warp(noise_warp(seed.next(), 30, 3, 20.0))
            .warp(noise_warp(seed.next(), 400, 3, 200.0))
            .scale(Module::constant(0.6 * self.terrain_vertical_scale))
    }

    /// `LandForms.hills2(Seed)`: draws five seeds in Engine's order; scales by `0.55 * globalVerticalScale`.
    pub fn hills2(&self, seed: &mut Seed) -> Module {
        let hills = Module::multiply(vec![
            source(SourceKind::Cubic, seed.next(), 128, 2, 0.5, 2.0),
            perlin(seed.next(), 32, 4, 2.0).alpha(Module::constant(0.075)),
        ])
        .warp(noise_warp(seed.next(), 30, 3, 20.0))
        .warp(noise_warp(seed.next(), 400, 3, 200.0));
        Module::multiply(vec![
            hills,
            // `Builder.ridge()` defaults the gain to 0.975 rather than 0.5.
            source(SourceKind::Ridge, seed.next(), 512, 2, 0.975, 2.0).alpha(Module::constant(0.8)),
        ])
        .scale(Module::constant(0.55 * self.terrain_vertical_scale))
    }

    /// `LandForms.dales(Seed)`: draws four seeds in Engine's order; ignores the landform settings.
    pub fn dales(&self, seed: &mut Seed) -> Module {
        let hills1 = source(SourceKind::Billow, seed.next(), 300, 4, 0.8, 4.0)
            .power_curve(0.5)
            .scale(Module::constant(0.75));
        let hills2 =
            source(SourceKind::Billow, seed.next(), 350, 3, 0.8, 4.0).power(Module::constant(1.25));
        let combined = perlin(seed.next(), 400, 1, 2.0)
            .clamp(Module::constant(0.3), Module::constant(0.6))
            .map(Module::constant(0.0), Module::constant(1.0))
            .blend(hills1, hills2, 0.4, 0.75, Interpolation::Linear);
        combined
            .power(Module::constant(1.125))
            .warp(noise_warp(seed.next(), 300, 1, 100.0))
            .scale(Module::constant(0.4))
    }

    /// `LandForms.plateau(Seed)`: draws nine seeds in Engine's order; scales by `0.475 * globalVerticalScale`,
    /// computed in `f64` as Engine does.
    pub fn plateau(&self, seed: &mut Seed) -> Module {
        let valley = source(SourceKind::Ridge, seed.next(), 500, 1, 0.975, 2.0)
            .invert()
            .warp(noise_warp(seed.next(), 100, 1, 150.0))
            .warp(noise_warp(seed.next(), 20, 1, 15.0));
        let top = source(SourceKind::Ridge, seed.next(), 150, 3, 0.975, 2.45)
            .warp(noise_warp(seed.next(), 300, 1, 150.0))
            .warp(noise_warp(seed.next(), 40, 2, 20.0))
            .scale(Module::constant(0.15));
        let top_mask = valley
            .clone()
            .clamp(Module::constant(0.02), Module::constant(0.1))
            .map(Module::constant(0.0), Module::constant(1.0));
        let top = Module::multiply(vec![top, top_mask]);
        let surface = perlin(seed.next(), 20, 3, 2.0)
            .scale(Module::constant(0.05))
            .warp(noise_warp(seed.next(), 40, 2, 20.0));
        let body = Module::multiply(vec![
            valley,
            source(SourceKind::Cubic, seed.next(), 500, 1, 0.5, 2.0)
                .scale(Module::constant(0.6))
                .bias(Module::constant(0.3)),
        ]);
        let terraced = Module::add(vec![body, top]).terrace(
            Module::constant(0.9),
            Module::constant(0.15),
            Module::constant(0.35),
            4,
            0.4,
        );
        Module::add(vec![terraced, surface]).scale(Module::constant(
            (0.475_f64 * f64::from(self.terrain_vertical_scale)) as f32,
        ))
    }

    /// `LandForms.badlands(Seed)`: draws six seeds in Engine's order and ignores the settings.
    pub fn badlands(&self, seed: &mut Seed) -> Module {
        let mask = perlin(seed.next(), 270, 3, 2.0)
            .clamp(Module::constant(0.35), Module::constant(0.65))
            .map(Module::constant(0.0), Module::constant(1.0));
        let hills = source(SourceKind::Ridge, seed.next(), 275, 4, 0.975, 2.0)
            .warp(noise_warp(seed.next(), 400, 2, 100.0))
            .warp(noise_warp(seed.next(), 18, 1, 20.0));
        let hills = Module::multiply(vec![hills, mask]);
        // Engine's `double`s, narrowed by `scale(double)`'s `Source.constant`.
        let modulation = 0.4_f64;
        let alpha = (1.0 - modulation) as f32;
        let modulation = modulation as f32;
        let mod1 = hills
            .clone()
            .warp(noise_warp(seed.next(), 100, 1, 50.0))
            .scale(Module::constant(modulation));
        let low_frequency = Module::add(vec![
            hills
                .clone()
                .steps(
                    Module::constant(4.0),
                    Module::constant(0.6),
                    Module::constant(0.7),
                    CurveFunction::Interpolation(Interpolation::Linear),
                )
                .scale(Module::constant(alpha)),
            mod1.clone(),
        ]);
        let high_frequency = Module::add(vec![
            hills
                .clone()
                .steps(
                    Module::constant(10.0),
                    Module::constant(0.6),
                    Module::constant(0.7),
                    CurveFunction::Interpolation(Interpolation::Linear),
                )
                .scale(Module::constant(alpha)),
            mod1,
        ]);
        let detail = Module::add(vec![low_frequency, high_frequency]);
        let mod2 = Module::multiply(vec![
            hills.clone(),
            perlin(seed.next(), 200, 3, 2.0).scale(Module::constant(modulation)),
        ]);
        let shape = Module::add(vec![
            hills
                .steps(
                    Module::constant(4.0),
                    Module::constant(0.65),
                    Module::constant(0.75),
                    CurveFunction::Interpolation(Interpolation::Curve3),
                )
                .scale(Module::constant(alpha)),
            mod2,
        ])
        .scale(Module::constant(alpha));
        Module::multiply(vec![shape, detail.alpha(Module::constant(0.5))])
            .scale(Module::constant(0.55))
            .bias(Module::constant(0.025))
    }

    /// `LandForms.torridonian(Seed)`: draws nine seeds in Engine's order and ignores the settings.
    pub fn torridonian(&self, seed: &mut Seed) -> Module {
        let plains = perlin(seed.next(), 100, 3, 2.0)
            .warp(noise_warp(seed.next(), 300, 1, 150.0))
            .warp(noise_warp(seed.next(), 20, 1, 40.0))
            .scale(Module::constant(0.15));
        let hills = perlin(seed.next(), 150, 4, 2.0)
            .warp(noise_warp(seed.next(), 300, 1, 200.0))
            .warp(noise_warp(seed.next(), 20, 2, 20.0))
            .boost(1);
        let module =
            perlin(seed.next(), 200, 3, 2.0).blend(plains, hills, 0.6, 0.6, Interpolation::Linear);
        module
            .advanced_terrace(
                perlin(seed.next(), 120, 1, 2.0).scale(Module::constant(0.25)),
                perlin(seed.next(), 200, 1, 2.0)
                    .scale(Module::constant(0.5))
                    .bias(Module::constant(0.5)),
                Module::constant(0.5),
                0.0,
                0.3,
                6,
                1,
            )
            .boost(1)
            .scale(Module::constant(0.5))
    }
}

/// `Source.build(seed, scale, octaves).lacunarity(lacunarity).perlin()` (Engine's builder defaults otherwise).
fn perlin(seed: i32, scale: i32, octaves: usize, lacunarity: f32) -> Module {
    source(SourceKind::Perlin, seed, scale, octaves, 0.5, lacunarity)
}

/// `Source.build(seed, scale, octaves).gain(gain).lacunarity(lacunarity)` built as `kind`.
fn source(
    kind: SourceKind,
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain,
            frequency: 1.0 / scale as f32,
            lacunarity,
            interpolation: Interpolation::Curve3,
        },
    ))
}

/// The domain of `Module.warp(seed, scale, octaves, strength)`: Perlin axes seeded `seed` and `seed + 1`.
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

    fn make_forms(settings: TerrainSettings) -> LandForms {
        LandForms::new(settings, Levels::new(256, 63), Module::constant(0.0))
    }

    #[test]
    fn steppe_matches_engine_0_3_0() {
        let forms = make_forms(TerrainSettings::default());
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
        let forms = make_forms(TerrainSettings::default());
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
        let forms = make_forms(settings);

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
    /// Engine 0.3.0's `LandForms.steppe`/`plains`/`hills1`/`hills2`/`dales`/`plateau`/`badlands`/`torridonian`
    /// (`LandFormsOracle`, kept outside Git): per settings and start seed, the seed after building, the bounds and an
    /// FNV-1a hash of the raw bits of every sample on `GRID` x `GRID`.
    const GRID: [f32; 10] = [
        0.0, 12.25, -98.75, -4096.5, 8192.25, 123456.75, -0.125, -77777.5, 1e6, -3.0e5,
    ];

    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const ORACLE: [(&str, &str, i32, i32, u32, u32, u64); 128] = [
        ("default", "steppe", 12345, 12350, 0xbca3d70a, 0x3d75c28f, 0x639d1d83e3e0f2eb),
        ("default", "plains", 12345, 12350, 0xbca3d70a, 0x3e020c4b, 0x6e4af950d6b9b37f),
        ("default", "hills1", 12345, 12349, 0x00000000, 0x3f16872c, 0x6d5563d0ee663df0),
        ("default", "hills2", 12345, 12350, 0x80000000, 0x3f1b3b65, 0x346e5dcff0a3c6a2),
        ("default", "dales", 12345, 12349, 0x00000000, 0x3ecccccd, 0xcc6308d1c64458e5),
        ("default", "plateau", 12345, 12354, 0x00000000, 0x3ee26b50, 0x5acae80b70cf5ebf),
        ("default", "badlands", 12345, 12351, 0x3ccccccd, 0x3f2f5c29, 0xbd71d98b4e7bcdc6),
        ("default", "torridonian", 12345, 12354, 0x00000000, 0x3f000000, 0xd96420d2d809c767),
        ("default", "steppe", -998877, -998872, 0xbca3d70a, 0x3d75c28f, 0x7a376023496a4361),
        ("default", "plains", -998877, -998872, 0xbca3d70a, 0x3e020c4b, 0x70dc912c2a72e694),
        ("default", "hills1", -998877, -998873, 0x00000000, 0x3f16872c, 0x511560374f860dea),
        ("default", "hills2", -998877, -998872, 0x80000000, 0x3f1b3b65, 0xc8d97f6e98f49a97),
        ("default", "dales", -998877, -998873, 0x00000000, 0x3ecccccd, 0xb3f6238244af33fb),
        ("default", "plateau", -998877, -998868, 0x00000000, 0x3ee26b50, 0x709f0400874ac2aa),
        ("default", "badlands", -998877, -998871, 0x3ccccccd, 0x3f2f5c29, 0x54b41cb31c853c2e),
        ("default", "torridonian", -998877, -998868, 0x00000000, 0x3f000000, 0x67f4f82dc0503ca9),
        ("default", "steppe", 2147483643, -2147483648, 0xbca3d70a, 0x3d75c28f, 0xffdf4562b5fae442),
        ("default", "plains", 2147483643, -2147483648, 0xbca3d70a, 0x3e020c4b, 0x5b03f6d91ab51dc4),
        ("default", "hills1", 2147483643, 2147483647, 0x00000000, 0x3f16872c, 0x5d9f3174bb93dab8),
        ("default", "hills2", 2147483643, -2147483648, 0x80000000, 0x3f1b3b65, 0x52177bc2f339df26),
        ("default", "dales", 2147483643, 2147483647, 0x00000000, 0x3ecccccd, 0x1bf3b706a93f5f84),
        ("default", "plateau", 2147483643, -2147483644, 0x00000000, 0x3ee26b50, 0x6c64d5bc5e9730c6),
        ("default", "badlands", 2147483643, -2147483647, 0x3ccccccd, 0x3f2f5c29, 0x78b3f2c8b2b756c8),
        ("default", "torridonian", 2147483643, -2147483644, 0x00000000, 0x3f000000, 0x4ca5f05a7898bf16),
        ("default", "steppe", 2147483639, 2147483644, 0xbca3d70a, 0x3d75c28f, 0xe9fc128adbe3435e),
        ("default", "plains", 2147483639, 2147483644, 0xbca3d70a, 0x3e020c4b, 0xb7d09708de9ebe32),
        ("default", "hills1", 2147483639, 2147483643, 0x00000000, 0x3f16872c, 0x1265553acce2f97b),
        ("default", "hills2", 2147483639, 2147483644, 0x80000000, 0x3f1b3b65, 0xeac06bbf6c6fa4e4),
        ("default", "dales", 2147483639, 2147483643, 0x00000000, 0x3ecccccd, 0x964cbfdab477cce6),
        ("default", "plateau", 2147483639, -2147483648, 0x00000000, 0x3ee26b50, 0x4114d1db7d3f7a6c),
        ("default", "badlands", 2147483639, 2147483645, 0x3ccccccd, 0x3f2f5c29, 0x4d523ae07a646c72),
        ("default", "torridonian", 2147483639, -2147483648, 0x00000000, 0x3f000000, 0xeacecc7c2bcea0fe),
        ("modterrains", "steppe", 9712416, 9712421, 0xbca3d70a, 0x3d75c28f, 0x8f72e0ca682b02ca),
        ("modterrains", "plains", 9712416, 9712421, 0xbca3d70a, 0x3e051eb9, 0x8a73fd5503b12098),
        ("modterrains", "hills1", 9712416, 9712420, 0x00000000, 0x3f19999a, 0xe86ad3cd86529607),
        ("modterrains", "hills2", 9712416, 9712421, 0x80000000, 0x3f1e6667, 0x895f048c9154db49),
        ("modterrains", "dales", 9712416, 9712420, 0x00000000, 0x3ecccccd, 0x7f19c2df0ce3dbe4),
        ("modterrains", "plateau", 9712416, 9712425, 0x00000000, 0x3ee70a3d, 0x03abed4354d577c4),
        ("modterrains", "badlands", 9712416, 9712422, 0x3ccccccd, 0x3f2f5c29, 0x1894db7e94fc12eb),
        ("modterrains", "torridonian", 9712416, 9712425, 0x00000000, 0x3f000000, 0x7d587bc7a0c7908d),
        ("scaled", "steppe", 2147483645, -2147483646, 0xbca3d70a, 0x3d75c28f, 0xb918d2033648e902),
        ("scaled", "plains", 2147483645, -2147483646, 0xbca3d70a, 0x3d3645a3, 0xc38b849bb3dd7d69),
        ("scaled", "hills1", 2147483645, -2147483647, 0x00000000, 0x3e841894, 0xdde8b6352082ee67),
        ("scaled", "hills2", 2147483645, -2147483646, 0x80000000, 0x3e883958, 0xf830272ce50e01f5),
        ("scaled", "dales", 2147483645, -2147483647, 0x00000000, 0x3ecccccd, 0xf3734c7bd6de92ce),
        ("scaled", "plateau", 2147483645, -2147483642, 0x00000000, 0x3e46b1c5, 0xa84838c2a2c6af31),
        ("scaled", "badlands", 2147483645, -2147483645, 0x3ccccccd, 0x3f2f5c29, 0x05aa29a6c2b389bd),
        ("scaled", "torridonian", 2147483645, -2147483642, 0x00000000, 0x3f000000, 0xeb48ac1a78d73cd8),
        ("scaled", "steppe", -2147483646, -2147483641, 0xbca3d70a, 0x3d75c28f, 0xd66522f75e6e4e3c),
        ("scaled", "plains", -2147483646, -2147483641, 0xbca3d70a, 0x3d3645a3, 0xf74f6abddcc0f328),
        ("scaled", "hills1", -2147483646, -2147483642, 0x00000000, 0x3e841894, 0xca4971d84c32856b),
        ("scaled", "hills2", -2147483646, -2147483641, 0x80000000, 0x3e883958, 0x1606af56f55e9e04),
        ("scaled", "dales", -2147483646, -2147483642, 0x00000000, 0x3ecccccd, 0x604e1091230c1932),
        ("scaled", "plateau", -2147483646, -2147483637, 0x00000000, 0x3e46b1c5, 0xf91642675126b264),
        ("scaled", "badlands", -2147483646, -2147483640, 0x3ccccccd, 0x3f2f5c29, 0x491cba2cb4d2cf7b),
        ("scaled", "torridonian", -2147483646, -2147483637, 0x00000000, 0x3f000000, 0x75737b7e1da41139),
        ("halfdown", "steppe", 7, 12, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("halfdown", "plains", 7, 12, 0xbca3d70a, 0x3e051eb9, 0xff17844fedc4a4c5),
        ("halfdown", "hills1", 7, 11, 0x00000000, 0x3f19999a, 0xab8b7ca55088abf3),
        ("halfdown", "hills2", 7, 12, 0x80000000, 0x3f1e6667, 0x98426f20663ea361),
        ("halfdown", "dales", 7, 11, 0x00000000, 0x3ecccccd, 0x19776d644d6ce475),
        ("halfdown", "plateau", 7, 16, 0x00000000, 0x3ee70a3d, 0x5137dff7ca9633d1),
        ("halfdown", "badlands", 7, 13, 0x3ccccccd, 0x3f2f5c29, 0xc2c698b8d3b3c1c6),
        ("halfdown", "torridonian", 7, 16, 0x00000000, 0x3f000000, 0xd37518d98a20da8c),
        ("oddbig", "steppe", -7, -2, 0xbca3d70a, 0x3d75c28f, 0xba1362b74aa359d2),
        ("oddbig", "plains", -7, -2, 0xbca3d70a, 0x3e8f5c29, 0x3e41276435ef31ee),
        ("oddbig", "hills1", -7, -3, 0x00000000, 0x3f99999a, 0x37bfd8f3ba6756dc),
        ("oddbig", "hills2", -7, -2, 0x80000000, 0x3f9e6667, 0xfb010b70b002ab58),
        ("oddbig", "dales", -7, -3, 0x00000000, 0x3ecccccd, 0xe2a185f95f689a06),
        ("oddbig", "plateau", -7, 2, 0x00000000, 0x3f670a3d, 0x2096e5e43e29b5a0),
        ("oddbig", "badlands", -7, -1, 0x3ccccccd, 0x3f2f5c29, 0xb3dfe8f91eaed475),
        ("oddbig", "torridonian", -7, 2, 0x00000000, 0x3f000000, 0x9838ff39f4332d9b),
        ("tiny", "steppe", 0, 5, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("tiny", "plains", 0, 5, 0xbca3d70a, 0xbca3d70a, 0xff17844fedc4a4c5),
        ("tiny", "hills1", 0, 4, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "hills2", 0, 5, 0x80000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "dales", 0, 4, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("tiny", "plateau", 0, 9, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "badlands", 0, 6, 0x3ccccccd, 0x3f2f5c29, 0x070a4e95cc14f757),
        ("tiny", "torridonian", 0, 9, 0x00000000, 0x3f000000, 0x9948e7b9d8cc94d8),
        ("negative", "steppe", 99, 104, 0xbca3d70a, 0x3d75c28f, 0x40e88a51c53467e4),
        ("negative", "plains", 99, 104, 0xbca3d70a, 0xbe2e147b, 0x91a54a45a82a6b97),
        ("negative", "hills1", 99, 103, 0x80000000, 0xbf19999a, 0xe7b11483b620da4d),
        ("negative", "hills2", 99, 104, 0x00000000, 0xbf1e6667, 0x8022d3283697befa),
        ("negative", "dales", 99, 103, 0x00000000, 0x3ecccccd, 0x6de28f6cec456865),
        ("negative", "plateau", 99, 108, 0x80000000, 0xbee70a3d, 0xbf60888d70f32208),
        ("negative", "badlands", 99, 105, 0x3ccccccd, 0x3f2f5c29, 0x6f108dfdad037603),
        ("negative", "torridonian", 99, 108, 0x00000000, 0x3f000000, 0x664a42c83ac8a4e8),
        ("nan", "steppe", 5, 10, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("nan", "plains", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills1", 5, 9, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills2", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "dales", 5, 9, 0x00000000, 0x3ecccccd, 0x2fec3b0ba97bf943),
        ("nan", "plateau", 5, 14, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "badlands", 5, 11, 0x3ccccccd, 0x3f2f5c29, 0x50a181457deee7f2),
        ("nan", "torridonian", 5, 14, 0x00000000, 0x3f000000, 0x8f06b6b25438eafd),
        ("huge", "steppe", 3, 8, 0xbca3d70a, 0x3d75c28f, 0x0a645dffde6efa79),
        ("huge", "plains", 3, 8, 0xbca3d70a, 0x6ff2568d, 0xdada775cdb02f309),
        ("huge", "hills1", 3, 7, 0x00000000, 0x70f2568d, 0x8dc617ef4529563f),
        ("huge", "hills2", 3, 8, 0x80000000, 0x70f9e940, 0xc0ec623bf089d277),
        ("huge", "dales", 3, 7, 0x00000000, 0x3ecccccd, 0xb9e0aa41aac5ed80),
        ("huge", "plateau", 3, 12, 0x00000000, 0x70b64229, 0x71d0178734f3c051),
        ("huge", "badlands", 3, 9, 0x3ccccccd, 0x3f2f5c29, 0x64a0f1d8f83a796b),
        ("huge", "torridonian", 3, 12, 0x00000000, 0x3f000000, 0xd621c04127a5949a),
        ("unit60", "steppe", -2147483648, -2147483643, 0xbca3d70a, 0x3d75c28f, 0xb7887cb514dad7de),
        ("unit60", "plains", -2147483648, -2147483643, 0xbca3d70a, 0x3e6b851f, 0x38b71332a48a77ea),
        ("unit60", "hills1", -2147483648, -2147483644, 0x00000000, 0x3f800000, 0xd03b8139d46beb97),
        ("unit60", "hills2", -2147483648, -2147483643, 0x80000000, 0x3f840000, 0x1878f64e1a713a2a),
        ("unit60", "dales", -2147483648, -2147483644, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("unit60", "plateau", -2147483648, -2147483639, 0x00000000, 0x3f408888, 0x75fa862db29171ee),
        ("unit60", "badlands", -2147483648, -2147483642, 0x3ccccccd, 0x3f2f5c29, 0x070a4e95cc14f757),
        ("unit60", "torridonian", -2147483648, -2147483639, 0x00000000, 0x3f000000, 0x9948e7b9d8cc94d8),
        ("unit55", "steppe", 2147483644, -2147483647, 0xbca3d70a, 0x3d75c28f, 0xcc009386f623d713),
        ("unit55", "plains", 2147483644, -2147483647, 0xbca3d70a, 0x3e816578, 0x1705d1ee820fa929),
        ("unit55", "hills1", 2147483644, -2147483648, 0x00000000, 0x3f8ba2e9, 0xb4dd50a98533c91a),
        ("unit55", "hills2", 2147483644, -2147483647, 0x80000000, 0x3f900000, 0x2b6ebb23f4895006),
        ("unit55", "dales", 2147483644, -2147483648, 0x00000000, 0x3ecccccd, 0x6b93cdd6668b400a),
        ("unit55", "plateau", 2147483644, -2147483643, 0x00000000, 0x3f52094e, 0x598542eb91645ee4),
        ("unit55", "badlands", 2147483644, -2147483646, 0x3ccccccd, 0x3f2f5c29, 0xe60d9a525b68e9a3),
        ("unit55", "torridonian", 2147483644, -2147483643, 0x00000000, 0x3f000000, 0xa84557514b32b047),
        ("unit475", "steppe", 31, 36, 0xbca3d70a, 0x3d75c28f, 0x2176826111496d93),
        ("unit475", "plains", 31, 36, 0xbca3d70a, 0x3e9771b8, 0x7943eb3ef168994a),
        ("unit475", "hills1", 31, 35, 0x00000000, 0x3fa1af29, 0xe3196de0194cf29b),
        ("unit475", "hills2", 31, 36, 0x80000000, 0x3fa6bca3, 0xffbd209351742db8),
        ("unit475", "dales", 31, 35, 0x00000000, 0x3ecccccd, 0x655b959e2db3b8c4),
        ("unit475", "plateau", 31, 40, 0x00000000, 0x3f733333, 0x8dc1f86b2820d5d0),
        ("unit475", "badlands", 31, 37, 0x3ccccccd, 0x3f2f5c29, 0xbe4dba04e49fee68),
        ("unit475", "torridonian", 31, 40, 0x00000000, 0x3f000000, 0xa5887f134194ad1e),
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
            // 0.6 * v == 1 and 0.55 * v == 1: hills1's and hills2's `scale(1)` returns the module unscaled.
            "unit60" => (1.0, 1.0, f32::from_bits(0x3fd55555)),
            "unit55" => (1.0, 1.0, f32::from_bits(0x3fe8ba2e)),
            // (float) (0.475 * (double) v) == 1: plateau's `scale(1)` returns the module unscaled.
            "unit475" => (1.0, 1.0, f32::from_bits(0x4006bca2)),
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
            let forms = make_forms(oracle_settings(name));
            let mut seed = Seed::new(start);
            let module = match kind {
                "steppe" => forms.steppe(&mut seed),
                "plains" => forms.plains(&mut seed),
                "hills1" => forms.hills1(&mut seed),
                "hills2" => forms.hills2(&mut seed),
                "dales" => forms.dales(&mut seed),
                "plateau" => forms.plateau(&mut seed),
                "badlands" => forms.badlands(&mut seed),
                "torridonian" => forms.torridonian(&mut seed),
                _ => unreachable!("{kind}"),
            };
            // NaN settings make Scale multiply two NaNs; which NaN's sign comes out is up to the compiler (HotSpot's
            // interpreter and JIT differ, and so do Rust builds), so the oracle and this test hash those samples as
            // the canonical NaN after checking NaN-ness.
            let nan_times_nan = name == "nan" && matches!(kind, "plains" | "hills1" | "hills2");
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

    /// `LandFormsOracle`'s `deepOcean` rows: per `Levels` (height, sea level) and start seed, the `water` level, the
    /// bounds and the grid hash. `ModTerrains` keeps TerraForged's swapped `new Levels(63, 255)`.
    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const OCEAN_ORACLE: [(i32, i32, i32, u32, u32, u32, u64); 9] = [
        (256, 63, 100, 0x3e780000, 0x00000000, 0x3e5f3334, 0xb85445c555f5652f),
        (256, 63, 2147483642, 0x3e780000, 0x00000000, 0x3e5f3334, 0x5cdf2edc9464eb00),
        (256, 63, 2147483647, 0x3e780000, 0x00000000, 0x3e5f3334, 0xc16b397355e0ba7c),
        (480, 62, 9712416, 0x3e022222, 0x00000000, 0x3dea3d70, 0x4f7a67d26ada5729),
        (63, 255, -998877, 0x3f800000, 0x00000000, 0x3f666666, 0x04ab013fa3eae1f2),
        (320, 71, 2147483644, 0x3e600000, 0x00000000, 0x3e49999a, 0xbd3a753beeba2b6b),
        (256, 0, 42, 0xbb800000, 0x80000000, 0xbb59999a, 0xa53198f05149b731),
        (0, 1, 7, 0x00000000, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        (256, i32::MIN, -1, 0x3f800000, 0x00000000, 0x3f666666, 0x90428e57aa7a8b1c),
    ];

    /// The oracle's hash of a module's raw sample bits over `GRID` x `GRID`.
    fn grid_hash(module: &Module) -> u64 {
        let mut fnv = 0xcbf2_9ce4_8422_2325_u64;
        for x in GRID {
            for y in GRID {
                for byte in module.value(x, y).to_bits().to_le_bytes() {
                    fnv = (fnv ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        fnv
    }

    #[test]
    fn deep_ocean_matches_engine_0_3_0() {
        for (height, sea_level, start, water, min, max, hash) in OCEAN_ORACLE {
            let levels = Levels::new(height, sea_level);
            assert_eq!(levels.water.to_bits(), water, "{height} {sea_level}");
            let forms = LandForms::new(TerrainSettings::default(), levels, Module::constant(0.375));
            let module = forms.deep_ocean(start);
            let got = (
                module.min_value().to_bits(),
                module.max_value().to_bits(),
                grid_hash(&module),
            );
            assert_eq!(got, (min, max, hash), "{height} {sea_level} {start}");

            // `getOceanBase` is `Source.ZERO`; `getLandBase` is the ground module passed in.
            let ocean_base = forms.ocean_base();
            assert_eq!(ocean_base.min_value().to_bits(), 0);
            assert_eq!(ocean_base.max_value().to_bits(), 0);
            assert_eq!(grid_hash(&ocean_base), 0x2c1b93daafb34265);
            assert_eq!(bits(&forms.land_base()), [0x3ec00000; 4]);
        }
    }
}
