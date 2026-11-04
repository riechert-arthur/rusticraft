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
    terraforged::{
        CurveFunction, DistanceFunction, Domain, EdgeFunction, ErosionMode, ErosionRidge,
        Interpolation, Module, Source, SourceConfig, SourceKind,
    },
    terraforged_engine::{Levels, Seed, TerrainSettings},
};

/// Engine's `LandForms`: builds each landform's noise graph from the terrain settings, the world's `Levels` (only
/// `deepOcean` reads them, through their `water` level) and the `ground` module (`getLandBase`). Only `deepOcean`,
/// `steppe`, `plains`, `hills1`, `hills2`, `dales`, `mountains`, `mountains2`, `mountains3`, `plateau`, `badlands` and
/// `torridonian` and the two bases are ported so far.
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

    /// `LandForms.mountains(Seed)`: draws three seeds, then `makeFancy`'s two when fancy mountains are on; scales by
    /// `0.7 * globalVerticalScale`, computed in `f64` as Engine does.
    pub fn mountains(&self, seed: &mut Seed) -> Module {
        let scale_h = java_round(410.0 * self.settings.mountains.horizontal_scale);
        let module = Module::multiply(vec![
            source(SourceKind::Ridge, seed.next(), scale_h, 4, 1.15, 2.35),
            perlin(seed.next(), 24, 4, 2.0).alpha(Module::constant(0.075)),
        ])
        .warp(noise_warp(seed.next(), 350, 1, 150.0));
        self.make_fancy(seed, module).scale(Module::constant(
            (0.7_f64 * f64::from(self.terrain_vertical_scale)) as f32,
        ))
    }

    /// `LandForms.mountains2(Seed)`: draws four seeds, then `makeFancy`'s two when fancy mountains are on; scales by
    /// `0.645 * globalVerticalScale`, computed in `f64`. Ignores the mountains' horizontal scale, as Engine does.
    pub fn mountains2(&self, seed: &mut Seed) -> Module {
        let cell = cell_edge(seed.next(), 360)
            .scale(Module::constant(1.2))
            .clamp(Module::constant(0.0), Module::constant(1.0))
            .warp(noise_warp(seed.next(), 200, 2, 100.0));
        let blur = perlin(seed.next(), 10, 1, 2.0).alpha(Module::constant(0.025));
        let surface = source(SourceKind::Ridge, seed.next(), 125, 4, 0.975, 2.0)
            .alpha(Module::constant(0.37));
        let mountains = Module::multiply(vec![
            cell.clamp(Module::constant(0.0), Module::constant(1.0)),
            blur,
            surface,
        ])
        .power(Module::constant(1.1));
        self.make_fancy(seed, mountains).scale(Module::constant(
            (0.645_f64 * f64::from(self.terrain_vertical_scale)) as f32,
        ))
    }

    /// `LandForms.mountains3(Seed)`: `mountains2` with a cell size of 400 and an advanced terrace (two more seeds)
    /// before `makeFancy`.
    pub fn mountains3(&self, seed: &mut Seed) -> Module {
        let cell = cell_edge(seed.next(), 400)
            .scale(Module::constant(1.2))
            .clamp(Module::constant(0.0), Module::constant(1.0))
            .warp(noise_warp(seed.next(), 200, 2, 100.0));
        let blur = perlin(seed.next(), 10, 1, 2.0).alpha(Module::constant(0.025));
        let surface = source(SourceKind::Ridge, seed.next(), 125, 4, 0.975, 2.0)
            .alpha(Module::constant(0.37));
        let mountains = Module::multiply(vec![
            cell.clamp(Module::constant(0.0), Module::constant(1.0)),
            blur,
            surface,
        ])
        .power(Module::constant(1.1));
        let terraced = mountains.advanced_terrace(
            perlin(seed.next(), 50, 1, 2.0).scale(Module::constant(0.5)),
            perlin(seed.next(), 100, 1, 2.0)
                .clamp(Module::constant(0.5), Module::constant(0.95))
                .map(Module::constant(0.0), Module::constant(1.0)),
            Module::constant(0.45),
            0.2,
            0.45,
            24,
            1,
        );
        self.make_fancy(seed, terraced).scale(Module::constant(
            (0.645_f64 * f64::from(self.terrain_vertical_scale)) as f32,
        ))
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

    /// `LandForms.makeFancy`: with `fancyMountains` on, wraps the module in Engine's erosion `Ridge` (2 octaves,
    /// strength 0.65, grid 128, `CONSTANT`) warped along a Perlin direction; draws the warp's seed, then the ridge's.
    fn make_fancy(&self, seed: &mut Seed, module: Module) -> Module {
        if !self.settings.general.fancy_mountains {
            return module;
        }
        let warp = Domain::direction(perlin(seed.next(), 10, 1, 2.0), Module::constant(2.0));
        ErosionRidge::full(
            seed.next(),
            2,
            0.65,
            128.0,
            0.15,
            3.1,
            0.8,
            ErosionMode::Constant,
        )
        .wrap(module)
        .warp(warp)
    }
}

/// `Source.build(seed, scale, octaves).lacunarity(lacunarity).perlin()` (Engine's builder defaults otherwise).
pub fn perlin(seed: i32, scale: i32, octaves: usize, lacunarity: f32) -> Module {
    source(SourceKind::Perlin, seed, scale, octaves, 0.5, lacunarity)
}

