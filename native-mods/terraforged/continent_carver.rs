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
    terraforged::{Source, SourceConfig, SourceKind, clamp, java_max, java_min, lerp, map},
    terraforged_continent_config::{ContinentConfig, RiverConfig},
    terraforged_continent_points::{BEACH, COAST},
    terraforged_river_nodes::{CarverSample, NodeSample},
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::NoiseLevels,
};

/// `RiverCarver.BORDER_OFFSET`: the valley's river-noise floor at the top of the banks.
const BORDER_OFFSET: f32 = 0.05;
/// `RiverCarver.BORDER_RANGE` (`1f - BORDER_OFFSET`, folded by javac to the float `0.95f`).
const BORDER_RANGE: f32 = 0.95;

/// TerraForged's continent `RiverCarver` (`worldgen.noise.continent.river`, the newer source's; its carving
/// arithmetic is byte-identical to the 1.18.2-0.3.1 jar's): cuts the valley, banks and bed of the nearest river and
/// lake node into a `NoiseSample`. The erosion noise is read seed-first, as the newer source's `carve(seed, x, y, …)`
/// does (the jar's `carve(x, y, …)` reads it seedless).
pub struct RiverCarver {
    levels: NoiseLevels,
    /// `erosionNoise`: `Source.builder().seed(rivers.seed + 21221).frequency(128).octaves(2).ridge()` (the builder's
    /// ridge gain, 0.975).
    erosion: Source,
    river_config: RiverConfig,
    lake_config: RiverConfig,
    /// `blendRadius`: `min(1, max(river valley max, lake valley max) + 0.2)` over the scaled configs. Java computes
    /// it and never reads it.
    pub blend_radius: f32,
}

impl RiverCarver {
    /// `new RiverCarver(levels, config)`: copies the river and lake configs scaled by `levels.frequency / scale`
    /// (`1f / int scale` in float: 0 gives infinity), and builds the erosion ridge (`rivers.seed + 21221` wraps like
    /// a Java `int`).
    pub fn new(levels: NoiseLevels, config: ContinentConfig) -> Self {
        let frequency = levels.frequency * (1.0 / config.shape.scale as f32);
        let mut river_config = config.rivers.rivers;
        let mut lake_config = config.rivers.lakes;
        river_config.scale(frequency);
        lake_config.scale(frequency);
        let valley_width = java_max(river_config.valley_width.max, lake_config.valley_width.max);
        Self {
            levels,
            erosion: Source::new(
                SourceKind::Ridge,
                SourceConfig {
                    seed: config.rivers.seed.wrapping_add(21221),
                    frequency: 128.0,
                    octaves: 2,
                    gain: 0.975,
                    ..SourceConfig::default()
                },
            ),
            river_config,
            lake_config,
            blend_radius: java_min(1.0, valley_width + 0.2),
        }
    }

    /// The newer source's `carve(seed, x, y, sample, carverSample)`: reads `erosionNoise.getValue(seed, x, y)`
    /// (Noise2D seed-first: the ridge's own seed plus `seed`), then [`Self::carve_with_erosion`].
    pub fn carve(
        &self,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
        carver: &CarverSample,
    ) {
        self.carve_with_erosion(self.erosion.value_seed_first(seed, x, y), sample, carver);
    }

    /// `carve(seed, x, y, sample, carverSample)` after its first line, with `erosion` standing for
    /// `erosionNoise.getValue(seed, x, y)`: scales the base noise by the coast modifier, carves the river node, then
    /// the lake node (each over the previous base noise), and sets the river noise to 1 below `BEACH`.
    pub fn carve_with_erosion(
        &self,
        erosion: f32,
        sample: &mut NoiseSample,
        carver: &CarverSample,
    ) {
        let modifier = map(sample.continent_noise, COAST, 1.0, 1.0 - COAST);
        let base = sample.base_noise * modifier;
        let base = self.carve_node(
            sample,
            &carver.river,
            &self.river_config,
            base,
            modifier,
            erosion,
        );
        let base = self.carve_node(
            sample,
            &carver.lake,
            &self.lake_config,
            base,
            modifier,
            erosion,
        );
        sample.base_noise = base;
        // `clipRiverNoise`: `continentNoise < BEACH ? 1f : riverNoise`.
        if sample.continent_noise < BEACH {
            sample.river_noise = 1.0;
        }
    }

