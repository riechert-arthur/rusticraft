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
    terraforged::{Interpolation, Module},
    terraforged_engine::{Cell, TerrainRegistry},
    terraforged_populator::Populator,
};
use std::sync::Arc;

/// Engine's `Select` control: a module, or a populator read through its `getValue` (`apply` on a
/// fresh cell of the world's registry, kept here as `fresh`).
enum BlendControl {
    Module(Module),
    Populator {
        populator: Arc<dyn Populator>,
        fresh: Box<Cell>,
    },
}

impl BlendControl {
    fn value(&self, x: f32, y: f32) -> f32 {
        match self {
            Self::Module(module) => module.value(x, y),
            Self::Populator { populator, fresh } => {
                let mut cell = Cell::clone(fresh);
                populator.apply(&mut cell, x, y);
                cell.value
            }
        }
    }
}

/// Engine's `Blender` (Heightmap's land: region terrain below, mountain chains above): below `min`
/// only `lower` applies, above `max` only `upper`; in between both apply to the cell in turn and
/// `value` is lerped linearly, keeping `lower`'s terrain while the select is below the midpoint
/// `min + (max - min) * split`. Every other field keeps what the two populators wrote.
pub struct Blender {
    control: BlendControl,
    lower: Arc<dyn Populator>,
    upper: Arc<dyn Populator>,
    blend_lower: f32,
    blend_upper: f32,
    blend_range: f32,
    midpoint: f32,
    tag_threshold: f32,
}

impl Blender {
    pub fn with_module(
        control: Module,
        lower: Arc<dyn Populator>,
        upper: Arc<dyn Populator>,
        min: f32,
        max: f32,
        split: f32,
    ) -> Self {
        let blend_range = max - min;
        let midpoint = min + blend_range * split;
        Self {
            control: BlendControl::Module(control),
            lower,
            upper,
            blend_lower: min,
            blend_upper: max,
            blend_range,
            midpoint,
            tag_threshold: midpoint,
        }
    }

    /// Engine's populator-controlled constructor; `registry` is the world's, for the control's
    /// scratch cell. Engine stores `tag_threshold` but never reads it.
    #[allow(clippy::too_many_arguments)]
    pub fn with_populator(
        registry: &TerrainRegistry,
        control: Arc<dyn Populator>,
        lower: Arc<dyn Populator>,
        upper: Arc<dyn Populator>,
        min: f32,
        max: f32,
        split: f32,
        tag_threshold: f32,
    ) -> Self {
        let blend_range = max - min;
        Self {
            control: BlendControl::Populator {
                populator: control,
                fresh: Box::new(Cell::new(registry)),
            },
            lower,
            upper,
            blend_lower: min,
            blend_upper: max,
            blend_range,
            midpoint: min + blend_range * split,
            tag_threshold,
        }
    }

    pub const fn tag_threshold(&self) -> f32 {
        self.tag_threshold
    }
}

impl Populator for Blender {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let select = self.control.value(x, y);
        if select < self.blend_lower {
            self.lower.apply(cell, x, y);
            return;
        }
        if select > self.blend_upper {
            self.upper.apply(cell, x, y);
            return;
        }
        let alpha = Interpolation::Linear.apply((select - self.blend_lower) / self.blend_range);
        self.lower.apply(cell, x, y);
        let lower_value = cell.value;
        let lower_type = cell.terrain.clone();
        self.upper.apply(cell, x, y);
        let upper_value = cell.value;
        cell.value = lower_value + alpha * (upper_value - lower_value);
        if select < self.midpoint {
            cell.terrain = lower_type;
        }
    }
}

/// Engine's `ContinentLerper2` (Heightmap's root: oceans below the shallow-ocean point, land above
/// inland): selects on the cell's `continent_edge`, as [`Blender`] does but without terrain handling
/// (the cell keeps the terrain `upper` wrote); `Interpolation::Linear` by default.
pub struct ContinentLerper2 {
    lower: Arc<dyn Populator>,
    upper: Arc<dyn Populator>,
    interpolation: Interpolation,
    blend_lower: f32,
    blend_upper: f32,
    blend_range: f32,
}

impl ContinentLerper2 {
    pub fn new(lower: Arc<dyn Populator>, upper: Arc<dyn Populator>, min: f32, max: f32) -> Self {
        Self::with_interpolation(lower, upper, min, max, Interpolation::Linear)
    }

