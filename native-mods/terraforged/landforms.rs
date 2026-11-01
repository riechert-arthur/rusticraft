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

    pub fn hills2(&self, seed: &mut Seed) -> Module {
        let hills = Module::multiply(vec![
            source(SourceKind::Cubic, seed.next(), 128, 2, 0.5, 2.0),
            perlin(seed.next(), 32, 4, 2.0).alpha(Module::constant(0.075)),
        ])
        .warp(noise_warp(seed.next(), 30, 3, 20.0))
        .warp(noise_warp(seed.next(), 400, 3, 200.0));
        Module::multiply(vec![
            hills,
            source(SourceKind::Ridge, seed.next(), 512, 2, 0.975, 2.0).alpha(Module::constant(0.8)),
        ])
        .scale(Module::constant(
            0.55 * self.settings.general.global_vertical_scale,
        ))
    }

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

    pub fn badlands(&self, seed: &mut Seed) -> Module {
        let mask = perlin(seed.next(), 270, 3, 2.0)
            .clamp(Module::constant(0.35), Module::constant(0.65))
            .map(Module::constant(0.0), Module::constant(1.0));
        let hills = source(SourceKind::Ridge, seed.next(), 275, 4, 0.975, 2.0)
            .warp(noise_warp(seed.next(), 400, 2, 100.0))
            .warp(noise_warp(seed.next(), 18, 1, 20.0));
        let hills = Module::multiply(vec![hills, mask]);
        let modulation = 0.4;
        let alpha = 1.0 - modulation;
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

fn perlin(seed: i32, scale: i32, octaves: usize, lacunarity: f32) -> Module {
    source(SourceKind::Perlin, seed, scale, octaves, 0.5, lacunarity)
}

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

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        perlin(seed, scale, octaves, 2.0),
        perlin(seed.wrapping_add(1), scale, octaves, 2.0),
        Module::constant(strength),
    )
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
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

    #[test]
    fn hills_match_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut hills1_seed = Seed::new(321);
        let hills1 = forms.hills1(&mut hills1_seed);
        assert_eq!(hills1_seed.get(), 325);
        assert_eq!(hills1.min_value().to_bits(), 0x00000000);
        assert_eq!(hills1.max_value().to_bits(), 0x3f16872c);
        assert_eq!(
            bits(&hills1),
            [0x3e16872c, 0x3e2a41f1, 0x3e4e8c02, 0x3e68e25a]
        );

        let mut hills2_seed = Seed::new(-45);
        let hills2 = forms.hills2(&mut hills2_seed);
        assert_eq!(hills2_seed.get(), -40);
        assert_eq!(hills2.min_value().to_bits(), 0x80000000);
        assert_eq!(hills2.max_value().to_bits(), 0x3f1b3b65);
        assert_eq!(
            bits(&hills2),
            [0x3e3929fe, 0x3e4b287b, 0x3e5b0922, 0x3e61f05e]
        );
    }

    #[test]
    fn dales_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(987_654);
        let dales = forms.dales(&mut seed);
        assert_eq!(seed.get(), 987_658);
        assert_eq!(dales.min_value().to_bits(), 0x00000000);
        assert_eq!(dales.max_value().to_bits(), 0x3ecccccd);
        assert_eq!(
            bits(&dales),
            [0x00000000, 0x3de86058, 0x3c1e05a0, 0x3ccd7073]
        );
    }

    #[test]
    fn scaled_hills_match_engine_0_3_0() {
        let mut settings = TerrainSettings::default();
        settings.general.global_vertical_scale = 0.37;
        let forms = LandForms::new(settings);
        let mut hills1_seed = Seed::new(i32::MAX - 3);
        let hills1 = forms.hills1(&mut hills1_seed);
        assert_eq!(hills1_seed.get(), i32::MIN);
        assert_eq!(hills1.max_value().to_bits(), 0x3e6353f9);
        assert_eq!(
            bits(&hills1),
            [0x3d6353f9, 0x3db9159e, 0x3d882819, 0x3dd855d4]
        );

        let mut hills2_seed = Seed::new(i32::MIN + 3);
        let hills2 = forms.hills2(&mut hills2_seed);
        assert_eq!(hills2_seed.get(), i32::MIN + 8);
        assert_eq!(hills2.max_value().to_bits(), 0x3e6a6e98);
        assert_eq!(
            bits(&hills2),
            [0x3d72ed88, 0x3d4b6714, 0x3dafad6e, 0x3dd98c72]
        );
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

    #[test]
    fn badlands_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(777);
        let badlands = forms.badlands(&mut seed);
        assert_eq!(seed.get(), 783);
        assert_eq!(badlands.min_value().to_bits(), 0x3ccccccd);
        assert_eq!(badlands.max_value().to_bits(), 0x3f2f5c29);
        assert_eq!(
            bits(&badlands),
            [0x3e20c49e, 0x3d77e3d4, 0x3e7fca05, 0x3e097a48]
        );

        let mut seed = Seed::new(i32::MAX - 7);
        let badlands = forms.badlands(&mut seed);
        assert_eq!(seed.get(), i32::MAX - 1);
        assert_eq!(
            bits(&badlands),
            [0x3e20c49e, 0x3cff1ff8, 0x3ccccccd, 0x3e0732a5]
        );
    }

    #[test]
    fn torridonian_matches_engine_0_3_0() {
        let forms = LandForms::new(TerrainSettings::default());
        let mut seed = Seed::new(-222);
        let torridonian = forms.torridonian(&mut seed);
        assert_eq!(seed.get(), -213);
        assert_eq!(torridonian.min_value().to_bits(), 0x00000000);
        assert_eq!(torridonian.max_value().to_bits(), 0x3f000000);
        assert_eq!(
            bits(&torridonian),
            [0x3e738a24, 0x3d5a2f19, 0x3eb11487, 0x3e6d83ad]
        );

        let mut seed = Seed::new(i32::MIN + 7);
        let torridonian = forms.torridonian(&mut seed);
        assert_eq!(seed.get(), i32::MIN + 16);
        assert_eq!(
            bits(&torridonian),
            [0x3e738a24, 0x3e531467, 0x3d3dae7e, 0x3eb1a779]
        );
    }
}