    /// The private `carve(sample, nodeSample, config, baseNoise, baseModifier, erosion)`. `getBaseNoise`'s -1 means
    /// "outside the valley"; a node whose carved base noise really is -1 is skipped too, as in Java.
    fn carve_node(
        &self,
        sample: &mut NoiseSample,
        node: &NodeSample,
        config: &RiverConfig,
        base: f32,
        modifier: f32,
        erosion: f32,
    ) -> f32 {
        let modified_base = base_noise(sample, node, config, modifier);
        if modified_base == -1.0 {
            return base;
        }
        let base_level = self.levels.to_height_noise(modified_base, 0.0);
        self.carve_height(base_level, erosion, sample, node, config);
        modified_base
    }

    /// The private `carve(baseLevel, erosion, sample, nodeSample, config)`: lowers the height towards the bank
    /// level across the valley (eroded), then towards the bed level inside the banks, where the terrain becomes
    /// the node's. The sample's base noise is still the incoming one here.
    fn carve_height(
        &self,
        base_level: f32,
        erosion: f32,
        sample: &mut NoiseSample,
        node: &NodeSample,
        config: &RiverConfig,
    ) {
        if node.is_invalid() {
            return;
        }
        let mut height = sample.height_noise;
        let position = node.position;
        let distance = node.distance;
        let valley_width = config.valley_width.at(position);
        let bank_width = config.bank_width.at(position);
        let bank_depth = config.bank_depth.at(position);
        let bed_width = config.bed_width.at(position);
        let bed_depth = config.bed_depth.at(position);
        let bed_level = base_level - bed_depth * self.levels.unit;
        let bank_level = base_level + bank_depth * self.levels.unit;
        let valley_alpha = valley_alpha(distance, bank_width, valley_width, sample.base_noise);
        if valley_alpha < 1.0 {
            let level = java_min(bank_level, height);
            let modifier = erosion_modifier(erosion * config.erosion, valley_alpha);
            height = lerp(level, height, valley_alpha * modifier);
            sample.river_noise *= valley_noise(distance, bank_width, valley_width);
        }
        let river_alpha = alpha(distance, bed_width, bank_width);
        if river_alpha < 1.0 {
            let level = java_min(bed_level, height);
            height = lerp(level, height, river_alpha);
            sample.terrain_type = node.terrain.clone();
            // `getRiverNoise`: falls from the top of the banks to the base level.
            sample.river_noise *= clamp(alpha(height, base_level, bank_level), 0.0, 1.0);
        }
        sample.height_noise = height;
    }
}

/// `getBaseNoise`: -1 for an invalid node or one at or past the valley radius, the node's level inside the banks,
/// and a lerp from it to the sample's base noise across the valley, all times the coast modifier.
fn base_noise(sample: &NoiseSample, node: &NodeSample, config: &RiverConfig, modifier: f32) -> f32 {
    if node.is_invalid() {
        return -1.0;
    }
    let valley_radius = config.valley_width.at(node.position);
    if node.distance >= valley_radius {
        return -1.0;
    }
    let bank_radius = config.bank_width.at(node.position);
    if node.distance <= bank_radius {
        return node.level * modifier;
    }
    let alpha = (node.distance - bank_radius) / (valley_radius - bank_radius);
    lerp(node.level, sample.base_noise, alpha) * modifier
}

/// `getErosionModifier`: full erosion inside the valley, fading out over its last 2.5%.
fn erosion_modifier(erosion: f32, valley_alpha: f32) -> f32 {
    let fade = 1.0 - map(valley_alpha, 0.975, 1.0, 0.025);
    1.0 - (erosion * fade)
}

/// `getValleyNoise` (`MathUtil.clamp`, which is `NoiseUtil.clamp`: NaN passes through).
fn valley_noise(distance: f32, bank_width: f32, valley_width: f32) -> f32 {
    clamp(
        BORDER_OFFSET + alpha(distance, bank_width, valley_width) / BORDER_RANGE,
        0.0,
        1.0,
    )
}

/// `getValleyAlpha`: between a U-shaped (squared) and a linear falloff, by the base noise over 0.4..0.6.
fn valley_alpha(distance: f32, bank_width: f32, valley_width: f32, base: f32) -> f32 {
    let alpha_value = alpha(distance, bank_width, valley_width);
    let shape_alpha = alpha(base, 0.4, 0.6);
    lerp(alpha_value * alpha_value, alpha_value, shape_alpha)
}