    pub fn with_interpolation(
        lower: Arc<dyn Populator>,
        upper: Arc<dyn Populator>,
        min: f32,
        max: f32,
        interpolation: Interpolation,
    ) -> Self {
        Self {
            lower,
            upper,
            interpolation,
            blend_lower: min,
            blend_upper: max,
            blend_range: max - min,
        }
    }
}

impl Populator for ContinentLerper2 {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        if cell.continent_edge < self.blend_lower {
            self.lower.apply(cell, x, y);
            return;
        }
        if cell.continent_edge > self.blend_upper {
            self.upper.apply(cell, x, y);
            return;
        }
        let alpha = self
            .interpolation
            .apply((cell.continent_edge - self.blend_lower) / self.blend_range);
        self.lower.apply(cell, x, y);
        let lower_value = cell.value;
        self.upper.apply(cell, x, y);
        cell.value = lower_value + alpha * (cell.value - lower_value);
    }
}

/// Engine's `ContinentLerper3` (Heightmap's oceans: deep, shallow, coast): selects on
/// `continent_edge`, lerping `lower`→`middle` below the midpoint and `middle`→`upper` from it on;
/// `Interpolation::Curve3` by default.
pub struct ContinentLerper3 {
    lower: Arc<dyn Populator>,
    middle: Arc<dyn Populator>,
    upper: Arc<dyn Populator>,
    interpolation: Interpolation,
    midpoint: f32,
    blend_lower: f32,
    blend_upper: f32,
    lower_range: f32,
    upper_range: f32,
}