/// `Source.build(seed, scale, octaves).gain(gain).lacunarity(lacunarity)` built as `kind`.
pub fn source(
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
pub fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        perlin(seed, scale, octaves, 2.0),
        perlin(seed.wrapping_add(1), scale, octaves, 2.0),
        Module::constant(strength),
    )
}

/// `Source.cellEdge(seed, scale, EdgeFunc.DISTANCE_2)`: the builder's defaults otherwise (displacement 1, Euclidean).
fn cell_edge(seed: i32, scale: i32) -> Module {
    Module::cell_edge_source(
        SourceConfig {
            seed,
            octaves: 1,
            gain: 0.5,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
        1.0,
        EdgeFunction::Distance2,
        DistanceFunction::Euclidean,
    )
}

/// Java's `Math.round(float)`: the floor of `value + 0.5` computed exactly (in `f64`, where adding 0.5 to an `f32`
/// never rounds), so 0.49999997 rounds to 0 and odd integers above 2^23 stay odd; NaN gives 0 and out-of-range
/// values saturate, as `as i32` does.
pub fn java_round(value: f32) -> i32 {
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
    /// Engine 0.3.0's `LandForms.steppe`/`plains`/`hills1`/`hills2`/`dales`/`plateau`/`badlands`/`torridonian`/
    /// `mountains`/`mountains2`/`mountains3` (`LandFormsOracle`, kept outside Git): per settings and start seed, the seed after building, the bounds and an
    /// FNV-1a hash of the raw bits of every sample on `GRID` x `GRID`.
    const GRID: [f32; 10] = [
        0.0, 12.25, -98.75, -4096.5, 8192.25, 123456.75, -0.125, -77777.5, 1e6, -3.0e5,
    ];

    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const ORACLE: [(&str, &str, i32, i32, u32, u32, u64); 231] = [
        ("default", "steppe", 12345, 12350, 0xbca3d70a, 0x3d75c28f, 0x639d1d83e3e0f2eb),
        ("default", "plains", 12345, 12350, 0xbca3d70a, 0x3e020c4b, 0x6e4af950d6b9b37f),
        ("default", "hills1", 12345, 12349, 0x00000000, 0x3f16872c, 0x6d5563d0ee663df0),
        ("default", "hills2", 12345, 12350, 0x80000000, 0x3f1b3b65, 0x346e5dcff0a3c6a2),
        ("default", "dales", 12345, 12349, 0x00000000, 0x3ecccccd, 0xcc6308d1c64458e5),
        ("default", "plateau", 12345, 12354, 0x00000000, 0x3ee26b50, 0x5acae80b70cf5ebf),
        ("default", "badlands", 12345, 12351, 0x3ccccccd, 0x3f2f5c29, 0xbd71d98b4e7bcdc6),
        ("default", "torridonian", 12345, 12354, 0x00000000, 0x3f000000, 0xd96420d2d809c767),
        ("default", "mountains", 12345, 12350, 0x00000000, 0x3f2f9db2, 0x1231ddfbbec6d490),
        ("default", "mountains2", 12345, 12351, 0x00000000, 0x3f21d14e, 0xe0e4fbc4d262f932),
        ("default", "mountains3", 12345, 12353, 0x00000000, 0x3f21d14e, 0xade3447720073eab),
        ("default", "steppe", -998877, -998872, 0xbca3d70a, 0x3d75c28f, 0x7a376023496a4361),
        ("default", "plains", -998877, -998872, 0xbca3d70a, 0x3e020c4b, 0x70dc912c2a72e694),
        ("default", "hills1", -998877, -998873, 0x00000000, 0x3f16872c, 0x511560374f860dea),
        ("default", "hills2", -998877, -998872, 0x80000000, 0x3f1b3b65, 0xc8d97f6e98f49a97),
        ("default", "dales", -998877, -998873, 0x00000000, 0x3ecccccd, 0xb3f6238244af33fb),
        ("default", "plateau", -998877, -998868, 0x00000000, 0x3ee26b50, 0x709f0400874ac2aa),
        ("default", "badlands", -998877, -998871, 0x3ccccccd, 0x3f2f5c29, 0x54b41cb31c853c2e),
        ("default", "torridonian", -998877, -998868, 0x00000000, 0x3f000000, 0x67f4f82dc0503ca9),
        ("default", "mountains", -998877, -998872, 0x00000000, 0x3f2f9db2, 0xea90a8feae759ace),
        ("default", "mountains2", -998877, -998871, 0x00000000, 0x3f21d14e, 0x4565e0329eb5d4f1),
        ("default", "mountains3", -998877, -998869, 0x00000000, 0x3f21d14e, 0x221c5c3e7f315913),
        ("default", "steppe", 2147483643, -2147483648, 0xbca3d70a, 0x3d75c28f, 0xffdf4562b5fae442),
        ("default", "plains", 2147483643, -2147483648, 0xbca3d70a, 0x3e020c4b, 0x5b03f6d91ab51dc4),
        ("default", "hills1", 2147483643, 2147483647, 0x00000000, 0x3f16872c, 0x5d9f3174bb93dab8),
        ("default", "hills2", 2147483643, -2147483648, 0x80000000, 0x3f1b3b65, 0x52177bc2f339df26),
        ("default", "dales", 2147483643, 2147483647, 0x00000000, 0x3ecccccd, 0x1bf3b706a93f5f84),
        ("default", "plateau", 2147483643, -2147483644, 0x00000000, 0x3ee26b50, 0x6c64d5bc5e9730c6),
        ("default", "badlands", 2147483643, -2147483647, 0x3ccccccd, 0x3f2f5c29, 0x78b3f2c8b2b756c8),
        ("default", "torridonian", 2147483643, -2147483644, 0x00000000, 0x3f000000, 0x4ca5f05a7898bf16),
        ("default", "mountains", 2147483643, -2147483648, 0x00000000, 0x3f2f9db2, 0x42a326412cf4ab68),
        ("default", "mountains2", 2147483643, -2147483647, 0x00000000, 0x3f21d14e, 0x7966445e9dfae5a2),
        ("default", "mountains3", 2147483643, -2147483645, 0x00000000, 0x3f21d14e, 0x47b3d6fea0850c68),
        ("default", "steppe", 2147483639, 2147483644, 0xbca3d70a, 0x3d75c28f, 0xe9fc128adbe3435e),
        ("default", "plains", 2147483639, 2147483644, 0xbca3d70a, 0x3e020c4b, 0xb7d09708de9ebe32),
        ("default", "hills1", 2147483639, 2147483643, 0x00000000, 0x3f16872c, 0x1265553acce2f97b),
        ("default", "hills2", 2147483639, 2147483644, 0x80000000, 0x3f1b3b65, 0xeac06bbf6c6fa4e4),
        ("default", "dales", 2147483639, 2147483643, 0x00000000, 0x3ecccccd, 0x964cbfdab477cce6),
        ("default", "plateau", 2147483639, -2147483648, 0x00000000, 0x3ee26b50, 0x4114d1db7d3f7a6c),
        ("default", "badlands", 2147483639, 2147483645, 0x3ccccccd, 0x3f2f5c29, 0x4d523ae07a646c72),
        ("default", "torridonian", 2147483639, -2147483648, 0x00000000, 0x3f000000, 0xeacecc7c2bcea0fe),
        ("default", "mountains", 2147483639, 2147483644, 0x00000000, 0x3f2f9db2, 0x3ca5453ee18978be),
        ("default", "mountains2", 2147483639, 2147483645, 0x00000000, 0x3f21d14e, 0x59c7dbab07e22709),
        ("default", "mountains3", 2147483639, 2147483647, 0x00000000, 0x3f21d14e, 0x20fa018f4487b4b5),
        ("modterrains", "steppe", 9712416, 9712421, 0xbca3d70a, 0x3d75c28f, 0x8f72e0ca682b02ca),
        ("modterrains", "plains", 9712416, 9712421, 0xbca3d70a, 0x3e051eb9, 0x8a73fd5503b12098),
        ("modterrains", "hills1", 9712416, 9712420, 0x00000000, 0x3f19999a, 0xe86ad3cd86529607),
        ("modterrains", "hills2", 9712416, 9712421, 0x80000000, 0x3f1e6667, 0x895f048c9154db49),
        ("modterrains", "dales", 9712416, 9712420, 0x00000000, 0x3ecccccd, 0x7f19c2df0ce3dbe4),
        ("modterrains", "plateau", 9712416, 9712425, 0x00000000, 0x3ee70a3d, 0x03abed4354d577c4),
        ("modterrains", "badlands", 9712416, 9712422, 0x3ccccccd, 0x3f2f5c29, 0x1894db7e94fc12eb),
        ("modterrains", "torridonian", 9712416, 9712425, 0x00000000, 0x3f000000, 0x7d587bc7a0c7908d),
        ("modterrains", "mountains", 9712416, 9712421, 0x00000000, 0x3f333333, 0x346ed28dd8181923),
        ("modterrains", "mountains2", 9712416, 9712422, 0x00000000, 0x3f251eb8, 0xcfed61f1ce0dd643),
        ("modterrains", "mountains3", 9712416, 9712424, 0x00000000, 0x3f251eb8, 0x7feeb28a8ffd4313),
        ("nofancy", "steppe", 9712416, 9712421, 0xbca3d70a, 0x3d75c28f, 0x8f72e0ca682b02ca),
        ("nofancy", "plains", 9712416, 9712421, 0xbca3d70a, 0x3e051eb9, 0x8a73fd5503b12098),
        ("nofancy", "hills1", 9712416, 9712420, 0x00000000, 0x3f19999a, 0xe86ad3cd86529607),
        ("nofancy", "hills2", 9712416, 9712421, 0x80000000, 0x3f1e6667, 0x895f048c9154db49),
        ("nofancy", "dales", 9712416, 9712420, 0x00000000, 0x3ecccccd, 0x7f19c2df0ce3dbe4),
        ("nofancy", "plateau", 9712416, 9712425, 0x00000000, 0x3ee70a3d, 0x03abed4354d577c4),
        ("nofancy", "badlands", 9712416, 9712422, 0x3ccccccd, 0x3f2f5c29, 0x1894db7e94fc12eb),
        ("nofancy", "torridonian", 9712416, 9712425, 0x00000000, 0x3f000000, 0x7d587bc7a0c7908d),
        ("nofancy", "mountains", 9712416, 9712419, 0x00000000, 0x3f333333, 0xc6c3f4e148459ca0),
        ("nofancy", "mountains2", 9712416, 9712420, 0x00000000, 0x3f251eb8, 0x9d73a0c498f10c07),
        ("nofancy", "mountains3", 9712416, 9712422, 0x00000000, 0x3f251eb8, 0x5159385ddcc0f843),
        ("nofancy", "steppe", 2147483644, -2147483647, 0xbca3d70a, 0x3d75c28f, 0xcc009386f623d713),
        ("nofancy", "plains", 2147483644, -2147483647, 0xbca3d70a, 0x3e051eb9, 0xb9f07193643edd1a),
        ("nofancy", "hills1", 2147483644, -2147483648, 0x00000000, 0x3f19999a, 0x4ae9210172a3ae01),
        ("nofancy", "hills2", 2147483644, -2147483647, 0x80000000, 0x3f1e6667, 0x8552ee4705b9ac65),
        ("nofancy", "dales", 2147483644, -2147483648, 0x00000000, 0x3ecccccd, 0x6b93cdd6668b400a),
        ("nofancy", "plateau", 2147483644, -2147483643, 0x00000000, 0x3ee70a3d, 0x1b8b252b33aa127a),
        ("nofancy", "badlands", 2147483644, -2147483646, 0x3ccccccd, 0x3f2f5c29, 0xe60d9a525b68e9a3),
        ("nofancy", "torridonian", 2147483644, -2147483643, 0x00000000, 0x3f000000, 0xa84557514b32b047),
        ("nofancy", "mountains", 2147483644, 2147483647, 0x00000000, 0x3f333333, 0xdb8f1e07d8cbc809),
        ("nofancy", "mountains2", 2147483644, -2147483648, 0x00000000, 0x3f251eb8, 0xc06645d75adea145),
        ("nofancy", "mountains3", 2147483644, -2147483646, 0x00000000, 0x3f251eb8, 0x0750236438934a11),
        ("scaled", "steppe", 2147483645, -2147483646, 0xbca3d70a, 0x3d75c28f, 0xb918d2033648e902),
        ("scaled", "plains", 2147483645, -2147483646, 0xbca3d70a, 0x3d3645a3, 0xc38b849bb3dd7d69),
        ("scaled", "hills1", 2147483645, -2147483647, 0x00000000, 0x3e841894, 0xdde8b6352082ee67),
        ("scaled", "hills2", 2147483645, -2147483646, 0x80000000, 0x3e883958, 0xf830272ce50e01f5),
        ("scaled", "dales", 2147483645, -2147483647, 0x00000000, 0x3ecccccd, 0xf3734c7bd6de92ce),
        ("scaled", "plateau", 2147483645, -2147483642, 0x00000000, 0x3e46b1c5, 0xa84838c2a2c6af31),
        ("scaled", "badlands", 2147483645, -2147483645, 0x3ccccccd, 0x3f2f5c29, 0x05aa29a6c2b389bd),
        ("scaled", "torridonian", 2147483645, -2147483642, 0x00000000, 0x3f000000, 0xeb48ac1a78d73cd8),
        ("scaled", "mountains", 2147483645, -2147483646, 0x00000000, 0x3e9a1cac, 0x051613384b354338),
        ("scaled", "mountains2", 2147483645, -2147483645, 0x00000000, 0x3e8e00d2, 0x38cb64effe0f011a),
        ("scaled", "mountains3", 2147483645, -2147483643, 0x00000000, 0x3e8e00d2, 0xc15d6d8c44c864da),
        ("scaled", "steppe", -2147483646, -2147483641, 0xbca3d70a, 0x3d75c28f, 0xd66522f75e6e4e3c),
        ("scaled", "plains", -2147483646, -2147483641, 0xbca3d70a, 0x3d3645a3, 0xf74f6abddcc0f328),
        ("scaled", "hills1", -2147483646, -2147483642, 0x00000000, 0x3e841894, 0xca4971d84c32856b),
        ("scaled", "hills2", -2147483646, -2147483641, 0x80000000, 0x3e883958, 0x1606af56f55e9e04),
        ("scaled", "dales", -2147483646, -2147483642, 0x00000000, 0x3ecccccd, 0x604e1091230c1932),
        ("scaled", "plateau", -2147483646, -2147483637, 0x00000000, 0x3e46b1c5, 0xf91642675126b264),
        ("scaled", "badlands", -2147483646, -2147483640, 0x3ccccccd, 0x3f2f5c29, 0x491cba2cb4d2cf7b),
        ("scaled", "torridonian", -2147483646, -2147483637, 0x00000000, 0x3f000000, 0x75737b7e1da41139),
        ("scaled", "mountains", -2147483646, -2147483641, 0x00000000, 0x3e9a1cac, 0x29491f10ad1e352e),
        ("scaled", "mountains2", -2147483646, -2147483640, 0x00000000, 0x3e8e00d2, 0x0bc7f07791cd0820),
        ("scaled", "mountains3", -2147483646, -2147483638, 0x00000000, 0x3e8e00d2, 0x60b74c83c204e4dd),
        ("nofancyscaled", "steppe", -2147483645, -2147483640, 0xbca3d70a, 0x3d75c28f, 0xcaab19e75fed4d59),
        ("nofancyscaled", "plains", -2147483645, -2147483640, 0xbca3d70a, 0x3d116874, 0xf02f9ae165d2bc83),
        ("nofancyscaled", "hills1", -2147483645, -2147483641, 0x00000000, 0x3e6353f9, 0x21c3ea3ced95672b),
        ("nofancyscaled", "hills2", -2147483645, -2147483640, 0x80000000, 0x3e6a6e98, 0x8c650f5747219275),
        ("nofancyscaled", "dales", -2147483645, -2147483641, 0x00000000, 0x3ecccccd, 0xb9e0aa41aac5ed80),
        ("nofancyscaled", "plateau", -2147483645, -2147483636, 0x00000000, 0x3e2af838, 0x143b217e32496d66),
        ("nofancyscaled", "badlands", -2147483645, -2147483639, 0x3ccccccd, 0x3f2f5c29, 0x64a0f1d8f83a796b),
        ("nofancyscaled", "torridonian", -2147483645, -2147483636, 0x00000000, 0x3f000000, 0xd621c04127a5949a),
        ("nofancyscaled", "mountains", -2147483645, -2147483642, 0x00000000, 0x3e849ba6, 0xa5e5901edc29adc2),
        ("nofancyscaled", "mountains2", -2147483645, -2147483641, 0x00000000, 0x3e7460ab, 0x2032d4d210b011ce),
        ("nofancyscaled", "mountains3", -2147483645, -2147483639, 0x00000000, 0x3e7460ab, 0x1eb32c1c5e178cbe),
        ("halfdown", "steppe", 7, 12, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("halfdown", "plains", 7, 12, 0xbca3d70a, 0x3e051eb9, 0xff17844fedc4a4c5),
        ("halfdown", "hills1", 7, 11, 0x00000000, 0x3f19999a, 0xab8b7ca55088abf3),
        ("halfdown", "hills2", 7, 12, 0x80000000, 0x3f1e6667, 0x98426f20663ea361),
        ("halfdown", "dales", 7, 11, 0x00000000, 0x3ecccccd, 0x19776d644d6ce475),
        ("halfdown", "plateau", 7, 16, 0x00000000, 0x3ee70a3d, 0x5137dff7ca9633d1),
        ("halfdown", "badlands", 7, 13, 0x3ccccccd, 0x3f2f5c29, 0xc2c698b8d3b3c1c6),
        ("halfdown", "torridonian", 7, 16, 0x00000000, 0x3f000000, 0xd37518d98a20da8c),
        ("halfdown", "mountains", 7, 12, 0x00000000, 0x3f333333, 0x12956ce69e90b3c5),
        ("halfdown", "mountains2", 7, 13, 0x00000000, 0x3f251eb8, 0x702ee980baffe3ad),
        ("halfdown", "mountains3", 7, 15, 0x00000000, 0x3f251eb8, 0xc87331488ef2c876),
        ("oddbig", "steppe", -7, -2, 0xbca3d70a, 0x3d75c28f, 0xba1362b74aa359d2),
        ("oddbig", "plains", -7, -2, 0xbca3d70a, 0x3e8f5c29, 0x3e41276435ef31ee),
        ("oddbig", "hills1", -7, -3, 0x00000000, 0x3f99999a, 0x37bfd8f3ba6756dc),
        ("oddbig", "hills2", -7, -2, 0x80000000, 0x3f9e6667, 0xfb010b70b002ab58),
        ("oddbig", "dales", -7, -3, 0x00000000, 0x3ecccccd, 0xe2a185f95f689a06),
        ("oddbig", "plateau", -7, 2, 0x00000000, 0x3f670a3d, 0x2096e5e43e29b5a0),
        ("oddbig", "badlands", -7, -1, 0x3ccccccd, 0x3f2f5c29, 0xb3dfe8f91eaed475),
        ("oddbig", "torridonian", -7, 2, 0x00000000, 0x3f000000, 0x9838ff39f4332d9b),
        ("oddbig", "mountains", -7, -2, 0x00000000, 0x3fb33333, 0x0d3b5749265234e8),
        ("oddbig", "mountains2", -7, -1, 0x00000000, 0x3fa51eb8, 0x0294d2ad7c2b45d8),
        ("oddbig", "mountains3", -7, 1, 0x00000000, 0x3fa51eb8, 0xf4e29191327f229e),
        ("tiny", "steppe", 0, 5, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("tiny", "plains", 0, 5, 0xbca3d70a, 0xbca3d70a, 0xff17844fedc4a4c5),
        ("tiny", "hills1", 0, 4, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "hills2", 0, 5, 0x80000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "dales", 0, 4, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("tiny", "plateau", 0, 9, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "badlands", 0, 6, 0x3ccccccd, 0x3f2f5c29, 0x070a4e95cc14f757),
        ("tiny", "torridonian", 0, 9, 0x00000000, 0x3f000000, 0x9948e7b9d8cc94d8),
        ("tiny", "mountains", 0, 5, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "mountains2", 0, 6, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "mountains3", 0, 8, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("negative", "steppe", 99, 104, 0xbca3d70a, 0x3d75c28f, 0x40e88a51c53467e4),
        ("negative", "plains", 99, 104, 0xbca3d70a, 0xbe2e147b, 0x91a54a45a82a6b97),
        ("negative", "hills1", 99, 103, 0x80000000, 0xbf19999a, 0xe7b11483b620da4d),
        ("negative", "hills2", 99, 104, 0x00000000, 0xbf1e6667, 0x8022d3283697befa),
        ("negative", "dales", 99, 103, 0x00000000, 0x3ecccccd, 0x6de28f6cec456865),
        ("negative", "plateau", 99, 108, 0x80000000, 0xbee70a3d, 0xbf60888d70f32208),
        ("negative", "badlands", 99, 105, 0x3ccccccd, 0x3f2f5c29, 0x6f108dfdad037603),
        ("negative", "torridonian", 99, 108, 0x00000000, 0x3f000000, 0x664a42c83ac8a4e8),
        ("negative", "mountains", 99, 104, 0x80000000, 0xbf333333, 0x48df6473bbaaef6b),
        ("negative", "mountains2", 99, 105, 0x80000000, 0xbf251eb8, 0xf64b3be08773cd92),
        ("negative", "mountains3", 99, 107, 0x80000000, 0xbf251eb8, 0xba0ac7c0266e2a6a),
        ("nan", "steppe", 5, 10, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("nan", "plains", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills1", 5, 9, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills2", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "dales", 5, 9, 0x00000000, 0x3ecccccd, 0x2fec3b0ba97bf943),
        ("nan", "plateau", 5, 14, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "badlands", 5, 11, 0x3ccccccd, 0x3f2f5c29, 0x50a181457deee7f2),
        ("nan", "torridonian", 5, 14, 0x00000000, 0x3f000000, 0x8f06b6b25438eafd),
        ("nan", "mountains", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "mountains2", 5, 11, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "mountains3", 5, 13, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("huge", "steppe", 3, 8, 0xbca3d70a, 0x3d75c28f, 0x0a645dffde6efa79),
        ("huge", "plains", 3, 8, 0xbca3d70a, 0x6ff2568d, 0xdada775cdb02f309),
        ("huge", "hills1", 3, 7, 0x00000000, 0x70f2568d, 0x8dc617ef4529563f),
        ("huge", "hills2", 3, 8, 0x80000000, 0x70f9e940, 0xc0ec623bf089d277),
        ("huge", "dales", 3, 7, 0x00000000, 0x3ecccccd, 0xb9e0aa41aac5ed80),
        ("huge", "plateau", 3, 12, 0x00000000, 0x70b64229, 0x71d0178734f3c051),
        ("huge", "badlands", 3, 9, 0x3ccccccd, 0x3f2f5c29, 0x64a0f1d8f83a796b),
        ("huge", "torridonian", 3, 12, 0x00000000, 0x3f000000, 0xd621c04127a5949a),
        ("huge", "mountains", 3, 8, 0x00000000, 0x710d5d27, 0x15f710201b288a15),
        ("huge", "mountains2", 3, 9, 0x00000000, 0x710241b8, 0x3bcaafe6e165c459),
        ("huge", "mountains3", 3, 11, 0x00000000, 0x710241b8, 0x5c290f2e4e443e8d),
        ("unit60", "steppe", -2147483648, -2147483643, 0xbca3d70a, 0x3d75c28f, 0xb7887cb514dad7de),
        ("unit60", "plains", -2147483648, -2147483643, 0xbca3d70a, 0x3e6b851f, 0x38b71332a48a77ea),
        ("unit60", "hills1", -2147483648, -2147483644, 0x00000000, 0x3f800000, 0xd03b8139d46beb97),
        ("unit60", "hills2", -2147483648, -2147483643, 0x80000000, 0x3f840000, 0x1878f64e1a713a2a),
        ("unit60", "dales", -2147483648, -2147483644, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("unit60", "plateau", -2147483648, -2147483639, 0x00000000, 0x3f408888, 0x75fa862db29171ee),
        ("unit60", "badlands", -2147483648, -2147483642, 0x3ccccccd, 0x3f2f5c29, 0x070a4e95cc14f757),
        ("unit60", "torridonian", -2147483648, -2147483639, 0x00000000, 0x3f000000, 0x9948e7b9d8cc94d8),
        ("unit60", "mountains", -2147483648, -2147483643, 0x00000000, 0x3f955555, 0xd2f105709b550049),
        ("unit60", "mountains2", -2147483648, -2147483642, 0x00000000, 0x3f899999, 0xce13c446e1ac9067),
        ("unit60", "mountains3", -2147483648, -2147483640, 0x00000000, 0x3f899999, 0x94ddfd9a5aff4ffa),
        ("unit55", "steppe", 2147483644, -2147483647, 0xbca3d70a, 0x3d75c28f, 0xcc009386f623d713),
        ("unit55", "plains", 2147483644, -2147483647, 0xbca3d70a, 0x3e816578, 0x1705d1ee820fa929),
        ("unit55", "hills1", 2147483644, -2147483648, 0x00000000, 0x3f8ba2e9, 0xb4dd50a98533c91a),
        ("unit55", "hills2", 2147483644, -2147483647, 0x80000000, 0x3f900000, 0x2b6ebb23f4895006),
        ("unit55", "dales", 2147483644, -2147483648, 0x00000000, 0x3ecccccd, 0x6b93cdd6668b400a),
        ("unit55", "plateau", 2147483644, -2147483643, 0x00000000, 0x3f52094e, 0x598542eb91645ee4),
        ("unit55", "badlands", 2147483644, -2147483646, 0x3ccccccd, 0x3f2f5c29, 0xe60d9a525b68e9a3),
        ("unit55", "torridonian", 2147483644, -2147483643, 0x00000000, 0x3f000000, 0xa84557514b32b047),
        ("unit55", "mountains", 2147483644, -2147483647, 0x00000000, 0x3fa2e8ba, 0x798ecedc86b79e99),
        ("unit55", "mountains2", 2147483644, -2147483646, 0x00000000, 0x3f961bed, 0xfb0f29cfd3b22ea1),
        ("unit55", "mountains3", 2147483644, -2147483644, 0x00000000, 0x3f961bed, 0xa3a11e4832262137),
        ("unit475", "steppe", 31, 36, 0xbca3d70a, 0x3d75c28f, 0x2176826111496d93),
        ("unit475", "plains", 31, 36, 0xbca3d70a, 0x3e9771b8, 0x7943eb3ef168994a),
        ("unit475", "hills1", 31, 35, 0x00000000, 0x3fa1af29, 0xe3196de0194cf29b),
        ("unit475", "hills2", 31, 36, 0x80000000, 0x3fa6bca3, 0xffbd209351742db8),
        ("unit475", "dales", 31, 35, 0x00000000, 0x3ecccccd, 0x655b959e2db3b8c4),
        ("unit475", "plateau", 31, 40, 0x00000000, 0x3f733333, 0x8dc1f86b2820d5d0),
        ("unit475", "badlands", 31, 37, 0x3ccccccd, 0x3f2f5c29, 0xbe4dba04e49fee68),
        ("unit475", "torridonian", 31, 40, 0x00000000, 0x3f000000, 0xa5887f134194ad1e),
        ("unit475", "mountains", 31, 36, 0x00000000, 0x3fbca1b0, 0xf29900c65e110afc),
        ("unit475", "mountains2", 31, 37, 0x00000000, 0x3fadcf7f, 0xde0e3080e660f91b),
        ("unit475", "mountains3", 31, 39, 0x00000000, 0x3fadcf7f, 0xfbaab20d2967f912),
        ("unit70", "steppe", 1234, 1239, 0xbca3d70a, 0x3d75c28f, 0x2e2ca23666d6a6bd),
        ("unit70", "plains", 1234, 1239, 0xbca3d70a, 0x3e46f2d7, 0xb724ba55174751f4),
        ("unit70", "hills1", 1234, 1238, 0x00000000, 0x3f5b6db8, 0x28584084a2d5b13c),
        ("unit70", "hills2", 1234, 1239, 0x80000000, 0x3f624925, 0x9d786fee78fc3ba8),
        ("unit70", "dales", 1234, 1238, 0x00000000, 0x3ecccccd, 0x655ba59fb51041d3),
        ("unit70", "plateau", 1234, 1243, 0x00000000, 0x3f250751, 0x4d63a2ae47875cb6),
        ("unit70", "badlands", 1234, 1240, 0x3ccccccd, 0x3f2f5c29, 0x46fb36cc5edf9eda),
        ("unit70", "torridonian", 1234, 1243, 0x00000000, 0x3f000000, 0x81a5c800b4d88456),
        ("unit70", "mountains", 1234, 1239, 0x00000000, 0x3f800000, 0x6ca894cd6c552f9c),
        ("unit70", "mountains2", 1234, 1240, 0x00000000, 0x3f6be2bf, 0x0734f1fa5e0225fc),
        ("unit70", "mountains3", 1234, 1242, 0x00000000, 0x3f6be2bf, 0x2596122f74ee4d45),
        ("unit645", "steppe", -1234, -1229, 0xbca3d70a, 0x3d75c28f, 0xb5105ecdb6055b8d),
        ("unit645", "plains", -1234, -1229, 0xbca3d70a, 0x3e59a8d8, 0x5e19933df9ffc68e),
        ("unit645", "hills1", -1234, -1230, 0x00000000, 0x3f6e23b9, 0x6dc4fd7fc0370462),
        ("unit645", "hills2", -1234, -1229, 0x80000000, 0x3f7594d7, 0xeedb290680578f56),
        ("unit645", "dales", -1234, -1230, 0x00000000, 0x3ecccccd, 0xac4b67079403f8dc),
        ("unit645", "plateau", -1234, -1225, 0x00000000, 0x3f3319cc, 0xf32dea7af1c5cfe0),
        ("unit645", "badlands", -1234, -1228, 0x3ccccccd, 0x3f2f5c29, 0x18f93ec072567586),
        ("unit645", "torridonian", -1234, -1225, 0x00000000, 0x3f000000, 0x51a504bd0693c1ad),
        ("unit645", "mountains", -1234, -1231, 0x00000000, 0x3f8aea2c, 0x8fca740e97db5a0c),
        ("unit645", "mountains2", -1234, -1230, 0x00000000, 0x3f800000, 0xb03290d1a5236b30),
        ("unit645", "mountains3", -1234, -1228, 0x00000000, 0x3f800000, 0xd03c02569c0eb140),
    ];

    fn oracle_settings(name: &str) -> TerrainSettings {
        // (steppe, plains, global vertical, mountains) scales and fancy mountains; the mountains' horizontal scale
        // is the plains' unless given.
        let (steppe, plains, vertical, mountains, fancy) = match name {
            "default" => return TerrainSettings::default(),
            "modterrains" => (1.0, 1.0, 1.0, 1.0, true),
            // ModTerrains' `nonFancy()` settings, used for `mountains_ridge_1` and `mountains_ridge_2`.
            "nofancy" => (1.0, 1.0, 1.0, 1.0, false),
            "scaled" => (0.25, 1.75, 0.43, 1.75, true),
            "nofancyscaled" => (0.25, 1.75, 0.37, 1.75, false),
            // 250 * h == 0.49999997 and 410 * m == 0.49999994: Math.round gives 0, so the frequency is infinite.
            "halfdown" => (
                f32::from_bits(0x3b03126e),
                f32::from_bits(0x3b03126e),
                1.0,
                f32::from_bits(0x3a9fd809),
                true,
            ),
            // 250 * h == 8388609 and 410 * m == 8388609: Math.round keeps the odd integer.
            "oddbig" => (
                f32::from_bits(0x47031270),
                f32::from_bits(0x47031270),
                2.0,
                f32::from_bits(0x469fd80b),
                true,
            ),
            "tiny" => (0.006, 0.01, 0.0, 0.006, true),
            "negative" => (-1.3, -0.5, -1.0, -1.3, true),
            "nan" => (f32::NAN, f32::NAN, f32::NAN, f32::NAN, true),
            "huge" => (1e30, 1e30, 1e30, 1e30, true),
            // 0.6 * v == 1 and 0.55 * v == 1: hills1's and hills2's `scale(1)` returns the module unscaled.
            "unit60" => (1.0, 1.0, f32::from_bits(0x3fd55555), 1.0, true),
            "unit55" => (1.0, 1.0, f32::from_bits(0x3fe8ba2e), 1.0, true),
            // (float) (0.475 * (double) v) == 1: plateau's `scale(1)` returns the module unscaled.
            "unit475" => (1.0, 1.0, f32::from_bits(0x4006bca2), 1.0, true),
            // (float) (0.7 * (double) v) == 1 and (float) (0.645 * (double) v) == 1: the mountains' `scale(1)`.
            "unit70" => (1.0, 1.0, f32::from_bits(0x3fb6db6e), 1.0, true),
            "unit645" => (1.0, 1.0, f32::from_bits(0x3fc6731a), 1.0, false),
            _ => unreachable!("{name}"),
        };
        let mut settings = TerrainSettings::default();
        settings.steppe.horizontal_scale = steppe;
        settings.plains.horizontal_scale = plains;
        settings.mountains.horizontal_scale = mountains;
        settings.general.global_vertical_scale = vertical;
        settings.general.fancy_mountains = fancy;
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
                "mountains" => forms.mountains(&mut seed),
                "mountains2" => forms.mountains2(&mut seed),
                "mountains3" => forms.mountains3(&mut seed),
                _ => unreachable!("{kind}"),
            };
            // NaN settings make Scale multiply two NaNs; which NaN's sign comes out is up to the compiler (HotSpot's
            // interpreter and JIT differ, and so do Rust builds), so the oracle and this test hash those samples as
            // the canonical NaN after checking NaN-ness.
            let nan_times_nan = name == "nan"
                && matches!(
                    kind,
                    "plains" | "hills1" | "hills2" | "mountains" | "mountains2" | "mountains3"
                );
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
    fn mountains_match_engine_0_3_0() {
        let forms = make_forms(TerrainSettings::default());

        let mut seed = Seed::new(321);
        let mountains = forms.mountains(&mut seed);
        assert_eq!(seed.get(), 326);
        assert_eq!(mountains.min_value().to_bits(), 0x00000000);
        assert_eq!(mountains.max_value().to_bits(), 0x3f2f9db2);
        assert_eq!(
            bits(&mountains),
            [0x3e6964f0, 0x3e853d24, 0x3ea800d6, 0x3e46c645]
        );

        let mut seed = Seed::new(-45);
        let mountains = forms.mountains2(&mut seed);
        assert_eq!(seed.get(), -39);
        assert_eq!(mountains.min_value().to_bits(), 0x00000000);
        assert_eq!(mountains.max_value().to_bits(), 0x3f21d14e);
        assert_eq!(
            bits(&mountains),
            [0x3e3a452e, 0x3d8b619e, 0x3e2cfdbd, 0x3dab43ab]
        );

        let mut seed = Seed::new(987_654);
        let mountains = forms.mountains3(&mut seed);
        assert_eq!(seed.get(), 987_662);
        assert_eq!(mountains.min_value().to_bits(), 0x00000000);
        assert_eq!(mountains.max_value().to_bits(), 0x3f21d14e);
        assert_eq!(
            bits(&mountains),
            [0x3e3f5c77, 0x3e436228, 0x3e0b4962, 0x3dc3cf5c]
        );
    }

    #[test]
    fn plain_scaled_mountains_match_engine_0_3_0() {
        let mut settings = TerrainSettings::default();
        settings.general.fancy_mountains = false;
        settings.general.global_vertical_scale = 0.37;
        settings.mountains.horizontal_scale = 1.75;
        let forms = make_forms(settings);

        let mut seed = Seed::new(i32::MAX - 3);
        let mountains = forms.mountains(&mut seed);
        assert_eq!(seed.get(), i32::MAX);
        assert_eq!(mountains.max_value().to_bits(), 0x3e849ba6);
        assert_eq!(
            bits(&mountains),
            [0x3e7f453a, 0x3e21a878, 0x3debb605, 0x3e60fb20]
        );

        let mut seed = Seed::new(i32::MIN + 3);
        let mountains = forms.mountains2(&mut seed);
        assert_eq!(seed.get(), i32::MIN + 7);
        assert_eq!(mountains.max_value().to_bits(), 0x3e7460ab);
        assert_eq!(
            bits(&mountains),
            [0x3bdf5fa5, 0x3bf07865, 0x3d3f7983, 0x3d98132b]
        );

        let mut seed = Seed::new(13_579);
        let mountains = forms.mountains3(&mut seed);
        assert_eq!(seed.get(), 13_585);
        assert_eq!(mountains.max_value().to_bits(), 0x3e7460ab);
        assert_eq!(
            bits(&mountains),
            [0x3e0a8674, 0x3dc8c31d, 0x3cf7f1e6, 0x3d08c80f]
        );
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