/// `getAlpha`: 0 at or below `minimum`, 1 at or above `maximum` (`minimum` wins when they cross), linear between;
/// NaN passes through.
fn alpha(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value <= minimum {
        0.0
    } else if value >= maximum {
        1.0
    } else {
        (value - minimum) / (maximum - minimum)
    }
}

/// Bit-exact tests against `ContinentCarverOracle.java` (`.local/terraforged/oracles/continent_carver/`), which runs
/// the jar's own `RiverCarver.carve` with its erosion module replaced by a constant (the jar's carving bytecode is
/// identical to the newer source's; `version/compare.sh` there checks it).
#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_continent_config::FloatRange, terraforged_engine::TerrainRegistry,
    };
    use super::*;

    const INF: f32 = f32::INFINITY;
    const NAN: f32 = f32::NAN;
    const POS: [f32; 8] = [0.0, -0.0, 0.25, 0.6, 1.0, 1.5, NAN, -INF];
    const HEIGHT: [f32; 5] = [0.85, 0.2, -0.0, NAN, -INF];
    const EROSION: [f32; 4] = [0.75, 0.0, NAN, INF];
    const LEVEL: [f32; 4] = [0.4, -1.0, -0.0, NAN];
    const BASE_S: [f32; 4] = [0.65, 0.4, 0.5, NAN];
    const HEIGHT_S: [f32; 3] = [0.85, 0.2, NAN];
    const EROSION_S: [f32; 2] = [0.75, 1.0];
    const LEVEL_S: [f32; 2] = [0.4, NAN];
    const RIVER: [f32; 7] = [1.0, 0.5, 0.0, -0.0, -1.0, NAN, INF];
    const EXTRA: [f32; 7] = [0.0, -0.0, -1.0, INF, -INF, NAN, f32::MAX];
    const FRACTIONS: [f32; 3] = [0.5, 0.975, 0.987_420_9];
    const VARIANTS: i32 = 16;

    fn cont() -> [f32; 7] {
        [
            0.9,
            1.0,
            0.55,
            0.55f32.next_down(),
            0.5,
            0.5f32.next_down(),
            NAN,
        ]
    }

    fn base() -> [f32; 9] {
        [
            0.65,
            0.4,
            0.4f32.next_up(),
            0.6,
            0.6f32.next_down(),
            0.0,
            -0.0,
            NAN,
            INF,
        ]
    }

    struct Fnv(u64, usize);

    impl Fnv {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        fn add_bits(&mut self, bits: u32) {
            for byte in bits.to_le_bytes() {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.1 += 1;
        }

        /// Canonical NaN, as the oracle's `floatToIntBits`: NaN payload signs are not portable.
        fn add(&mut self, value: f32) {
            self.add_bits(if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            });
        }

        fn add_config(&mut self, config: &RiverConfig) {
            self.add(config.erosion);
            for range in [
                config.bed_width,
                config.bank_width,
                config.valley_width,
                config.bed_depth,
                config.bank_depth,
            ] {
                self.add(range.min);
                self.add(range.max);
            }
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    fn levels(variant: i32) -> NoiseLevels {
        match variant {
            1 => NoiseLevels::new(true, 1.0, 63, 0, 384, 64),
            2 => NoiseLevels::new(false, 0.0, 62, 22, 256, 128),
            3 => NoiseLevels::new(true, 2.5, 62, 22, 480, 128),
            4 => NoiseLevels::new(false, -3.0, 1, 0, 1, 0),
            _ => NoiseLevels::new(false, 1.0, 62, 22, 256, 128),
        }
    }

    fn config(variant: i32) -> ContinentConfig {
        let mut c = ContinentConfig::default();
        let (r, l) = (&mut c.rivers.rivers, &mut c.rivers.lakes);
        match variant {
            1 => c.shape.scale = 1,
            2 => c.shape.scale = 0,
            3 => c.shape.scale = -400,
            4 => c.shape.scale = i32::MAX,
            5 => (r.bank_width.min, r.valley_width.max) = (NAN, NAN),
            6 => (r.bank_width.min, l.valley_width.max) = (INF, -INF),
            7 => (r.erosion, l.erosion) = (NAN, -0.0),
            8 => {
                (r.bed_depth, r.bank_depth) =
                    (FloatRange::new(-0.0, INF), FloatRange::new(NAN, -1.0))
            }
            9 => r.valley_width = r.bank_width,
            10 => r.bank_width = FloatRange::new(250.0, 300.0),
            11 => l.valley_width.max = NAN,
            12 => (l.valley_width.max, r.valley_width.max) = (1000.0, -0.0),
            13 => (r.bed_width, r.erosion) = (FloatRange::new(40.0, 50.0), 2.0),
            14 => {
                r.bed_width = FloatRange::new(-0.0, 0.0);
                r.bank_width = FloatRange::new(0.0, -0.0);
                r.valley_width = FloatRange::new(-0.0, 0.0);
            }
            15 => (c.shape.scale, c.rivers.rivers.erosion) = (3, 1e30),
            _ => {}
        }
        c
    }

    /// The oracle's `distances`: each scaled radius at `position` with its float neighbours, points across the
    /// valley, and edge values.
    fn distances(config: &RiverConfig, position: f32) -> Vec<f32> {
        let bed = config.bed_width.at(position);
        let bank = config.bank_width.at(position);
        let valley = config.valley_width.at(position);
        let mut out = Vec::new();
        for value in [bed, bank, valley] {
            out.extend([value.next_down(), value, value.next_up()]);
        }
        out.extend(FRACTIONS.map(|fraction| bank + fraction * (valley - bank)));
        out.extend(EXTRA);
        out
    }

    struct Run {
        fnv: Fnv,
        sample: NoiseSample,
        nodes: CarverSample,
        none: NoiseSample,
    }

    impl Run {
        fn new(terrains: &TerrainRegistry) -> Self {
            Self {
                fnv: Fnv::new(),
                sample: NoiseSample::new(terrains),
                nodes: CarverSample::new(terrains),
                none: NoiseSample::new(terrains),
            }
        }

        fn carve(
            &mut self,
            carver: &RiverCarver,
            cont: f32,
            base: f32,
            height: f32,
            river: f32,
            erosion: f32,
        ) {
            let sample = &mut self.sample;
            sample.continent_noise = cont;
            sample.base_noise = base;
            sample.height_noise = height;
            sample.river_noise = river;
            sample.terrain_type = self.none.terrain_type.clone();
            carver.carve_with_erosion(erosion, sample, &self.nodes);
            let terrain = &sample.terrain_type;
            let code = if terrain.is_same(&self.nodes.river.terrain) {
                1
            } else if terrain.is_same(&self.nodes.lake.terrain) {
                2
            } else if terrain.is_same(&self.none.terrain_type) {
                0
            } else {
                3
            };
            let (b, h, r) = (sample.base_noise, sample.height_noise, sample.river_noise);
            self.fnv.add(b);
            self.fnv.add(h);
            self.fnv.add(r);
            self.fnv.add_bits(code);
        }

        fn grid(
            &mut self,
            carver: &RiverCarver,
            cont: &[f32],
            base: &[f32],
            height: &[f32],
            erosion: &[f32],
            level: &[f32],
        ) {
            for position in POS {
                for distance in distances(&carver.river_config, position) {
                    for &node_level in level {
                        self.nodes.river.position = position;
                        self.nodes.river.distance = distance;
                        self.nodes.river.level = node_level;
                        for &c in cont {
                            for &b in base {
                                for &h in height {
                                    for &e in erosion {
                                        self.carve(carver, c, b, h, 1.0, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// `ContinentCarverOracle`'s four sections: 16 configurations (blend radius, both scaled configs, a small grid:
    /// int scales 0, negative and `MAX`, NaN/infinite/signed-zero widths, depths and erosion, equal and crossed radii)
    /// over five noise levels; the river node over the full grid (each radius and its float neighbours, points
    /// across the valley through the 0.975 erosion fade, the 0.4/0.6 shape and the COAST/BEACH thresholds and their
    /// neighbours, a node level whose carved base is the -1 sentinel, NaN, infinities, signed zeros); incoming river
    /// noise; and river-then-lake precedence.
    #[test]
    fn carving_matches_the_jar() {
        let terrains = TerrainRegistry::new();
        let mut run = Run::new(&terrains);
        let cont_s = [0.9, 0.55, 0.5f32.next_down()];
        for variant in 0..VARIANTS {
            let carver = RiverCarver::new(levels(variant % 5), config(variant));
            run.fnv.add(carver.blend_radius);
            run.fnv.add_config(&carver.river_config);
            run.fnv.add_config(&carver.lake_config);
            run.nodes = CarverSample::new(&terrains);
            run.grid(&carver, &cont_s, &BASE_S, &HEIGHT_S, &EROSION_S, &LEVEL_S);
        }
        run.fnv.check("configs", 1_401_200, 0xc326_9d8d_f5c3_27d6);

        let carver = RiverCarver::new(levels(0), config(0));
        run.nodes = CarverSample::new(&terrains);
        run.grid(&carver, &cont(), &base(), &HEIGHT, &EROSION, &LEVEL);
        run.fnv.check("river", 3_064_320, 0x6b1f_b646_22ad_bb10);

        for river in RIVER {
            for position in [0.6, NAN] {
                for distance in distances(&carver.river_config, position) {
                    run.nodes.river.position = position;
                    run.nodes.river.distance = distance;
                    run.nodes.river.level = 0.4;
                    for c in [0.9, 0.5f32.next_down()] {
                        for b in [0.65, 0.4] {
                            for h in [0.85, 0.2] {
                                run.carve(&carver, c, b, h, river, 0.75);
                            }
                        }
                    }
                }
            }
        }
        run.fnv.check("noise", 8_512, 0x74a6_4087_6b3f_3f9e);

        let river = &carver.river_config;
        let river_distances = [
            NAN,
            0.0,
            river.bed_width.at(0.6),
            river.bank_width.at(0.6),
            river.valley_width.at(0.6).next_down(),
        ];
        for river_distance in river_distances {
            for lake_position in [0.0, 0.8, 1.0, NAN] {
                for lake_distance in distances(&carver.lake_config, lake_position) {
                    for lake_level in [0.55, -1.0, NAN] {
                        let nodes = &mut run.nodes;
                        (
                            nodes.river.position,
                            nodes.river.distance,
                            nodes.river.level,
                        ) = (0.6, river_distance, 0.4);
                        (nodes.lake.position, nodes.lake.distance, nodes.lake.level) =
                            (lake_position, lake_distance, lake_level);
                        for c in [0.9, 0.55] {
                            for b in [0.65, 0.4, NAN] {
                                for h in [0.85, 0.2] {
                                    run.carve(&carver, c, b, h, 1.0, 0.75);
                                }
                            }
                        }
                    }
                }
            }
        }
        run.fnv.check("lake", 54_720, 0x1c0c_99f2_5b31_ece4);
    }

    /// The PR's original spot checks (default levels and config, erosion 0.75): the valley edge, inside the bed,
    /// the banks, outside the valley, below `BEACH`, and a lake carved after the river.
    #[test]
    fn carving_spot_checks() {
        let terrains = TerrainRegistry::new();
        let carver = RiverCarver::new(levels(0), config(0));
        let expected = [
            (0x3f016c16, 0x3f59999a, 0x3f800000, "none"),
            (0x3e9f49f4, 0x3ec4a4fa, 0, "river"),
            (0x3e9f49f4, 0x3eca861e, 0, "river"),
            (0x3ebde00f, 0x3f0e744c, 0x3ebf2e24, "none"),
            (0x3f016c16, 0x3f59999a, 0x3f800000, "none"),
            (0, 0x3e75c248, 0x3f800000, "river"),
            (0x3edb05b0, 0x3eca861e, 0, "lake"),
        ];
        for (index, distance) in [NAN, 0.0, 0.03, 0.15, 1.0, 0.03, 0.03]
            .into_iter()
            .enumerate()
        {
            let mut sample = NoiseSample::new(&terrains);
            sample.continent_noise = if index == 5 { 0.1 } else { 0.9 };
            sample.base_noise = 0.65;
            sample.height_noise = 0.85;
            let mut nodes = CarverSample::new(&terrains);
            (
                nodes.river.position,
                nodes.river.level,
                nodes.river.distance,
            ) = (0.6, 0.4, distance);
            if index == 6 {
                (nodes.lake.position, nodes.lake.level, nodes.lake.distance) = (0.8, 0.55, 0.01);
            }
            carver.carve_with_erosion(0.75, &mut sample, &nodes);
            let terrain = &sample.terrain_type;
            assert_eq!(
                (
                    sample.base_noise.to_bits(),
                    sample.height_noise.to_bits(),
                    sample.river_noise.to_bits(),
                    terrain.name()
                ),
                expected[index],
                "{index}"
            );
        }
        assert_eq!(carver.blend_radius, 0.7);
    }
}