impl ContinentLerper3 {
    pub fn new(
        lower: Arc<dyn Populator>,
        middle: Arc<dyn Populator>,
        upper: Arc<dyn Populator>,
        min: f32,
        midpoint: f32,
        max: f32,
    ) -> Self {
        Self::with_interpolation(
            lower,
            middle,
            upper,
            min,
            midpoint,
            max,
            Interpolation::Curve3,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_interpolation(
        lower: Arc<dyn Populator>,
        middle: Arc<dyn Populator>,
        upper: Arc<dyn Populator>,
        min: f32,
        midpoint: f32,
        max: f32,
        interpolation: Interpolation,
    ) -> Self {
        Self {
            lower,
            middle,
            upper,
            interpolation,
            midpoint,
            blend_lower: min,
            blend_upper: max,
            lower_range: midpoint - min,
            upper_range: max - midpoint,
        }
    }
}

impl Populator for ContinentLerper3 {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let select = cell.continent_edge;
        if select < self.blend_lower {
            self.lower.apply(cell, x, y);
            return;
        }
        if select > self.blend_upper {
            self.upper.apply(cell, x, y);
            return;
        }
        if select < self.midpoint {
            let alpha = self
                .interpolation
                .apply((select - self.blend_lower) / self.lower_range);
            self.lower.apply(cell, x, y);
            let lower_value = cell.value;
            self.middle.apply(cell, x, y);
            cell.value = lower_value + alpha * (cell.value - lower_value);
        } else {
            let alpha = self
                .interpolation
                .apply((select - self.midpoint) / self.upper_range);
            self.middle.apply(cell, x, y);
            let lower_value = cell.value;
            self.upper.apply(cell, x, y);
            cell.value = lower_value + alpha * (cell.value - lower_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::{Source, SourceConfig, SourceKind},
        terraforged_engine::{BiomeType, ControlPoints},
        terraforged_populator::TerrainPopulator,
    };
    use super::*;
    use std::fmt::Write as _;

    fn populator(registry: &TerrainRegistry, terrain: i32, value: f32) -> Arc<dyn Populator> {
        Arc::new(TerrainPopulator::new(
            registry.get_by_id(terrain),
            Module::constant(0.0),
            Module::constant(value),
            1.0,
        ))
    }

    fn sample(registry: &TerrainRegistry, populator: &dyn Populator, edge: f32) -> (u32, i32) {
        let mut cell = Cell::new(registry);
        cell.continent_edge = edge;
        populator.apply(&mut cell, 12.25, -98.75);
        (cell.value.to_bits(), cell.terrain.id())
    }

    #[test]
    fn blender_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let r = &registry;
        for (select, expected) in [
            (0.2, (0x3dcccccd, 8)),
            (0.3, (0x3dcccccd, 8)),
            (0.4, (0x3e851eb8, 8)),
            (0.55, (0x3effffff, 8)),
            (0.8, (0x3f666666, 12)),
            (0.9, (0x3f666666, 12)),
        ] {
            let blender = Blender::with_module(
                Module::constant(select),
                populator(r, 8, 0.1),
                populator(r, 12, 0.9),
                0.3,
                0.8,
                0.575,
            );
            assert_eq!(sample(r, &blender, 0.0), expected);
        }
        let blender = Blender::with_populator(
            r,
            populator(r, 11, 0.6),
            populator(r, 8, 0.2),
            populator(r, 12, 0.8),
            0.3,
            0.8,
            0.5,
            0.7,
        );
        assert_eq!(sample(r, &blender, 0.0), (0x3f0f5c29, 12));
        assert_eq!(blender.tag_threshold().to_bits(), 0x3f333333);
    }

    #[test]
    fn continent_lerpers_match_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let r = &registry;
        let two = ContinentLerper2::with_interpolation(
            populator(r, 1, 0.1),
            populator(r, 8, 0.9),
            0.25,
            0.75,
            Interpolation::Curve3,
        );
        for (edge, expected) in [
            (0.1, (0x3dcccccd, 1)),
            (0.25, (0x3dcccccd, 8)),
            (0.5, (0x3effffff, 8)),
            (0.75, (0x3f666666, 8)),
            (0.9, (0x3f666666, 8)),
        ] {
            assert_eq!(sample(r, &two, edge), expected);
        }

        let three = ContinentLerper3::new(
            populator(r, 1, 0.1),
            populator(r, 2, 0.4),
            populator(r, 8, 0.9),
            0.1,
            0.3,
            0.6,
        );
        for (edge, expected) in [
            (0.0, (0x3dcccccd, 1)),
            (0.1, (0x3dcccccd, 2)),
            (0.2, (0x3e800000, 2)),
            (0.3, (0x3ecccccd, 8)),
            (0.45, (0x3f266665, 8)),
            (0.6, (0x3f666666, 8)),
            (0.8, (0x3f666666, 8)),
        ] {
            assert_eq!(sample(r, &three, edge), expected);
        }
    }

    // Java's `BiomeType` names in id order, for the oracle's text.
    const BIOME_NAMES: [&str; 11] = [
        "TROPICAL_RAINFOREST",
        "SAVANNA",
        "DESERT",
        "TEMPERATE_RAINFOREST",
        "TEMPERATE_FOREST",
        "GRASSLAND",
        "COLD_STEPPE",
        "STEPPE",
        "TAIGA",
        "TUNDRA",
        "ALPINE",
    ];

    /// Raw bits; every NaN prints as `nan` (the oracle canonicalizes NaN signs).
    fn bits(value: f32) -> String {
        if value.is_nan() {
            "nan".to_owned()
        } else {
            format!("{:08x}", value.to_bits())
        }
    }

    fn dump(cell: &Cell) -> String {
        format!(
            "value={} erosion={} sediment={} gradient={} moisture={} temperature={} continentId={} continentEdge={} trId={} trEdge={} trCenter={} brId={} brEdge={} macro={} river={} cx={} cz={} emask={} terrain={} biome={} absent={}",
            bits(cell.value),
            bits(cell.erosion),
            bits(cell.sediment),
            bits(cell.gradient),
            bits(cell.moisture),
            bits(cell.temperature),
            bits(cell.continent_id),
            bits(cell.continent_edge),
            bits(cell.terrain_region_id),
            bits(cell.terrain_region_edge),
            cell.terrain_region_center,
            bits(cell.biome_region_id),
            bits(cell.biome_region_edge),
            bits(cell.macro_biome_id),
            bits(cell.river_mask),
            cell.continent_x,
            cell.continent_z,
            cell.erosion_mask,
            cell.terrain.id(),
            BIOME_NAMES[cell.biome.id() as usize],
            cell.is_absent(),
        )
    }

    fn filled(registry: &TerrainRegistry) -> Cell {
        let mut cell = Cell::new(registry);
        cell.value = 0.75;
        cell.erosion = 0.25;
        cell.sediment = 0.125;
        cell.gradient = 0.875;
        cell.moisture = 0.2;
        cell.temperature = 0.8;
        cell.continent_id = 0.1;
        cell.continent_edge = 0.9;
        cell.terrain_region_id = 0.3;
        cell.terrain_region_edge = 0.7;
        cell.terrain_region_center = 123_456_789;
        cell.biome_region_id = 0.4;
        cell.biome_region_edge = 0.6;
        cell.macro_biome_id = 0.55;
        cell.river_mask = 0.45;
        cell.continent_x = -123;
        cell.continent_z = 456;
        cell.erosion_mask = true;
        cell.terrain = registry.get_by_id(14);
        cell.biome = BiomeType::Alpine;
        cell
    }

    /// Writes only `value`.
    struct Plain(f32);

    impl Populator for Plain {
        fn apply(&self, cell: &mut Cell, _x: f32, _z: f32) {
            cell.value = self.0;
        }
    }

    /// Reads the cell before writing, so the order of a blender's two applies shows.
    struct Reader(f32);

    impl Populator for Reader {
        fn apply(&self, cell: &mut Cell, x: f32, _z: f32) {
            let old = cell.value;
            cell.value = old * 0.5 + self.0 + x * 1.0e-4;
            cell.gradient = old;
            cell.erosion += 0.0625;
        }
    }

    /// `Source.perlin(seed, scale, octaves)` with Engine's builder defaults.
    fn perlin(seed: i32, scale: i32, octaves: usize) -> Module {
        Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed,
                octaves,
                gain: 0.5,
                frequency: 1.0 / scale as f32,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    }

    /// The oracle's `tp`: a terrain populator over a constant plus a quarter of a Perlin.
    fn tp(registry: &TerrainRegistry, terrain: i32, seed: i32, value: f32) -> Arc<dyn Populator> {
        Arc::new(TerrainPopulator::new(
            registry.get_by_id(terrain),
            Module::constant(value),
            perlin(seed, 64, 2).scale(Module::constant(0.25)),
            1.0,
        ))
    }

    fn around(points: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for &point in points {
            out.extend([point.next_down(), point, point.next_up()]);
        }
        out.extend([
            0.0,
            -0.0,
            1.0,
            -0.5,
            1.5,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            1.4e-45,
        ]);
        out.extend((0..=32).map(|i| i as f32 / 32.0));
        out
    }

    const XZ: [(f32, f32); 3] = [(12.25, -98.75), (0.0, 0.0), (-4096.5, 8192.25)];

    fn edge(
        out: &mut String,
        registry: &TerrainRegistry,
        name: &str,
        populator: &dyn Populator,
        edges: &[f32],
    ) {
        for &edge in edges {
            for (x, z) in XZ {
                let mut fresh = Cell::new(registry);
                fresh.continent_edge = edge;
                populator.apply(&mut fresh, x, z);
                let mut full = filled(registry);
                full.continent_edge = edge;
                populator.apply(&mut full, x, z);
                let prefix = format!("{name} {} {} {}", bits(edge), bits(x), bits(z));
                writeln!(out, "{prefix} fresh {}", dump(&fresh)).unwrap();
                writeln!(out, "{prefix} filled {}", dump(&full)).unwrap();
            }
        }
    }

    fn blended(
        out: &mut String,
        registry: &TerrainRegistry,
        prefix: &str,
        blender: &Blender,
        x: f32,
        z: f32,
    ) {
        let mut fresh = Cell::new(registry);
        blender.apply(&mut fresh, x, z);
        let mut full = filled(registry);
        blender.apply(&mut full, x, z);
        writeln!(
            out,
            "{prefix} getValue={}",
            bits(blender.value(registry, x, z))
        )
        .unwrap();
        writeln!(out, "{prefix} fresh {}", dump(&fresh)).unwrap();
        writeln!(out, "{prefix} filled {}", dump(&full)).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn select(
        out: &mut String,
        registry: &TerrainRegistry,
        name: &str,
        (min, max, split): (f32, f32, f32),
        lower: &Arc<dyn Populator>,
        upper: &Arc<dyn Populator>,
        selects: &[f32],
    ) {
        for &select in selects {
            for (x, z) in XZ {
                let blender = Blender::with_module(
                    Module::constant(select),
                    lower.clone(),
                    upper.clone(),
                    min,
                    max,
                    split,
                );
                let prefix = format!("{name} {} {} {}", bits(select), bits(x), bits(z));
                blended(out, registry, &prefix, &blender, x, z);
            }
        }
    }

    /// Rebuilds the text `BlendOracle` prints from Engine 0.3.0 (`.local/terraforged/oracles/blend/`).
    fn oracle_text() -> String {
        let registry = TerrainRegistry::new();
        let r = &registry;
        let mut out = String::new();
        let cp = ControlPoints::default();
        let mid = 0.3f32 + (0.8f32 - 0.3) * 0.575;
        writeln!(
            out,
            "controlPoints deep={} shallow={} coast={} inland={} mid={}",
            bits(cp.deep_ocean),
            bits(cp.shallow_ocean),
            bits(cp.coast),
            bits(cp.inland),
            bits(mid)
        )
        .unwrap();
        let heightmap = (0.3, 0.8, 0.575);
        let sels = around(&[0.3, 0.8, mid]);
        let (flats, chain) = (tp(r, 8, 11, 0.2), tp(r, 13, 12, 0.7));
        select(&mut out, r, "blender-tp", heightmap, &flats, &chain, &sels);
        let plain: Arc<dyn Populator> = Arc::new(Plain(0.1));
        select(
            &mut out,
            r,
            "blender-plain",
            heightmap,
            &plain,
            &chain,
            &sels,
        );
        let (low, high): (Arc<dyn Populator>, Arc<dyn Populator>) =
            (Arc::new(Reader(0.1)), Arc::new(Reader(0.6)));
        select(&mut out, r, "blender-reader", heightmap, &low, &high, &sels);
        select(
            &mut out,
            r,
            "blender-flat",
            (0.5, 0.5, 0.5),
            &flats,
            &chain,
            &around(&[0.5]),
        );
        let reversed = around(&[0.3, 0.8, 0.675]);
        select(
            &mut out,
            r,
            "blender-reversed",
            (0.8, 0.3, 0.25),
            &flats,
            &chain,
            &reversed,
        );
        let nan_split = (0.3, 0.8, f32::NAN);
        let sels = around(&[0.3, 0.8]);
        select(
            &mut out,
            r,
            "blender-nansplit",
            nan_split,
            &flats,
            &chain,
            &sels,
        );

        let controls: [Arc<dyn Populator>; 3] = [
            tp(r, 11, 21, 0.4),
            Arc::new(ContinentLerper2::new(
                Arc::new(Plain(0.2)),
                Arc::new(Plain(0.9)),
                0.25,
                0.75,
            )),
            Arc::new(Reader(0.45)),
        ];
        for (i, control) in controls.into_iter().enumerate() {
            let blender = Blender::with_populator(
                r,
                control,
                flats.clone(),
                chain.clone(),
                0.3,
                0.8,
                0.575,
                0.9,
            );
            for gx in -3..=3 {
                for gz in -3..=3 {
                    let (x, z) = (gx as f32 * 37.5 + 0.25, gz as f32 * 41.25 - 0.5);
                    let prefix = format!("blender-control{i} {} {}", bits(x), bits(z));
                    blended(&mut out, r, &prefix, &blender, x, z);
                }
            }
        }

        let e2 = around(&[cp.shallow_ocean, cp.inland, 0.376]);
        let (deep, flat) = (tp(r, 1, 31, 0.1), tp(r, 8, 32, 0.6));
        let linear = ContinentLerper2::new(deep.clone(), flat.clone(), cp.shallow_ocean, cp.inland);
        edge(&mut out, r, "lerp2-linear", &linear, &e2);
        let curve3 = ContinentLerper2::with_interpolation(
            deep.clone(),
            flat.clone(),
            cp.shallow_ocean,
            cp.inland,
            Interpolation::Curve3,
        );
        edge(&mut out, r, "lerp2-curve3", &curve3, &e2);
        let reader = ContinentLerper2::new(
            Arc::new(Reader(0.1)),
            Arc::new(Reader(0.6)),
            cp.shallow_ocean,
            cp.inland,
        );
        edge(&mut out, r, "lerp2-reader", &reader, &e2);
        let flat_range = ContinentLerper2::new(deep.clone(), flat.clone(), 0.5, 0.5);
        edge(&mut out, r, "lerp2-flat", &flat_range, &around(&[0.5]));
        let reversed = ContinentLerper2::new(deep, flat, 0.7, 0.2);
        edge(
            &mut out,
            r,
            "lerp2-reversed",
            &reversed,
            &around(&[0.2, 0.7]),
        );

        let e3 = around(&[cp.deep_ocean, cp.shallow_ocean, cp.coast]);
        let oceans = || (tp(r, 1, 41, 0.05), tp(r, 2, 42, 0.2), tp(r, 3, 43, 0.35));
        let (a, b, c) = oceans();
        let three = ContinentLerper3::new(a, b, c, cp.deep_ocean, cp.shallow_ocean, cp.coast);
        edge(&mut out, r, "lerp3-curve3", &three, &e3);
        let (a, b, c) = oceans();
        let three = ContinentLerper3::with_interpolation(
            a,
            b,
            c,
            cp.deep_ocean,
            cp.shallow_ocean,
            cp.coast,
            Interpolation::Linear,
        );
        edge(&mut out, r, "lerp3-linear", &three, &e3);
        let three = ContinentLerper3::new(
            Arc::new(Reader(0.05)),
            Arc::new(Reader(0.2)),
            Arc::new(Reader(0.35)),
            cp.deep_ocean,
            cp.shallow_ocean,
            cp.coast,
        );
        edge(&mut out, r, "lerp3-reader", &three, &e3);
        for (name, (min, mid, max), edges) in [
            ("lerp3-flatlow", (0.3, 0.3, 0.6), around(&[0.3, 0.6])),
            ("lerp3-flathigh", (0.3, 0.6, 0.6), around(&[0.3, 0.6])),
            ("lerp3-unordered", (0.6, 0.2, 0.4), around(&[0.2, 0.4, 0.6])),
        ] {
            let (a, b, c) = oceans();
            edge(
                &mut out,
                r,
                name,
                &ContinentLerper3::new(a, b, c, min, mid, max),
                &edges,
            );
        }

        // Heightmap's root over a grid.
        let land: Arc<dyn Populator> = Arc::new(Blender::with_module(
            perlin(7, 300, 3),
            tp(r, 8, 51, 0.3),
            tp(r, 13, 52, 0.75),
            0.3,
            0.8,
            0.575,
        ));
        let terrain = |id: i32, value: f32| -> Arc<dyn Populator> {
            Arc::new(TerrainPopulator::new(
                r.get_by_id(id),
                Module::constant(0.0),
                Module::constant(value),
                1.0,
            ))
        };
        let oceans: Arc<dyn Populator> = Arc::new(ContinentLerper3::new(
            tp(r, 1, 53, 0.05),
            terrain(2, 0.2),
            terrain(3, 0.25),
            cp.deep_ocean,
            cp.shallow_ocean,
            cp.coast,
        ));
        let root = ContinentLerper2::new(oceans, land, cp.shallow_ocean, cp.inland);
        for ei in 0..=24 {
            for gx in -4..=4 {
                for gz in -4..=4 {
                    let (edge, x, z) = (ei as f32 / 24.0, gx as f32 * 113.5, gz as f32 * 97.25);
                    let mut fresh = Cell::new(r);
                    fresh.continent_edge = edge;
                    root.apply(&mut fresh, x, z);
                    let mut full = filled(r);
                    full.continent_edge = edge;
                    root.apply(&mut full, x, z);
                    let prefix = format!("root {} {} {}", bits(edge), bits(x), bits(z));
                    writeln!(out, "{prefix} fresh {}", dump(&fresh)).unwrap();
                    writeln!(out, "{prefix} filled {}", dump(&full)).unwrap();
                }
            }
        }
        out
    }

    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Every cell field and `getValue` after Blender, ContinentLerper2 and ContinentLerper3 on a
    /// fresh and on a fully set cell, at each boundary (Heightmap's 0.3 / 0.8 / split midpoint and
    /// Engine's default control points), one ulp either side, signed zeros, NaN, infinities and a
    /// sweep; degenerate and reversed ranges, a NaN split, populator controls, populators that read
    /// the cell, and Heightmap's root chain over a grid. The text is `BlendOracle`'s.
    #[test]
    fn blenders_match_engine_oracle() {
        let text = oracle_text();
        assert_eq!(text.lines().count(), EXPECTED_LINES);
        assert_eq!(fnv1a(&text), EXPECTED_HASH);
    }

    const EXPECTED_LINES: usize = 10_441;
    const EXPECTED_HASH: u64 = 0xd529_13f7_7065_d084;
}
