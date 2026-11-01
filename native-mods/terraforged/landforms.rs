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

/// Engine's `LandForms`: builds each landform's noise graph from the terrain settings. Only `steppe`, `plains`,
/// `hills1`, `hills2` and `dales` are ported so far, so Engine's `levels` and `ground` constructor arguments are not
/// taken yet.
pub struct LandForms {
    settings: TerrainSettings,
}

impl LandForms {
    pub const fn new(settings: TerrainSettings) -> Self {
        Self { settings }
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
            .scale(Module::constant(
                0.15 * self.settings.general.global_vertical_scale,
            ))
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
            .scale(Module::constant(
                0.6 * self.settings.general.global_vertical_scale,
            ))
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
        .scale(Module::constant(
            0.55 * self.settings.general.global_vertical_scale,
        ))
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
            (0.475_f64 * f64::from(self.settings.general.global_vertical_scale)) as f32,
        ))
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
    /// Engine 0.3.0's `LandForms.steppe`/`plains`/`hills1`/`hills2`/`dales` (`LandFormsOracle`, kept outside Git):
    /// per settings and start seed, the seed after building, the bounds and an FNV-1a hash of the raw bits of every
    /// sample on `GRID` x `GRID`.
    const GRID: [f32; 10] = [
        0.0, 12.25, -98.75, -4096.5, 8192.25, 123456.75, -0.125, -77777.5, 1e6, -3.0e5,
    ];

    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const ORACLE: [(&str, &str, i32, i32, u32, u32, u64); 70] = [
        ("default", "steppe", 12345, 12350, 0xbca3d70a, 0x3d75c28f, 0x639d1d83e3e0f2eb),
        ("default", "plains", 12345, 12350, 0xbca3d70a, 0x3e020c4b, 0x6e4af950d6b9b37f),
        ("default", "hills1", 12345, 12349, 0x00000000, 0x3f16872c, 0x6d5563d0ee663df0),
        ("default", "hills2", 12345, 12350, 0x80000000, 0x3f1b3b65, 0x346e5dcff0a3c6a2),
        ("default", "dales", 12345, 12349, 0x00000000, 0x3ecccccd, 0xcc6308d1c64458e5),
        ("default", "steppe", -998877, -998872, 0xbca3d70a, 0x3d75c28f, 0x7a376023496a4361),
        ("default", "plains", -998877, -998872, 0xbca3d70a, 0x3e020c4b, 0x70dc912c2a72e694),
        ("default", "hills1", -998877, -998873, 0x00000000, 0x3f16872c, 0x511560374f860dea),
        ("default", "hills2", -998877, -998872, 0x80000000, 0x3f1b3b65, 0xc8d97f6e98f49a97),
        ("default", "dales", -998877, -998873, 0x00000000, 0x3ecccccd, 0xb3f6238244af33fb),
        ("default", "steppe", 2147483643, -2147483648, 0xbca3d70a, 0x3d75c28f, 0xffdf4562b5fae442),
        ("default", "plains", 2147483643, -2147483648, 0xbca3d70a, 0x3e020c4b, 0x5b03f6d91ab51dc4),
        ("default", "hills1", 2147483643, 2147483647, 0x00000000, 0x3f16872c, 0x5d9f3174bb93dab8),
        ("default", "hills2", 2147483643, -2147483648, 0x80000000, 0x3f1b3b65, 0x52177bc2f339df26),
        ("default", "dales", 2147483643, 2147483647, 0x00000000, 0x3ecccccd, 0x1bf3b706a93f5f84),
        ("modterrains", "steppe", 9712416, 9712421, 0xbca3d70a, 0x3d75c28f, 0x8f72e0ca682b02ca),
        ("modterrains", "plains", 9712416, 9712421, 0xbca3d70a, 0x3e051eb9, 0x8a73fd5503b12098),
        ("modterrains", "hills1", 9712416, 9712420, 0x00000000, 0x3f19999a, 0xe86ad3cd86529607),
        ("modterrains", "hills2", 9712416, 9712421, 0x80000000, 0x3f1e6667, 0x895f048c9154db49),
        ("modterrains", "dales", 9712416, 9712420, 0x00000000, 0x3ecccccd, 0x7f19c2df0ce3dbe4),
        ("scaled", "steppe", 2147483645, -2147483646, 0xbca3d70a, 0x3d75c28f, 0xb918d2033648e902),
        ("scaled", "plains", 2147483645, -2147483646, 0xbca3d70a, 0x3d3645a3, 0xc38b849bb3dd7d69),
        ("scaled", "hills1", 2147483645, -2147483647, 0x00000000, 0x3e841894, 0xdde8b6352082ee67),
        ("scaled", "hills2", 2147483645, -2147483646, 0x80000000, 0x3e883958, 0xf830272ce50e01f5),
        ("scaled", "dales", 2147483645, -2147483647, 0x00000000, 0x3ecccccd, 0xf3734c7bd6de92ce),
        ("scaled", "steppe", -2147483646, -2147483641, 0xbca3d70a, 0x3d75c28f, 0xd66522f75e6e4e3c),
        ("scaled", "plains", -2147483646, -2147483641, 0xbca3d70a, 0x3d3645a3, 0xf74f6abddcc0f328),
        ("scaled", "hills1", -2147483646, -2147483642, 0x00000000, 0x3e841894, 0xca4971d84c32856b),
        ("scaled", "hills2", -2147483646, -2147483641, 0x80000000, 0x3e883958, 0x1606af56f55e9e04),
        ("scaled", "dales", -2147483646, -2147483642, 0x00000000, 0x3ecccccd, 0x604e1091230c1932),
        ("halfdown", "steppe", 7, 12, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("halfdown", "plains", 7, 12, 0xbca3d70a, 0x3e051eb9, 0xff17844fedc4a4c5),
        ("halfdown", "hills1", 7, 11, 0x00000000, 0x3f19999a, 0xab8b7ca55088abf3),
        ("halfdown", "hills2", 7, 12, 0x80000000, 0x3f1e6667, 0x98426f20663ea361),
        ("halfdown", "dales", 7, 11, 0x00000000, 0x3ecccccd, 0x19776d644d6ce475),
        ("oddbig", "steppe", -7, -2, 0xbca3d70a, 0x3d75c28f, 0xba1362b74aa359d2),
        ("oddbig", "plains", -7, -2, 0xbca3d70a, 0x3e8f5c29, 0x3e41276435ef31ee),
        ("oddbig", "hills1", -7, -3, 0x00000000, 0x3f99999a, 0x37bfd8f3ba6756dc),
        ("oddbig", "hills2", -7, -2, 0x80000000, 0x3f9e6667, 0xfb010b70b002ab58),
        ("oddbig", "dales", -7, -3, 0x00000000, 0x3ecccccd, 0xe2a185f95f689a06),
        ("tiny", "steppe", 0, 5, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("tiny", "plains", 0, 5, 0xbca3d70a, 0xbca3d70a, 0xff17844fedc4a4c5),
        ("tiny", "hills1", 0, 4, 0x00000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "hills2", 0, 5, 0x80000000, 0x00000000, 0x2c1b93daafb34265),
        ("tiny", "dales", 0, 4, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("negative", "steppe", 99, 104, 0xbca3d70a, 0x3d75c28f, 0x40e88a51c53467e4),
        ("negative", "plains", 99, 104, 0xbca3d70a, 0xbe2e147b, 0x91a54a45a82a6b97),
        ("negative", "hills1", 99, 103, 0x80000000, 0xbf19999a, 0xe7b11483b620da4d),
        ("negative", "hills2", 99, 104, 0x00000000, 0xbf1e6667, 0x8022d3283697befa),
        ("negative", "dales", 99, 103, 0x00000000, 0x3ecccccd, 0x6de28f6cec456865),
        ("nan", "steppe", 5, 10, 0xbca3d70a, 0x3d75c28f, 0xff17844fedc4a4c5),
        ("nan", "plains", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills1", 5, 9, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "hills2", 5, 10, 0x7fc00000, 0x7fc00000, 0x12956ce69e90b3c5),
        ("nan", "dales", 5, 9, 0x00000000, 0x3ecccccd, 0x2fec3b0ba97bf943),
        ("huge", "steppe", 3, 8, 0xbca3d70a, 0x3d75c28f, 0x0a645dffde6efa79),
        ("huge", "plains", 3, 8, 0xbca3d70a, 0x6ff2568d, 0xdada775cdb02f309),
        ("huge", "hills1", 3, 7, 0x00000000, 0x70f2568d, 0x8dc617ef4529563f),
        ("huge", "hills2", 3, 8, 0x80000000, 0x70f9e940, 0xc0ec623bf089d277),
        ("huge", "dales", 3, 7, 0x00000000, 0x3ecccccd, 0xb9e0aa41aac5ed80),
        ("unit60", "steppe", -2147483648, -2147483643, 0xbca3d70a, 0x3d75c28f, 0xb7887cb514dad7de),
        ("unit60", "plains", -2147483648, -2147483643, 0xbca3d70a, 0x3e6b851f, 0x38b71332a48a77ea),
        ("unit60", "hills1", -2147483648, -2147483644, 0x00000000, 0x3f800000, 0xd03b8139d46beb97),
        ("unit60", "hills2", -2147483648, -2147483643, 0x80000000, 0x3f840000, 0x1878f64e1a713a2a),
        ("unit60", "dales", -2147483648, -2147483644, 0x00000000, 0x3ecccccd, 0xfc2055251cba1c7d),
        ("unit55", "steppe", 2147483644, -2147483647, 0xbca3d70a, 0x3d75c28f, 0xcc009386f623d713),
        ("unit55", "plains", 2147483644, -2147483647, 0xbca3d70a, 0x3e816578, 0x1705d1ee820fa929),
        ("unit55", "hills1", 2147483644, -2147483648, 0x00000000, 0x3f8ba2e9, 0xb4dd50a98533c91a),
        ("unit55", "hills2", 2147483644, -2147483647, 0x80000000, 0x3f900000, 0x2b6ebb23f4895006),
        ("unit55", "dales", 2147483644, -2147483648, 0x00000000, 0x3ecccccd, 0x6b93cdd6668b400a),
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
                "plains" => forms.plains(&mut seed),
                "hills1" => forms.hills1(&mut seed),
                "hills2" => forms.hills2(&mut seed),
                _ => forms.dales(&mut seed),
            };
            // NaN settings make Scale multiply two NaNs; which NaN's bits come out is up to the compiler (HotSpot
            // returns the scale's +NaN, a debug Rust build the source's -NaN), so only NaN-ness is checked there.
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

    #[test]
    fn plateau_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(123);
        let plateau = forms.plateau(&mut seed);
        assert_eq!(seed.get(), 132);
        assert_eq!(plateau.min_value().to_bits(), 0x00000000);
        assert_eq!(plateau.max_value().to_bits(), 0x3ee26b50);
        assert_eq!(
            bits(&plateau),
            [0x3c3eab37, 0x3e1cfff9, 0x3e1cbbb1, 0x3e18dbdd]
        );

        let mut settings = TerrainSettings::default();
        settings.general.global_vertical_scale = 0.37;
        let forms = LandForms::new(settings);
        let mut seed = Seed::new(i32::MAX - 8);
        let plateau = forms.plateau(&mut seed);
        assert_eq!(seed.get(), i32::MIN);
        assert_eq!(plateau.max_value().to_bits(), 0x3e2af838);
        assert_eq!(
            bits(&plateau),
            [0x3b8ff973, 0x3ddf6222, 0x3d8c5342, 0x3bf244bf]
        );
    }
}
