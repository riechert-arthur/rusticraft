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
}
