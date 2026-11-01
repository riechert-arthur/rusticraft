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
    terraforged_engine::Cell,
    terraforged_populator::Populator,
};
use std::sync::Arc;

enum BlendControl {
    Module(Module),
    Populator(Arc<dyn Populator>),
}

impl BlendControl {
    fn value(&self, x: f32, y: f32) -> f32 {
        match self {
            Self::Module(module) => module.value(x, y),
            Self::Populator(populator) => populator.value(x, y),
        }
    }
}

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

    #[allow(clippy::too_many_arguments)]
    pub fn with_populator(
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
            control: BlendControl::Populator(control),
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
        terraforged_engine::TerrainRegistry, terraforged_populator::TerrainPopulator,
    };
    use super::*;

    fn populator(terrain: i32, value: f32) -> Arc<dyn Populator> {
        Arc::new(TerrainPopulator::new(
            TerrainRegistry::new().get_by_id(terrain),
            Module::constant(0.0),
            Module::constant(value),
            1.0,
        ))
    }

    fn sample(populator: &dyn Populator, edge: f32) -> (u32, i32) {
        let mut cell = Cell::default();
        cell.continent_edge = edge;
        populator.apply(&mut cell, 12.25, -98.75);
        (cell.value.to_bits(), cell.terrain.id())
    }

    #[test]
    fn blender_matches_engine_0_3_0() {
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
                populator(8, 0.1),
                populator(12, 0.9),
                0.3,
                0.8,
                0.575,
            );
            assert_eq!(sample(&blender, 0.0), expected);
        }
        let blender = Blender::with_populator(
            populator(11, 0.6),
            populator(8, 0.2),
            populator(12, 0.8),
            0.3,
            0.8,
            0.5,
            0.7,
        );
        assert_eq!(sample(&blender, 0.0), (0x3f0f5c29, 12));
        assert_eq!(blender.tag_threshold().to_bits(), 0x3f333333);
    }

    #[test]
    fn continent_lerpers_match_engine_0_3_0() {
        let two = ContinentLerper2::with_interpolation(
            populator(1, 0.1),
            populator(8, 0.9),
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
            assert_eq!(sample(&two, edge), expected);
        }

        let three = ContinentLerper3::new(
            populator(1, 0.1),
            populator(2, 0.4),
            populator(8, 0.9),
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
            assert_eq!(sample(&three, edge), expected);
        }
    }
}
