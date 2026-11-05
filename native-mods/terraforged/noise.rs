/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
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

use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy)]
pub enum Interpolation {
    Linear,
    Curve3,
    Curve4,
}

impl Interpolation {
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Linear => value,
            Self::Curve3 => value * value * (3.0 - 2.0 * value),
            Self::Curve4 => value * value * value * (value * (value * 6.0 - 15.0) + 10.0),
        }
    }
}

/// Engine's `CurveFunc`: an `Interpolation`, a `MidPointCurve` or an `SCurve`.
#[derive(Clone, Copy)]
pub enum CurveFunction {
    Interpolation(Interpolation),
    /// `MidPointCurve(mid, steepness)`: `NoiseUtil.curve`, a logistic curve over `NoiseUtil.exp`.
    Midpoint {
        midpoint: f32,
        steepness: f32,
    },
    /// `SCurve`, built with [`CurveFunction::s_curve`].
    SCurve(SCurve),
}

/// `SCurve`'s fields, private so that only [`CurveFunction::s_curve`] (Engine's constructor) sets them.
#[derive(Clone, Copy)]
pub struct SCurve {
    lower: f32,
    upper: f32,
}

#[derive(Clone, Copy)]
pub enum DistanceFunction {
    Euclidean,
    Manhattan,
    Natural,
}

impl DistanceFunction {
    pub fn apply(self, x: f32, y: f32) -> f32 {
        match self {
            Self::Euclidean => x * x + y * y,
            Self::Manhattan => x.abs() + y.abs(),
            Self::Natural => x.abs() + y.abs() + (x * x + y * y),
        }
    }
}

#[derive(Clone, Copy)]
pub enum EdgeFunction {
    Distance2,
    Distance2Add,
    Distance2Sub,
    Distance2Mul,
    Distance2Div,
}

#[derive(Clone, Copy)]
pub enum CellFunction {
    CellValue,
    NoiseLookup,
    Distance,
}

impl EdgeFunction {
    pub fn apply(self, distance: f32, distance2: f32) -> f32 {
        match self {
            Self::Distance2 => distance2 - 1.0,
            Self::Distance2Add => distance2 + distance - 1.0,
            Self::Distance2Sub => distance2 - distance - 1.0,
            Self::Distance2Mul => distance2 * distance - 1.0,
            Self::Distance2Div => distance / distance2 - 1.0,
        }
    }

    pub fn bounds(self) -> (f32, f32, f32) {
        match self {
            Self::Distance2 => (-1.0, 1.0, 2.0),
            Self::Distance2Add => (-1.0, 1.6, 2.6),
            Self::Distance2Sub => (-1.0, 0.8, 1.8),
            Self::Distance2Mul => (-1.0, 0.7, 1.7),
            Self::Distance2Div => (-1.0, 0.0, 1.0),
        }
    }
}

impl CurveFunction {
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Interpolation(interpolation) => interpolation.apply(value),
            Self::Midpoint {
                midpoint,
                steepness,
            } => noise_curve(value, midpoint, steepness),
            Self::SCurve(SCurve { lower, upper }) => java_pow(value, lower + upper * value),
        }
    }

    /// `new SCurve(lower, upper)`: a negative `upper` is raised to at least `-lower` (`Math.max`).
    pub fn s_curve(lower: f32, upper: f32) -> Self {
        let upper = if upper < 0.0 {
            java_max(-lower, upper)
        } else {
            upper
        };
        Self::SCurve(SCurve { lower, upper })
    }
}

const GRAD_2D: [[f32; 2]; 8] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [-1.0, 1.0],
    [1.0, 1.0],
    [0.0, -1.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [1.0, 0.0],
];

const GRAD_2D_24: [[f32; 2]; 32] = [
    [0.13052619, 0.9914449],
    [0.38268343, 0.9238795],
    [0.6087614, 0.7933533],
    [0.6087614, 0.7933533],
    [0.7933533, 0.6087614],
    [0.9238795, 0.38268343],
    [0.9914449, 0.13052619],
    [0.9914449, 0.13052619],
    [0.9914449, -0.13052619],
    [0.9238795, -0.38268343],
    [0.7933533, -0.6087614],
    [0.7933533, -0.6087614],
    [0.6087614, -0.7933533],
    [0.38268343, -0.9238795],
    [0.13052619, -0.9914449],
    [0.13052619, -0.9914449],
    [-0.13052619, -0.9914449],
    [-0.38268343, -0.9238795],
    [-0.6087614, -0.7933533],
    [-0.6087614, -0.7933533],
    [-0.7933533, -0.6087614],
    [-0.9238795, -0.38268343],
    [-0.9914449, -0.13052619],
    [-0.9914449, -0.13052619],
    [-0.9914449, 0.13052619],
    [-0.9238795, 0.38268343],
    [-0.7933533, 0.6087614],
    [-0.7933533, 0.6087614],
    [-0.6087614, 0.7933533],
    [-0.38268343, 0.9238795],
    [-0.13052619, 0.9914449],
    [-0.13052619, 0.9914449],
];

const CELL_2D: [[f32; 2]; 256] = [
    [0.06864607, 0.62819433],
    [0.32666832, 0.9152784],
    [0.21780425, 0.14947817],
    [0.21935263, 0.8517628],
    [0.8125509, 0.17625329],
    [0.83830184, 0.20326465],
    [0.0606018, 0.40289584],
    [0.053955644, 0.44046497],
    [0.19777697, 0.8334085],
    [0.28731894, 0.103431255],
    [0.20088434, 0.83619905],
    [0.7293324, 0.8871778],
    [0.9475439, 0.4530485],
    [0.6777518, 0.9134057],
    [0.66885227, 0.0828802],
    [0.4023403, 0.939275],
    [0.58450186, 0.94199485],
    [0.09012395, 0.31425387],
    [0.8476586, 0.21428421],
    [0.164933, 0.19961673],
    [0.729819, 0.11311084],
    [0.48930076, 0.9498728],
    [0.053985864, 0.4402388],
    [0.8650294, 0.7631607],
    [0.15052056, 0.78348565],
    [0.087727934, 0.68036556],
    [0.23267218, 0.8619887],
    [0.822124, 0.18577698],
    [0.7880446, 0.15426844],
    [0.8892171, 0.27414596],
    [0.9492085, 0.47332188],
    [0.050227523, 0.514308],
    [0.6278175, 0.068534255],
    [0.464279, 0.94858],
    [0.0702593, 0.36649746],
    [0.17821822, 0.8145735],
    [0.19428411, 0.8302087],
    [0.08549601, 0.6751755],
    [0.12618601, 0.75052565],
    [0.72368914, 0.10953468],
    [0.5029678, 0.050009787],
    [0.67471284, 0.0853008],
    [0.05762276, 0.4175235],
    [0.2236039, 0.14488706],
    [0.09806141, 0.2976504],
    [0.8871414, 0.27060616],
    [0.06999126, 0.63263667],
    [0.46962425, 0.051026374],
    [0.15138185, 0.78454417],
    [0.54553515, 0.05230975],
    [0.46241972, 0.94842803],
    [0.8266409, 0.809525],
    [0.5654002, 0.0547778],
    [0.5340903, 0.94870687],
    [0.055080622, 0.56742966],
    [0.074406385, 0.35381493],
    [0.9499173, 0.5086273],
    [0.55242604, 0.94693565],
    [0.050469488, 0.47944975],
    [0.37952244, 0.93357253],
    [0.15801361, 0.2075187],
    [0.886532, 0.73041916],
    [0.545061, 0.0522618],
    [0.43716535, 0.9455915],
    [0.89326, 0.28126147],
    [0.94722617, 0.4501127],
    [0.8753571, 0.2517923],
    [0.2263377, 0.857224],
    [0.67004615, 0.9166345],
    [0.91026926, 0.6848761],
    [0.82322717, 0.18691185],
    [0.21176898, 0.15442386],
    [0.70509726, 0.9005435],
    [0.94140863, 0.41248745],
    [0.33152997, 0.9172743],
    [0.49602196, 0.9499824],
    [0.29448664, 0.9003302],
    [0.49390432, 0.05004129],
    [0.38037717, 0.06619084],
    [0.8901528, 0.2757663],
    [0.51723534, 0.94966984],
    [0.19849297, 0.83405614],
    [0.48485738, 0.05025485],
    [0.08574259, 0.3242422],
    [0.30836228, 0.09284526],
    [0.8749249, 0.74886006],
    [0.2762226, 0.8904147],
    [0.08336568, 0.32995337],
    [0.86191714, 0.7674247],
    [0.68911266, 0.09166631],
    [0.1872575, 0.8235616],
    [0.17181921, 0.80789185],
    [0.27051932, 0.88708997],
    [0.15547338, 0.78948474],
    [0.08329046, 0.33013785],
    [0.24210969, 0.13122827],
    [0.13879621, 0.76838744],
    [0.72679967, 0.88866687],
    [0.7071571, 0.89948213],
    [0.58355176, 0.05782458],
    [0.06877667, 0.62863296],
    [0.8257055, 0.8105091],
    [0.6777011, 0.08657247],
    [0.054817468, 0.5656698],
    [0.89551437, 0.7146355],
    [0.0735386, 0.64363384],
    [0.12062004, 0.25798586],
    [0.546176, 0.052375406],
    [0.12859458, 0.24591732],
    [0.7563571, 0.13016075],
    [0.53476644, 0.948655],
    [0.19345456, 0.82943875],
    [0.274302, 0.8893076],
    [0.9116448, 0.31820747],
    [0.20922542, 0.15656129],
    [0.78422785, 0.15112391],
    [0.81145895, 0.17520264],
    [0.94641554, 0.44331557],
    [0.19626659, 0.16796684],
    [0.9079607, 0.6899159],
    [0.15130511, 0.21554989],
    [0.8264822, 0.80969244],
    [0.82111424, 0.8152549],
    [0.51183385, 0.94984436],
    [0.9333844, 0.6211526],
    [0.8118669, 0.8244057],
    [0.22724652, 0.8579184],
    [0.077771366, 0.34436262],
    [0.39903, 0.061473995],
    [0.22588289, 0.14312494],
    [0.3534875, 0.92548096],
    [0.7302279, 0.11335403],
    [0.13005644, 0.7562065],
    [0.6057004, 0.06259009],
    [0.23532864, 0.86393553],
    [0.8521828, 0.78012013],
    [0.31358123, 0.090429455],
    [0.6994493, 0.09661436],
    [0.8937065, 0.7179339],
    [0.17738417, 0.8137181],
    [0.87962353, 0.74163187],
    [0.6482922, 0.924864],
    [0.0925996, 0.6911149],
    [0.9212853, 0.65817297],
    [0.23787028, 0.86577046],
    [0.2463013, 0.12833217],
    [0.28997636, 0.8979825],
    [0.86241525, 0.76674926],
    [0.13549614, 0.23611188],
    [0.73184866, 0.88567626],
    [0.17395431, 0.81015193],
    [0.28699547, 0.10360491],
    [0.8814999, 0.26134157],
    [0.15702268, 0.79131866],
    [0.06441343, 0.6129794],
    [0.28953204, 0.8977477],
    [0.8348365, 0.19935977],
    [0.8430469, 0.79123676],
    [0.27081633, 0.11273414],
    [0.75477076, 0.1290662],
    [0.9236175, 0.3481836],
    [0.34612256, 0.9228732],
    [0.059255064, 0.59079593],
    [0.43194723, 0.055175513],
    [0.9453517, 0.43548763],
    [0.75624645, 0.13008413],
    [0.8278198, 0.19172388],
    [0.08772257, 0.31964666],
    [0.8354091, 0.19999877],
    [0.94663286, 0.44505385],
    [0.33910465, 0.92025316],
    [0.40536046, 0.9399356],
    [0.47362313, 0.05077371],
    [0.9471028, 0.4490188],
    [0.0634329, 0.6091292],
    [0.10401413, 0.7137643],
    [0.07599518, 0.34926873],
    [0.11172053, 0.72746223],
    [0.07163474, 0.36214787],
    [0.8303888, 0.80552125],
    [0.8321435, 0.19638726],
    [0.08697894, 0.32135618],
    [0.5840306, 0.05791533],
    [0.11771172, 0.7373935],
    [0.12876043, 0.24567503],
    [0.9472364, 0.45020437],
    [0.053340882, 0.5547323],
    [0.54862726, 0.052635074],
    [0.07968986, 0.33925363],
    [0.7205361, 0.8922548],
    [0.13750994, 0.7666476],
    [0.09639132, 0.30100244],
    [0.7152728, 0.8951678],
    [0.06406072, 0.38838938],
    [0.9178354, 0.6670735],
    [0.700763, 0.90273345],
    [0.42721933, 0.055924594],
    [0.8644749, 0.23607183],
    [0.06825483, 0.6268704],
    [0.20256355, 0.8376856],
    [0.20013279, 0.8355289],
    [0.23263258, 0.86195946],
    [0.7808423, 0.85160714],
    [0.84989464, 0.78297305],
    [0.27703142, 0.89087725],
    [0.8305823, 0.80531186],
    [0.25633186, 0.12168023],
    [0.46597224, 0.94871163],
    [0.18064117, 0.81703305],
    [0.94546336, 0.436263],
    [0.9483504, 0.5384954],
    [0.05726415, 0.41947067],
    [0.554523, 0.9466847],
    [0.21874392, 0.8512763],
    [0.6266697, 0.9318041],
    [0.12640187, 0.75084746],
    [0.7959708, 0.1610291],
    [0.12856227, 0.7540355],
    [0.095532894, 0.30275303],
    [0.66361654, 0.080798835],
    [0.8289186, 0.19289646],
    [0.2505175, 0.125489],
    [0.5328313, 0.94880074],
    [0.33369392, 0.081858516],
    [0.3931668, 0.9371346],
    [0.5644026, 0.054632396],
    [0.0516769, 0.5388124],
    [0.07862225, 0.3420735],
    [0.5509792, 0.052896976],
    [0.7050584, 0.09943658],
    [0.91780984, 0.33286256],
    [0.14348105, 0.22541988],
    [0.94783986, 0.5440398],
    [0.16000003, 0.20521191],
    [0.8767122, 0.74614614],
    [0.18610656, 0.82244515],
    [0.35379982, 0.07441157],
    [0.89702904, 0.28817946],
    [0.9459149, 0.4395031],
    [0.08951107, 0.3156123],
    [0.6475104, 0.07486391],
    [0.5925803, 0.94037354],
    [0.34103352, 0.07901347],
    [0.7482445, 0.8753327],
    [0.9383624, 0.39832214],
    [0.56242806, 0.94564867],
    [0.7846591, 0.1514757],
    [0.15579724, 0.21013024],
    [0.61981887, 0.06624496],
    [0.2564093, 0.8783696],
    [0.7958191, 0.16089669],
    [0.3835992, 0.93468475],
    [0.62740374, 0.06841189],
    [0.8680473, 0.7589231],
    [0.7451437, 0.8773653],
    [0.06854904, 0.62786734],
];

/// Java's `NoiseUtil.floor`. The `int` cast saturates and the `- 1` wraps, exactly as in Java.
pub fn floor(value: f32) -> i32 {
    if value >= 0.0 {
        value as i32
    } else {
        (value as i32).wrapping_sub(1)
    }
}

pub fn hash_2d(seed: i32, x: i32, y: i32) -> i32 {
    let mut hash = seed;
    hash ^= 1619_i32.wrapping_mul(x);
    hash ^= 31337_i32.wrapping_mul(y);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60493);
    (hash >> 13) ^ hash
}

pub fn value_coord_2d(seed: i32, x: i32, y: i32) -> f32 {
    let mut value = seed;
    value ^= 1619_i32.wrapping_mul(x);
    value ^= 31337_i32.wrapping_mul(y);
    value
        .wrapping_mul(value)
        .wrapping_mul(value)
        .wrapping_mul(60493) as f32
        / 2_147_483_648.0_f32
}

#[derive(Clone, Copy)]
struct CellResult {
    x: i32,
    y: i32,
    /// The nearest cell's feature vector; `None` (Java's null `vec2f`) when no distance beat `Float.MAX_VALUE`,
    /// as for NaN or infinite coordinates or squared distances that overflow.
    vector: Option<[f32; 2]>,
}

pub fn cell_vector(seed: i32, x: i32, y: i32) -> [f32; 2] {
    CELL_2D[(hash_2d(seed, x, y) & 255) as usize]
}

/// Engine's `Ridge.Mode`: how far `Ridge.Noise` lerps from the erosion value back to the source sample.
#[derive(Clone, Copy)]
pub enum ErosionMode {
    Constant,
    InputLinear,
    OutputLinear,
}

impl ErosionMode {
    fn blend(self, value: f32, erosion: f32, strength: f32) -> f32 {
        match self {
            Self::Constant => 1.0 - strength,
            Self::InputLinear => 1.0 - strength * value,
            Self::OutputLinear => 1.0 - strength * erosion,
        }
    }
}

/// Engine's erosion `Ridge` (`com.terraforged.engine.module.Ridge`, spec name `Valley`), not the `RidgeNoise` source:
/// the distance to the nearest segment from a jittered cell point to its lowest-sampled neighbour, over a few octaves.
#[derive(Clone, Copy)]
pub struct ErosionRidge {
    seed: i32,
    octaves: i32,
    strength: f32,
    grid_size: f32,
    amplitude: f32,
    lacunarity: f32,
    distance_falloff: f32,
    blend_mode: ErosionMode,
}

impl ErosionRidge {
    /// `new Ridge(seed, strength, gridSize, blendMode)`: one octave.
    pub fn new(seed: i32, strength: f32, grid_size: f32, blend_mode: ErosionMode) -> Self {
        Self::with_octaves(seed, 1, strength, grid_size, blend_mode)
    }

    /// `new Ridge(seed, octaves, strength, gridSize, blendMode)`: an amplitude of `1 / (octaves + 1)` (the `int` sum
    /// wraps), a lacunarity of 2.25 and a distance fall-off of 0.75.
    pub fn with_octaves(
        seed: i32,
        octaves: i32,
        strength: f32,
        grid_size: f32,
        blend_mode: ErosionMode,
    ) -> Self {
        Self::full(
            seed,
            octaves,
            strength,
            grid_size,
            1.0 / octaves.wrapping_add(1) as f32,
            2.25,
            0.75,
            blend_mode,
        )
    }

    /// Engine's full `Ridge` constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn full(
        seed: i32,
        octaves: i32,
        strength: f32,
        grid_size: f32,
        amplitude: f32,
        lacunarity: f32,
        distance_falloff: f32,
        blend_mode: ErosionMode,
    ) -> Self {
        Self {
            seed,
            octaves,
            strength,
            grid_size,
            amplitude,
            lacunarity,
            distance_falloff,
            blend_mode,
        }
    }

    /// `Ridge.wrap(source)`.
    pub fn wrap(self, source: Module) -> Module {
        Module::erosion_ridge(self, source)
    }

    /// `Ridge.getValue(x, y, source)`: the source sample, then the erosion, blended by the mode.
    pub fn value(self, x: f32, y: f32, source: &Module) -> f32 {
        let mut cache = [0.0; 25];
        let value = source.value(x, y);
        let erosion = self.erosion_value_with_cache(x, y, source, &mut cache);
        lerp(
            erosion,
            value,
            self.blend_mode.blend(value, erosion, self.strength),
        )
    }

    /// `Ridge.getErosionValue`: the octaves' gain-weighted mean (NaN for zero or negative octaves).
    pub fn erosion_value(self, x: f32, y: f32, source: &Module) -> f32 {
        self.erosion_value_with_cache(x, y, source, &mut [0.0; 25])
    }

    fn erosion_value_with_cache(
        self,
        mut x: f32,
        mut y: f32,
        source: &Module,
        cache: &mut [f32; 25],
    ) -> f32 {
        let mut sum = 0.0;
        let mut max = 0.0;
        let mut gain = 1.0;
        let mut distance = self.grid_size;
        for _ in 0..self.octaves {
            let value = self.single_erosion_value(x, y, distance, source, cache);
            sum += value * gain;
            max += gain;
            gain *= self.amplitude;
            distance *= self.distance_falloff;
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        sum / max
    }

    fn single_erosion_value(
        self,
        x: f32,
        y: f32,
        grid_size: f32,
        source: &Module,
        cache: &mut [f32; 25],
    ) -> f32 {
        cache.fill(-1.0);
        let pixel_x = floor(x / grid_size);
        let pixel_y = floor(y / grid_size);
        let mut min_height2 = f32::MAX;
        for delta_y1 in -1_i32..=1 {
            for delta_x1 in -1_i32..=1 {
                let point_a_x = pixel_x.wrapping_add(delta_x1);
                let point_a_y = pixel_y.wrapping_add(delta_y1);
                let vector_a = cell_vector(self.seed, point_a_x, point_a_y);
                let ax = (point_a_x as f32 + vector_a[0]) * grid_size;
                let ay = (point_a_y as f32 + vector_a[1]) * grid_size;
                let mut bx = ax;
                let mut by = ay;
                let mut lowest_neighbour = f32::MAX;
                for delta_y2 in -1_i32..=1 {
                    for delta_x2 in -1_i32..=1 {
                        let point_b_x = point_a_x.wrapping_add(delta_x2);
                        let point_b_y = point_a_y.wrapping_add(delta_y2);
                        let vector_b = if point_b_x == point_a_x && point_b_y == point_a_y {
                            vector_a
                        } else {
                            cell_vector(self.seed, point_b_x, point_b_y)
                        };
                        let candidate_x = (point_b_x as f32 + vector_b[0]) * grid_size;
                        let candidate_y = (point_b_y as f32 + vector_b[1]) * grid_size;
                        let index =
                            ((delta_y1 + delta_y2 + 2) * 5 + delta_x1 + delta_x2 + 2) as usize;
                        let mut height = cache[index];
                        if height == -1.0 {
                            height = source.value(candidate_x, candidate_y);
                            cache[index] = height;
                        }
                        if height < lowest_neighbour {
                            lowest_neighbour = height;
                            bx = candidate_x;
                            by = candidate_y;
                        }
                    }
                }
                let height2 = segment_distance2(x, y, ax, ay, bx, by);
                if height2 < min_height2 {
                    min_height2 = height2;
                }
            }
        }
        clamp((f64::from(min_height2).sqrt() as f32) / grid_size, 0.0, 1.0)
    }
}

fn segment_distance2(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let point_x = px - ax;
    let point_y = py - ay;
    let segment_x = bx - ax;
    let segment_y = by - ay;
    let projection = point_x * segment_x + point_y * segment_y;
    let length2 = segment_x * segment_x + segment_y * segment_y;
    let scale = clamp(projection / length2, 0.0, 1.0);
    let dx = segment_x * scale - point_x;
    let dy = segment_y * scale - point_y;
    dx * dx + dy * dy
}

fn nearest_cell(
    x: f32,
    y: f32,
    seed: i32,
    displacement: f32,
    distance: DistanceFunction,
) -> CellResult {
    let xi = floor(x);
    let yi = floor(y);
    let mut result = CellResult {
        x: xi,
        y: yi,
        vector: None,
    };
    let mut nearest = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cell_x = xi.wrapping_add(dx);
            let cell_y = yi.wrapping_add(dy);
            let vector = cell_vector(seed, cell_x, cell_y);
            let delta_x = cell_x as f32 + vector[0] * displacement - x;
            let delta_y = cell_y as f32 + vector[1] * displacement - y;
            let candidate = distance.apply(delta_x, delta_y);
            if candidate < nearest {
                nearest = candidate;
                result = CellResult {
                    x: cell_x,
                    y: cell_y,
                    vector: Some(vector),
                };
            }
        }
    }
    result
}

/// Java's `Noise.cell` with `CellFunc.CELL_VALUE`: the value of the nearest of the 3×3 cells around `(x, y)`.
pub fn cell_value(x: f32, y: f32, seed: i32, displacement: f32, distance: DistanceFunction) -> f32 {
    let cell = nearest_cell(x, y, seed, displacement, distance);
    value_coord_2d(seed, cell.x, cell.y)
}

/// Java's `Noise.cellEdge`: `edge` applied to the nearest and second-nearest of the 3×3 cells around `(x, y)`.
pub fn cell_edge(
    x: f32,
    y: f32,
    seed: i32,
    displacement: f32,
    edge: EdgeFunction,
    distance: DistanceFunction,
) -> f32 {
    let xi = floor(x);
    let yi = floor(y);
    let mut nearest = f32::MAX;
    let mut second = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cell_x = xi.wrapping_add(dx);
            let cell_y = yi.wrapping_add(dy);
            let vector = cell_vector(seed, cell_x, cell_y);
            let delta_x = cell_x as f32 + vector[0] * displacement - x;
            let delta_y = cell_y as f32 + vector[1] * displacement - y;
            let candidate = distance.apply(delta_x, delta_y);
            if candidate < nearest {
                second = nearest;
                nearest = candidate;
            } else if candidate < second {
                second = candidate;
            }
        }
    }
    edge.apply(nearest, second)
}

fn gradient(seed: i32, x: i32, y: i32, better: bool) -> [f32; 2] {
    let hash = hash_2d(seed, x, y);
    if better {
        let selector = (((hash & 0x3f_ffff) as f32 * 1.3333334) as i32 & 31) as usize;
        GRAD_2D_24[selector]
    } else {
        GRAD_2D[(hash & 7) as usize]
    }
}

fn grad(seed: i32, x: i32, y: i32, xd: f32, yd: f32, better: bool) -> f32 {
    let gradient = gradient(seed, x, y, better);
    xd * gradient[0] + yd * gradient[1]
}

pub fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
    a + alpha * (b - a)
}

/// Classic Perlin noise over the 8 directions of `GRAD_2D` (Java `Noise.singlePerlin`).
pub fn perlin(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    perlin_with(x, y, seed, interpolation, false)
}

/// Perlin noise over the 24 directions of `GRAD_2D_24` (Java `Noise.singlePerlin2`).
pub fn perlin2(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    perlin_with(x, y, seed, interpolation, true)
}

fn perlin_with(x: f32, y: f32, seed: i32, interpolation: Interpolation, better: bool) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, better),
        grad(seed, x1, y0, xd1, yd0, better),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, better),
        grad(seed, x1, y1, xd1, yd1, better),
        xs,
    );
    lerp(lower, upper, ys)
}

pub fn simplex(x: f32, y: f32, seed: i32) -> f32 {
    simplex_scaled(x, y, seed, 99.83685)
}

pub fn legacy_simplex(x: f32, y: f32, seed: i32) -> f32 {
    simplex_scaled(x, y, seed, 79.869484)
}

fn simplex_scaled(x: f32, y: f32, seed: i32, scale: f32) -> f32 {
    let skew = (x + y) * 0.36602542;
    let i = floor(x + skew);
    let j = floor(y + skew);
    let unskew = (i.wrapping_add(j)) as f32 * 0.21132487;
    let x0 = x - (i as f32 - unskew);
    let y0 = y - (j as f32 - unskew);
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let x1 = x0 - i1 as f32 + 0.21132487;
    let y1 = y0 - j1 as f32 + 0.21132487;
    let x2 = x0 - 1.0 + 0.42264974;
    let y2 = y0 - 1.0 + 0.42264974;
    fn corner(seed: i32, x: i32, y: i32, xd: f32, yd: f32) -> f32 {
        let mut attenuation = 0.5 - xd * xd - yd * yd;
        if attenuation < 0.0 {
            return 0.0;
        }
        attenuation *= attenuation;
        attenuation * attenuation * grad(seed, x, y, xd, yd, true)
    }
    scale
        * (corner(seed, i, j, x0, y0)
            + corner(seed, i.wrapping_add(i1), j.wrapping_add(j1), x1, y1)
            + corner(seed, i.wrapping_add(1), j.wrapping_add(1), x2, y2))
}

fn cubic_lerp(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let p = d - c - (a - b);
    t * t * t * p + t * t * (a - b - p) + t * (c - a) + b
}

pub fn cubic(x: f32, y: f32, seed: i32) -> f32 {
    let x1 = floor(x);
    let y1 = floor(y);
    let rows = [-1, 0, 1, 2].map(|dy| {
        let values =
            [-1, 0, 1, 2].map(|dx| value_coord_2d(seed, x1.wrapping_add(dx), y1.wrapping_add(dy)));
        cubic_lerp(values[0], values[1], values[2], values[3], x - x1 as f32)
    });
    cubic_lerp(rows[0], rows[1], rows[2], rows[3], y - y1 as f32) * 0.44444445
}

#[derive(Clone, Copy)]
pub struct SourceConfig {
    pub seed: i32,
    pub octaves: usize,
    pub gain: f32,
    pub frequency: f32,
    pub lacunarity: f32,
    pub interpolation: Interpolation,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            seed: 1337,
            octaves: 1,
            gain: 0.5,
            frequency: 1.0,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        }
    }
}

#[derive(Clone, Copy)]
pub enum SourceKind {
    Perlin,
    Perlin2,
    Simplex,
    Simplex2,
    Cubic,
    Ridge,
    Billow,
    SimplexRidge,
}

#[derive(Clone, Copy)]
pub struct Source {
    config: SourceConfig,
    kind: SourceKind,
    min: f32,
    max: f32,
    range: f32,
}

impl Source {
    pub fn new(kind: SourceKind, config: SourceConfig) -> Self {
        let (min, max) = match kind {
            SourceKind::Perlin | SourceKind::Perlin2 => {
                let bound = octave_bound(
                    &[1.0, 0.9, 0.83, 0.75, 0.64, 0.62, 0.61],
                    config,
                    config.gain,
                );
                (-bound, bound)
            }
            SourceKind::Simplex | SourceKind::Simplex2 => {
                let bound =
                    octave_bound(&[1.0, 0.989, 0.81, 0.781, 0.708, 0.702, 0.696], config, 1.0);
                (-bound, bound)
            }
            SourceKind::Cubic => {
                let bound = cubic_bound(0.75, config.octaves, config.gain);
                (-bound, bound)
            }
            SourceKind::Ridge | SourceKind::Billow => (0.0, ridge_bound(config)),
            SourceKind::SimplexRidge => {
                let bound =
                    octave_bound(&[1.0, 0.989, 0.81, 0.781, 0.708, 0.702, 0.696], config, 1.0);
                (0.0, bound)
            }
        };
        let range = max - min;
        Self {
            config,
            kind,
            min,
            max,
            // PerlinNoise, PerlinNoise2, RidgeNoise (so BillowNoise) and SimplexRidgeNoise take `Math.abs`:
            // their bound is negative for a negative gain (or, for the ridged ones, a negative lacunarity).
            range: match kind {
                SourceKind::Simplex | SourceKind::Simplex2 | SourceKind::Cubic => range,
                _ => range.abs(),
            },
        }
    }

    pub fn value(self, x: f32, y: f32) -> f32 {
        self.evaluate(x, y, self.config.seed, false, map)
    }

    pub fn value_seeded(self, x: f32, y: f32, seed: i32) -> f32 {
        self.evaluate(x, y, seed, true, map)
    }

    /// The newer Noise2D's seed-first `getValue(seed, x, y)` (commit 2cb4843, used by the current
    /// TerraForged source): `NoiseSource` samples with its own seed plus the call's (Java `int` wrap);
    /// `PerlinNoise`/`PerlinNoise2` override it and ignore the call's seed. Outputs go through the newer
    /// [`map_current`] (1, not NaN, for a zero or infinite range). [`Source::value`] and
    /// [`Source::value_seeded`] keep the jar's construction-time seeding.
    pub fn value_seed_first(self, seed: i32, x: f32, y: f32) -> f32 {
        let seed = if matches!(self.kind, SourceKind::Perlin | SourceKind::Perlin2) {
            self.config.seed
        } else {
            self.config.seed.wrapping_add(seed)
        };
        self.evaluate(x, y, seed, false, map_current)
    }

    /// `Module.minValue()`: only `CubicNoise` overrides it, with its raw octave bound (not the mapped
    /// output range); the other sources keep `Noise`'s default of 0.
    pub fn min_value(self) -> f32 {
        match self.kind {
            SourceKind::Cubic => self.min,
            _ => 0.0,
        }
    }

    /// `Module.maxValue()`: the raw bound for `CubicNoise`, `Noise`'s default of 1 for the others.
    pub fn max_value(self) -> f32 {
        match self.kind {
            SourceKind::Cubic => self.max,
            _ => 1.0,
        }
    }

    /// `mapping` is the `NoiseUtil.map` of the API version: [`map`] (jar) or [`map_current`] (newer).
    fn evaluate(
        self,
        mut x: f32,
        mut y: f32,
        seed: i32,
        explicit_seed: bool,
        mapping: fn(f32, f32, f32, f32) -> f32,
    ) -> f32 {
        x *= self.config.frequency;
        y *= self.config.frequency;
        if matches!(self.kind, SourceKind::Cubic) {
            let mut sum = cubic(x, y, seed);
            let mut amplitude = 1.0;
            let mut current_seed = seed;
            for _ in 1..self.config.octaves {
                x *= self.config.lacunarity;
                y *= self.config.lacunarity;
                current_seed = current_seed.wrapping_add(1);
                amplitude *= self.config.gain;
                sum += cubic(x, y, current_seed) * amplitude;
            }
            return mapping(sum, self.min, self.max, self.range);
        }
        if matches!(
            self.kind,
            SourceKind::Ridge | SourceKind::Billow | SourceKind::SimplexRidge
        ) {
            let mut value = 0.0;
            let mut weight = 1.0;
            let mut amplitude = 2.0;
            let mut spectral_frequency = 1.0_f32;
            for octave in 0..self.config.octaves {
                let octave_seed = seed.wrapping_add(octave as i32);
                let mut signal = if matches!(self.kind, SourceKind::SimplexRidge) {
                    simplex(x, y, octave_seed)
                } else {
                    perlin(x, y, octave_seed, self.config.interpolation)
                };
                signal = 1.0 - signal.abs();
                signal *= signal;
                signal *= weight;
                weight = clamp(signal * amplitude, 0.0, 1.0);
                value += signal * spectral_weight(spectral_frequency);
                spectral_frequency *= self.config.lacunarity;
                x *= self.config.lacunarity;
                y *= self.config.lacunarity;
                amplitude *= self.config.gain;
            }
            let result = mapping(value, self.min, self.max, self.range);
            return if matches!(self.kind, SourceKind::Billow) {
                1.0 - result
            } else {
                result
            };
        }

        let mut sum = 0.0;
        let mut amplitude = if matches!(self.kind, SourceKind::Perlin | SourceKind::Perlin2) {
            self.config.gain
        } else {
            1.0
        };
        for octave in 0..self.config.octaves {
            let octave_seed = seed.wrapping_add(octave as i32);
            let signal = match self.kind {
                SourceKind::Perlin => perlin(x, y, octave_seed, self.config.interpolation),
                SourceKind::Perlin2 if !explicit_seed => {
                    perlin2(x, y, octave_seed, self.config.interpolation)
                }
                SourceKind::Perlin2 => perlin(x, y, octave_seed, self.config.interpolation),
                SourceKind::Simplex => legacy_simplex(x, y, octave_seed),
                SourceKind::Simplex2 => simplex(x, y, octave_seed),
                SourceKind::Cubic => unreachable!(),
                SourceKind::Ridge | SourceKind::Billow | SourceKind::SimplexRidge => unreachable!(),
            };
            sum += signal * amplitude;
            x *= self.config.lacunarity;
            y *= self.config.lacunarity;
            amplitude *= self.config.gain;
        }
        mapping(sum, self.min, self.max, self.range)
    }
}

fn octave_bound(signals: &[f32; 7], config: SourceConfig, mut amplitude: f32) -> f32 {
    let signal = signals[config.octaves.min(signals.len() - 1)];
    let mut sum = 0.0;
    for _ in 0..config.octaves {
        sum += signal * amplitude;
        amplitude *= config.gain;
    }
    sum
}

fn cubic_bound(signal: f32, octaves: usize, gain: f32) -> f32 {
    let mut amplitude = 1.0;
    let mut value = signal;
    for _ in 1..octaves {
        amplitude *= gain;
        value += signal * amplitude;
    }
    value
}

fn ridge_bound(config: SourceConfig) -> f32 {
    let mut value = 0.0;
    let mut weight = 1.0;
    let mut amplitude = 2.0;
    let mut spectral_frequency = 1.0_f32;
    for _ in 0..config.octaves {
        let mut noise = 1.0;
        noise *= noise;
        noise *= weight;
        weight = clamp(noise * amplitude, 0.0, 1.0);
        value += noise * spectral_weight(spectral_frequency);
        spectral_frequency *= config.lacunarity;
        amplitude *= config.gain;
    }
    value
}

/// `RidgeNoise.spectralWeights[octave]`: Java's `(float) Math.pow(frequency, -1.0)`, which is the correctly rounded
/// `1 / frequency` (checked over 24,030 lacunarity/octave pairs). Not `powf(-1.0)`: libm's `powf` differs in the last
/// bit for some inputs, and LLVM folds it to a division only in optimized builds. Java's table has 30 entries, so
/// Engine throws for more than 30 octaves; this port keeps going instead.
fn spectral_weight(frequency: f32) -> f32 {
    1.0 / frequency
}

/// `NoiseUtil.clamp`: NaN passes through.
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `NoiseUtil.seed(x, z)`: `x` in the low half, `z` in the high half (the reverse of `PosUtil.pack`).
pub fn pack_seed(x: i32, z: i32) -> i64 {
    (i64::from(z) << 32) | (i64::from(x) & 0xffff_ffff)
}

/// `NoiseUtil.curve(t, mid, steepness)`.
pub fn noise_curve(value: f32, midpoint: f32, steepness: f32) -> f32 {
    1.0 / (1.0 + noise_exp(-steepness * (value - midpoint)))
}

/// `NoiseUtil.exp`: `(1 + x / 256)^256` by eight squarings, not `Math.exp`.
fn noise_exp(mut value: f32) -> f32 {
    value = 1.0 + value / 256.0;
    for _ in 0..8 {
        value *= value;
    }
    value
}

/// `NoiseUtil.map(value, min, max, range)` as the jar's Engine 0.3.0 has it.
pub fn map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let clamped = clamp(value, min, max);
    let difference = clamped - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

/// The newer Noise2D's `NoiseUtil.map` (commit 2cb4843): `dif >= range ? 1 : dif / range`. It differs
/// from the jar's [`map`] only where the difference equals the range and the division is not 1: a zero
/// range (0/0) or an infinite one (inf/inf) give 1 here and NaN there.
pub fn map_current(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let difference = clamp(value, min, max) - min;
    if difference >= range {
        1.0
    } else {
        difference / range
    }
}

/// Engine's `Rand` source: white noise at `frequency`, with `nextInt` kept exactly as Java computes it.
#[derive(Clone, Copy)]
pub struct Rand {
    seed: i32,
    frequency: f32,
}

impl Rand {
    pub fn new(seed: i32, frequency: f32) -> Self {
        Self { seed, frequency }
    }

    pub fn value(self, mut x: f32, mut y: f32) -> f32 {
        x *= self.frequency;
        y *= self.frequency;
        white(x, y, self.seed).abs()
    }

    pub fn value_seeded(self, x: f32, y: f32, child_seed: i32) -> f32 {
        white(x, y, hash(self.seed, child_seed))
    }

    pub fn next_int(self, x: f32, y: f32, range: i32) -> i32 {
        round(range as f32 * self.value(x, y) / range.wrapping_add(range) as f32)
    }

    pub fn next_int_seeded(self, x: f32, y: f32, child_seed: i32, range: i32) -> i32 {
        round(range as f32 * self.value_seeded(x, y, child_seed) / range.wrapping_add(range) as f32)
    }
}

fn white(x: f32, y: f32, seed: i32) -> f32 {
    value_coord_2d(seed, round(x), round(y))
}

pub fn round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

/// `NoiseUtil.hash(x, y)`, in Java `int` math.
pub fn hash(x: i32, y: i32) -> i32 {
    let mut hash = x ^ 31337_i32.wrapping_mul(y);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60493);
    (hash >> 13) ^ hash
}

/// A node of Engine's `Module` graph. Cloning shares the node, as Java shares module instances between graphs.
/// Each node keeps the `minValue`/`maxValue` it reports, so samples never walk the graph to read bounds.
#[derive(Clone)]
pub struct Module(Arc<Node>);

struct Node {
    kind: ModuleKind,
    min: f32,
    max: f32,
}

/// Engine's `Line` source: a segment from (x1, y1) to (x2, y2) whose value falls from 1 on the segment to 0 at its
/// `radius` (sampled per point, as a squared distance), narrowed near its ends by `fade_in`/`fade_out` (fractions of
/// the squared length) and scaled by `feather`. As a module it reports the `Noise` defaults 0..1.
#[derive(Clone)]
pub struct Line {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    dx: f32,
    dy: f32,
    orth_x1: f32,
    orth_y1: f32,
    orth_x2: f32,
    orth_y2: f32,
    length2: f32,
    feather_bias: f32,
    feather_scale: f32,
    fade_in: Module,
    fade_out: Module,
    radius: Module,
}

/// A `MultiBlend` slot: its source and the `[min, max]` of the selector range where it is used alone.
struct BlendNode {
    source: Module,
    min: f32,
    max: f32,
}

/// A `Terrace` step: its output value and the `[lower_bound, upper_bound]` window where it is flat.
struct TerraceStep {
    value: f32,
    lower_bound: f32,
    upper_bound: f32,
}

/// The coordinate and sample a `Cache` holds (Engine's `Cache.Value`; `None` is its `empty` state). `seed` is the
/// runtime seed of a seed-first sample (`try_value_seed_first`) and `None` for the seedless `value`, so the two paths
/// (which use different `NoiseUtil.map`s) and two runtime seeds never share an entry.
#[derive(Clone, Copy)]
struct CacheValue {
    seed: Option<i32>,
    x: f32,
    y: f32,
    value: f32,
}

enum ModuleKind {
    Constant(f32),
    Custom(Arc<dyn Fn(f32, f32) -> f32 + Send + Sync + std::panic::RefUnwindSafe>),
    Source(Source),
    ErosionRidge {
        ridge: ErosionRidge,
        source: Module,
    },
    CellSource {
        config: SourceConfig,
        displacement: f32,
        function: CellFunction,
        distance: DistanceFunction,
        lookup: Module,
    },
    CellEdgeSource {
        config: SourceConfig,
        displacement: f32,
        edge: EdgeFunction,
        distance: DistanceFunction,
    },
    SineSource {
        frequency: f32,
        alpha: Module,
    },
    Line(Line),
    Abs(Module),
    Alpha(Module, Module),
    Bias(Module, Module),
    Boost(Module, usize),
    Clamp(Module, Module, Module),
    Invert(Module),
    /// `Map` keeps `sourceRange = source.maxValue() - source.minValue()` from its constructor.
    Map(Module, Module, Module, f32),
    Power(Module, Module),
    Scale(Module, Module),
    Threshold(Module, Module),
    Warp(Module, Domain),
    /// `Base`: `min` is `base.maxValue()` and `max` is `min + falloff`, both fixed in the constructor.
    Base {
        base: Module,
        source: Module,
        min: f32,
        max: f32,
        falloff: f32,
        interpolation: Interpolation,
    },
    /// `Blend`: the blend window is fixed from the control's bounds in the constructor; `midpoint` and `blend` are
    /// the constructor's arguments, which reseeding passes again.
    Blend {
        control: Module,
        lower: Module,
        upper: Module,
        midpoint: f32,
        blend: f32,
        blend_lower: f32,
        blend_upper: f32,
        blend_range: f32,
        interpolation: Interpolation,
    },
    VariableBlend {
        control: Module,
        variable: Module,
        lower: Module,
        upper: Module,
        midpoint: f32,
        min_blend: f32,
        max_blend: f32,
        interpolation: Interpolation,
    },
    /// `MultiBlend`: the slots, `maxIndex` and `blendRange`, fixed in the constructor.
    MultiBlend {
        control: Module,
        nodes: Vec<BlendNode>,
        max_index: i32,
        blend: f32,
        blend_range: f32,
        interpolation: Interpolation,
    },
    /// `Terrace`: the steps and `maxIndex`, fixed in the constructor.
    Terrace {
        source: Module,
        ramp: Module,
        cliff: Module,
        ramp_height: Module,
        steps: Vec<TerraceStep>,
        max_index: i32,
        blend: f32,
    },
    /// `LegacyTerrace`: the steps and `maxIndex`, fixed in the constructor.
    LegacyTerrace {
        source: Module,
        lower_curve: Module,
        upper_curve: Module,
        steps: Vec<TerraceStep>,
        max_index: i32,
        blend: f32,
    },
    /// `AdvancedTerrace`: `blendRange` (`blendMax - blendMin`) and `modRange` (`source.maxValue() +
    /// modulation.maxValue()`), fixed in the constructor.
    AdvancedTerrace {
        source: Module,
        modulation: Module,
        mask: Module,
        slope: Module,
        blend_min: f32,
        blend_max: f32,
        blend_range: f32,
        mod_range: f32,
        steps: i32,
        octaves: i32,
    },
    /// `Curve`: a fixed `CurveFunc` applied to the source sample.
    Curve {
        source: Module,
        function: CurveFunction,
    },
    /// `VariableCurve`: `NoiseUtil.curve` with a sampled midpoint and steepness (`gradient`).
    VariableCurve {
        source: Module,
        midpoint: Module,
        gradient: Module,
    },
    /// `PowerCurve`: `min`, `max`, `midpoint` and `range` fixed from the source's bounds in the constructor.
    PowerCurve {
        source: Module,
        min: f32,
        max: f32,
        midpoint: f32,
        range: f32,
        power: f32,
    },
    /// `Steps`: step count and slope window sampled per point.
    Steps {
        source: Module,
        steps: Module,
        slope_min: Module,
        slope_max: Module,
        curve: CurveFunction,
    },
    /// `Freq`: scales the coordinates by `x` and `y` before sampling its source.
    Frequency {
        source: Module,
        x: Module,
        y: Module,
    },
    /// `Grad`: raises samples at or below `upper` to a power that falls towards `1 - strength` at `lower`.
    Gradient {
        source: Module,
        lower: Module,
        upper: Module,
        strength: Module,
    },
    /// `Modulate`: offsets the coordinates by `strength` along `direction` (in turns), through `NoiseUtil`'s sine table.
    Modulate {
        source: Module,
        direction: Module,
        strength: Module,
    },
    /// `Cache`: Engine's single shared entry, behind a lock that is only ever tried, never waited for.
    Cache {
        source: Module,
        value: std::sync::Mutex<Option<CacheValue>>,
    },
    /// `Select`: `lower_curve`/`upper_curve` hold Engine's `(curveMin, curveMax, curveRange)` per bound, fixed in the
    /// constructor.
    Select {
        control: Module,
        lower: Module,
        upper: Module,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        lower_curve: (f32, f32, f32),
        upper_curve: (f32, f32, f32),
        interpolation: Interpolation,
    },
    Add(Vec<Module>),
    Multiply(Vec<Module>),
    Max(Vec<Module>),
    Min(Vec<Module>),
    Subtract(Vec<Module>),
}

impl Line {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        radius: Module,
        fade_in: Module,
        fade_out: Module,
        feather: f32,
    ) -> Self {
        let dx = x2 - x1;
        let dy = y2 - y1;
        Self {
            x1,
            y1,
            x2,
            y2,
            dx,
            dy,
            orth_x1: x1 + (y2 - y1),
            orth_y1: y1 + (x1 - x2),
            orth_x2: x2 + (y2 - y1),
            orth_y2: y2 + (x1 - x2),
            length2: dx * dx + dy * dy,
            feather_bias: 1.0 - feather,
            feather_scale: feather,
            fade_in,
            fade_out,
            radius,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        self.value_with_width(x, y, self.width_modifier(x, y))
    }

    pub fn value_with_width(&self, x: f32, y: f32, width_modifier: f32) -> f32 {
        self.value_with_min_width(x, y, 0.0, width_modifier)
    }

    pub fn value_with_min_width(
        &self,
        x: f32,
        y: f32,
        min_width2: f32,
        width_modifier: f32,
    ) -> f32 {
        let distance2 = self.distance2(x, y);
        let radius2 = min_width2 + self.radius.value(x, y) * width_modifier;
        if distance2 > radius2 {
            return 0.0;
        }
        let value = distance2 / radius2;
        if self.feather_scale == 0.0 {
            return 1.0 - value;
        }
        let feather = self.feather_bias + width_modifier * self.feather_scale;
        (1.0 - value) * feather
    }

    pub fn clip_start(&self, x: f32, y: f32) -> bool {
        Self::sign(x, y, self.x1, self.y1, self.orth_x1, self.orth_y1) > 0
    }

    pub fn clip_end(&self, x: f32, y: f32) -> bool {
        Self::sign(x, y, self.x2, self.y2, self.orth_x2, self.orth_y2) < 0
    }

    pub fn width_modifier(&self, x: f32, y: f32) -> f32 {
        let distance1 = Self::dist2(x, y, self.x1, self.y1);
        if distance1 == 0.0 {
            return 0.0;
        }
        let distance2 = Self::dist2(x, y, self.x2, self.y2);
        if distance2 == 0.0 {
            return 0.0;
        }
        let mut fade = 1.0;
        let fade_in = self.fade_in.value(x, y);
        let fade_out = self.fade_out.value(x, y);
        if fade_in > 0.0 {
            let distance = fade_in * self.length2;
            if distance1 < distance {
                fade *= distance1 / distance;
            }
        }
        if fade_out > 0.0 {
            let distance = fade_out * self.length2;
            if distance2 < distance {
                fade *= distance2 / distance;
            }
        }
        fade
    }

    fn distance2(&self, x: f32, y: f32) -> f32 {
        let projection = (x - self.x1) * self.dx + (y - self.y1) * self.dy;
        let scale = clamp(projection / self.length2, 0.0, 1.0);
        let intersection_x = self.x1 + scale * self.dx;
        let intersection_y = self.y1 + scale * self.dy;
        Self::dist2(x, y, intersection_x, intersection_y)
    }

    pub fn dist2(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
        let dx = x2 - x1;
        let dy = y2 - y1;
        dx * dx + dy * dy
    }

    pub fn sign(x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> i32 {
        let value = (x - x1) * (y2 - y1) - (y - y1) * (x2 - x1);
        if value == 0.0 {
            0
        } else if value < 0.0 {
            -1
        } else {
            1
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn intersect(
        ax1: f32,
        ay1: f32,
        ax2: f32,
        ay2: f32,
        bx1: f32,
        by1: f32,
        bx2: f32,
        by2: f32,
    ) -> bool {
        Self::relative_ccw(ax1, ay1, ax2, ay2, bx1, by1)
            * Self::relative_ccw(ax1, ay1, ax2, ay2, bx2, by2)
            <= 0
            && Self::relative_ccw(bx1, by1, bx2, by2, ax1, ay1)
                * Self::relative_ccw(bx1, by1, bx2, by2, ax2, ay2)
                <= 0
    }

    pub fn distance_on_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let dx = bx - ax;
        let dy = by - ay;
        ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)
    }

    /// `Line.distance2Line`: -1 when the projection falls outside the segment. Not `!(0.0..=1.0).contains(..)`:
    /// a NaN projection (a zero-length segment) falls through to a NaN distance, as in Engine.
    #[allow(clippy::manual_range_contains)]
    pub fn distance2_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let dx = bx - ax;
        let dy = by - ay;
        let projection = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy);
        if projection < 0.0 || projection > 1.0 {
            return -1.0;
        }
        Self::dist2(x, y, ax + dx * projection, ay + dy * projection)
    }

    pub fn distance2_line_including_ends(
        x: f32,
        y: f32,
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
    ) -> f32 {
        let dx = bx - ax;
        let dy = by - ay;
        let projection = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy);
        if projection < 0.0 {
            Self::dist2(x, y, ax, ay)
        } else if projection > 1.0 {
            Self::dist2(x, y, bx, by)
        } else {
            Self::dist2(x, y, ax + dx * projection, ay + dy * projection)
        }
    }

    fn relative_ccw(x1: f32, y1: f32, mut x2: f32, mut y2: f32, mut px: f32, mut py: f32) -> i32 {
        x2 -= x1;
        y2 -= y1;
        px -= x1;
        py -= y1;
        let mut ccw = f64::from(px * y2 - py * x2);
        if ccw == 0.0 {
            ccw = f64::from(px * x2 + py * y2);
            if ccw > 0.0 {
                px -= x2;
                py -= y2;
                ccw = f64::from(px * x2 + py * y2);
                if ccw < 0.0 {
                    ccw = 0.0;
                }
            }
        }
        if ccw < 0.0 {
            -1
        } else if ccw > 0.0 {
            1
        } else {
            0
        }
    }
}

impl Module {
    fn node(kind: ModuleKind, min: f32, max: f32) -> Self {
        Self(Arc::new(Node { kind, min, max }))
    }

    /// `Modifier`'s default bounds: those of its source.
    fn modifier(kind: ModuleKind, source: &Self) -> Self {
        Self::node(kind, source.min_value(), source.max_value())
    }

    /// `Source.constant(double)`: 0 (either sign), 0.5 and 1 give the shared `ZERO`, `HALF` and `ONE`, so -0.0
    /// becomes +0.0; any other value is `new Constant((float) value)`. (Java compares the double before its cast,
    /// so a negative double too small for a float, such as -1e-50, keeps -0.0f there; callers pass the float.)
    pub fn constant(value: f32) -> Self {
        let value = if value == 0.0 { 0.0 } else { value };
        Self::node(ModuleKind::Constant(value), value, value)
    }

    /// `new Constant(float)`, which keeps -0.0 (only Java oracles in the tests build these).
    #[cfg(test)]
    fn new_constant(value: f32) -> Self {
        Self::node(ModuleKind::Constant(value), value, value)
    }

    /// A Java class implementing `Module` itself (such as the climate layer's `Temperature`): `value` is its
    /// `getValue`, `min`/`max` its `minValue`/`maxValue` (the `Noise` defaults 0..1 unless the class overrides them).
    pub fn custom(
        min: f32,
        max: f32,
        value: impl Fn(f32, f32) -> f32 + Send + Sync + std::panic::RefUnwindSafe + 'static,
    ) -> Self {
        Self::node(ModuleKind::Custom(Arc::new(value)), min, max)
    }

    pub fn source(source: Source) -> Self {
        Self::node(
            ModuleKind::Source(source),
            source.min_value(),
            source.max_value(),
        )
    }

    /// Engine's `Ridge.Noise` (`new Ridge(...).wrap(source)`, `engine.module.Ridge`, not the `RidgeNoise` source):
    /// `ridge` erodes `source`. Like every Engine `Noise`, it reports the defaults 0..1.
    pub fn erosion_ridge(ridge: ErosionRidge, source: Self) -> Self {
        Self::node(ModuleKind::ErosionRidge { ridge, source }, 0.0, 1.0)
    }

    /// Engine's `CellNoise` (`Builder.cell()`): `config` gives the seed and frequency (the octave settings are
    /// unused), `displacement` is the builder's `displacement`. `CellValue` maps the nearest cell's value from -1..1
    /// to 0..1, `NoiseLookup` samples `lookup` unmapped at the nearest cell's feature point (unscaled by
    /// `displacement`), and `Distance` always returns 0. As in Engine it reports the `Noise` defaults 0..1 whatever
    /// the function or `lookup`'s bounds. Where Engine throws (`NoiseLookup` when no cell's distance is below
    /// `Float.MAX_VALUE`: NaN or infinite coordinates, or squared distances that overflow), this panics.
    pub fn cell_source(
        config: SourceConfig,
        displacement: f32,
        function: CellFunction,
        distance: DistanceFunction,
        lookup: Self,
    ) -> Self {
        Self::node(
            ModuleKind::CellSource {
                config,
                displacement,
                function,
                distance,
                lookup,
            },
            0.0,
            1.0,
        )
    }

    /// Engine's `CellEdgeNoise` (`Builder.cellEdge()`): `edge` of the two nearest cells' distances, mapped to 0..1
    /// from the edge function's bounds; it reports 0..1.
    pub fn cell_edge_source(
        config: SourceConfig,
        displacement: f32,
        edge: EdgeFunction,
        distance: DistanceFunction,
    ) -> Self {
        Self::node(
            ModuleKind::CellEdgeSource {
                config,
                displacement,
                edge,
                distance,
            },
            0.0,
            1.0,
        )
    }

    /// `Sin` (`Builder.sin()`, with `frequency` and `source` as `alpha`): reports the `Noise` defaults 0..1.
    pub fn sine_source(frequency: f32, alpha: Self) -> Self {
        Self::node(ModuleKind::SineSource { frequency, alpha }, 0.0, 1.0)
    }

    /// `Line` as a module: reports the `Noise` defaults 0..1.
    pub fn line(line: Line) -> Self {
        Self::node(ModuleKind::Line(line), 0.0, 1.0)
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        match &self.0.kind {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Custom(value) => value(x, y),
            ModuleKind::Source(source) => source.value(x, y),
            ModuleKind::ErosionRidge { ridge, source } => ridge.value(x, y, source),
            ModuleKind::CellSource {
                config,
                displacement,
                function,
                distance,
                lookup,
            } => {
                let nearest = || {
                    let (x, y) = (x * config.frequency, y * config.frequency);
                    nearest_cell(x, y, config.seed, *displacement, *distance)
                };
                match function {
                    CellFunction::CellValue => {
                        let cell = nearest();
                        map(value_coord_2d(config.seed, cell.x, cell.y), -1.0, 1.0, 2.0)
                    }
                    CellFunction::NoiseLookup => {
                        let cell = nearest();
                        let Some(vector) = cell.vector else {
                            panic!(
                                "Engine throws a NullPointerException: no cell is nearer than Float.MAX_VALUE"
                            );
                        };
                        lookup.value(cell.x as f32 + vector[0], cell.y as f32 + vector[1])
                    }
                    // `CellFunc.DISTANCE.mapValue` always returns 0; its (side-effect free) search is skipped.
                    CellFunction::Distance => 0.0,
                }
            }
            ModuleKind::CellEdgeSource {
                config,
                displacement,
                edge,
                distance,
            } => {
                let value = cell_edge(
                    x * config.frequency,
                    y * config.frequency,
                    config.seed,
                    *displacement,
                    *edge,
                    *distance,
                );
                let (min, max, range) = edge.bounds();
                map(value, min, max, range)
            }
            ModuleKind::SineSource { frequency, alpha } => {
                let alpha = alpha.value(x, y);
                let x = x * *frequency;
                let y = y * *frequency;
                let noise = if alpha == 0.0 {
                    sin(x)
                } else if alpha == 1.0 {
                    sin(y)
                } else {
                    lerp(sin(x), sin(y), alpha)
                };
                map(noise, -1.0, 1.0, 2.0)
            }
            ModuleKind::Line(line) => line.value(x, y),
            ModuleKind::Abs(source) => source.value(x, y).abs(),
            ModuleKind::Alpha(source, alpha) => {
                let value = source.value(x, y);
                let alpha = alpha.value(x, y);
                value * alpha + (1.0 - alpha)
            }
            ModuleKind::Bias(source, bias) => source.value(x, y) + bias.value(x, y),
            ModuleKind::Boost(source, iterations) => {
                let mut value = source.value(x, y);
                for _ in 0..*iterations {
                    value = java_pow(value, 1.0 - value);
                }
                value
            }
            ModuleKind::Clamp(source, min, max) => {
                let value = source.value(x, y);
                clamp(value, min.value(x, y), max.value(x, y))
            }
            ModuleKind::Invert(source) => {
                let value = source.value(x, y);
                if value > source.max_value() {
                    source.min_value()
                } else if value < source.min_value() {
                    source.max_value()
                } else {
                    source.max_value() - value
                }
            }
            ModuleKind::Map(source, min, max, range) => {
                let alpha = (source.value(x, y) - source.min_value()) / range;
                let min = min.value(x, y);
                let max = max.value(x, y);
                min + alpha * (max - min)
            }
            ModuleKind::Power(source, power) => {
                let value = source.value(x, y);
                java_pow(value, power.value(x, y))
            }
            ModuleKind::Scale(source, scale) => source.value(x, y) * scale.value(x, y),
            ModuleKind::Threshold(source, threshold) => {
                let value = source.value(x, y);
                if value < threshold.value(x, y) {
                    0.0
                } else {
                    1.0
                }
            }
            ModuleKind::Warp(source, domain) => source.value(domain.x(x, y), domain.y(x, y)),
            ModuleKind::Base {
                base,
                source,
                min,
                max,
                falloff,
                interpolation,
            } => {
                let upper = source.value(x, y);
                if upper < *max {
                    let lower = base.value(x, y);
                    if *falloff > 0.0 {
                        let alpha = (*max - java_max(*min, upper)) / *falloff;
                        lerp(upper, lower, interpolation.apply(alpha))
                    } else {
                        lower
                    }
                } else {
                    upper
                }
            }
            ModuleKind::Blend {
                control,
                lower,
                upper,
                blend_lower,
                blend_upper,
                blend_range,
                interpolation,
                ..
            } => {
                let select = control.value(x, y);
                if select < *blend_lower {
                    lower.value(x, y)
                } else if select > *blend_upper {
                    upper.value(x, y)
                } else {
                    let alpha = (select - *blend_lower) / *blend_range;
                    lerp(
                        lower.value(x, y),
                        upper.value(x, y),
                        interpolation.apply(alpha),
                    )
                }
            }
            ModuleKind::VariableBlend {
                control,
                variable,
                lower,
                upper,
                midpoint,
                min_blend,
                max_blend,
                interpolation,
            } => {
                let select = control.value(x, y);
                let radius = *min_blend + variable.value(x, y) * *max_blend;
                let min = java_max(0.0, *midpoint - radius);
                if select < min {
                    return lower.value(x, y);
                }
                let max = java_min(1.0, *midpoint + radius);
                if select > max {
                    return upper.value(x, y);
                }
                let alpha = (select - min) / (max - min);
                lerp(
                    lower.value(x, y),
                    upper.value(x, y),
                    interpolation.apply(alpha),
                )
            }
            ModuleKind::MultiBlend {
                control,
                nodes,
                max_index,
                blend_range,
                interpolation,
                ..
            } => {
                let selector = control.value(x, y);
                let index = round(selector * *max_index as f32);
                let mut min = multi_blend_slot(nodes, Some(index));
                let mut max = min;
                if *blend_range == 0.0 {
                    return min.source.value(x, y);
                }
                if selector > min.max {
                    max = multi_blend_slot(nodes, index.checked_add(1));
                } else if selector < min.min {
                    min = multi_blend_slot(nodes, index.checked_sub(1));
                } else {
                    return min.source.value(x, y);
                }
                let alpha = clamp((selector - min.max) / *blend_range, 0.0, 1.0);
                lerp(
                    min.source.value(x, y),
                    max.source.value(x, y),
                    interpolation.apply(alpha),
                )
            }
            ModuleKind::Terrace {
                source,
                ramp,
                cliff,
                ramp_height,
                steps,
                max_index,
                ..
            } => {
                let noise_value = clamp(source.value(x, y), 0.0, 0.999999);
                let index = floor(noise_value * steps.len() as f32);
                // Only a NaN sample (index -1) or a terrace without steps leaves the array, where Engine throws.
                let step = usize::try_from(index)
                    .ok()
                    .and_then(|index| steps.get(index))
                    .expect("Terrace samples a step outside its steps, where Engine throws");
                if index == *max_index || noise_value < step.lower_bound {
                    return step.value;
                }
                if noise_value > step.upper_bound {
                    return steps[index as usize + 1].value;
                }
                let ramp = 1.0 - ramp.value(x, y) * 0.5;
                let cliff = 1.0 - cliff.value(x, y) * 0.5;
                let alpha =
                    (noise_value - step.lower_bound) / (step.upper_bound - step.lower_bound);
                let mut value = step.value;
                if alpha > ramp {
                    let next = &steps[index as usize + 1];
                    let ramp_size = 1.0 - ramp;
                    let ramp_alpha = (alpha - ramp) / ramp_size;
                    value += (next.value - value) * ramp_alpha * ramp_height.value(x, y);
                }
                if alpha > cliff {
                    let next = &steps[index as usize + 1];
                    let cliff_alpha = (alpha - cliff) / (1.0 - cliff);
                    value = lerp(value, next.value, cliff_alpha);
                }
                value
            }
            ModuleKind::LegacyTerrace {
                source,
                lower_curve,
                upper_curve,
                steps,
                max_index,
                ..
            } => {
                let Ok(value) = legacy_terrace(
                    source.value(x, y),
                    steps,
                    *max_index,
                    || Ok::<_, std::convert::Infallible>(lower_curve.value(x, y)),
                    || Ok(upper_curve.value(x, y)),
                );
                value
            }
            ModuleKind::AdvancedTerrace {
                source,
                modulation,
                mask,
                slope,
                blend_min,
                blend_max,
                blend_range,
                mod_range,
                steps,
                octaves,
            } => {
                let Ok(value) = advanced_terrace(
                    source.value(x, y),
                    [*blend_min, *blend_max, *blend_range, *mod_range],
                    *steps,
                    *octaves,
                    || Ok::<_, std::convert::Infallible>(mask.value(x, y)),
                    || Ok(slope.value(x, y)),
                    || Ok(modulation.value(x, y)),
                );
                value
            }
            ModuleKind::Curve { source, function } => function.apply(source.value(x, y)),
            ModuleKind::VariableCurve {
                source,
                midpoint,
                gradient,
            } => noise_curve(
                source.value(x, y),
                midpoint.value(x, y),
                gradient.value(x, y),
            ),
            ModuleKind::PowerCurve {
                source,
                min,
                max,
                midpoint,
                range,
                power,
            } => map(
                power_curve_value(source.value(x, y), *midpoint, *power),
                *min,
                *max,
                *range,
            ),
            ModuleKind::Steps { .. }
            | ModuleKind::Frequency { .. }
            | ModuleKind::Gradient { .. }
            | ModuleKind::Modulate { .. } => {
                let Ok(value) = transform_value(&self.0.kind, x, y, map, |module, x, y| {
                    Ok::<_, std::convert::Infallible>(module.value(x, y))
                });
                value
            }
            // Engine keeps one unsynchronised entry per `Cache`. Horizon samples one graph from many threads, so the
            // entry sits behind a lock that is only held to copy it in or out, never while the source is sampled, and
            // a busy lock just skips the cache: threads never wait on each other here and a panicking source cannot
            // poison it. The key compares bits rather than Java's `==`, so a hit only ever returns the sample of the
            // very same input; with `==`, -0.0 would hit +0.0's entry (and NaN never hits), which could make a sample
            // depend on which coordinate another thread sampled last.
            ModuleKind::Cache { source, value } => {
                let Ok(value) = cache_lookup(value, None, x, y, || {
                    Ok::<_, std::convert::Infallible>(source.value(x, y))
                });
                value
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                falloff,
                lower_curve: (lower_min, lower_max, lower_range),
                upper_curve: (upper_min, upper_max, upper_range),
                interpolation,
                ..
            } => {
                let value = control.value(x, y);
                if *falloff == 0.0 {
                    // Engine's hard edge: only values above `upperBound` pick the upper source.
                    if value < *lower_max {
                        lower.value(x, y)
                    } else if value > *upper_min {
                        upper.value(x, y)
                    } else {
                        lower.value(x, y)
                    }
                } else if value < *lower_min {
                    lower.value(x, y)
                } else if value < *lower_max {
                    let alpha = (value - *lower_min) / *lower_range;
                    lerp(
                        lower.value(x, y),
                        upper.value(x, y),
                        interpolation.apply(alpha),
                    )
                } else if value < *upper_min {
                    upper.value(x, y)
                } else if value < *upper_max {
                    let alpha = (value - *upper_min) / *upper_range;
                    lerp(
                        upper.value(x, y),
                        lower.value(x, y),
                        interpolation.apply(alpha),
                    )
                } else {
                    lower.value(x, y)
                }
            }
            ModuleKind::Add(sources) => combine(sources, x, y, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine(sources, x, y, |a, b| a * b),
            ModuleKind::Max(sources) => combine(sources, x, y, java_max),
            ModuleKind::Min(sources) => combine(sources, x, y, java_min),
            ModuleKind::Subtract(sources) => combine(sources, x, y, |a, b| a - b),
        }
    }

    /// Noise2D 2cb4843's `getValue(int seed, float x, float y)` for the scalar nodes, `Warp`, the `Base`,
    /// `Blend`, `VariableBlend`, `Select` and `MultiBlend` selectors, `Cache`, `Terrace` and `LegacyTerrace`: every node passes the
    /// call's seed unchanged to its children (only octave sources add their own seed, see
    /// [`Source::value_seed_first`]); their arithmetic and evaluation order are the jar's (`Cache` also keys its
    /// entry by the seed). `Warp` samples its source at the domain's seed-first coordinates ([`Domain::try_x_seed_first`]).
    /// Nodes not yet ported to seed-first sampling return an error instead of falling back to [`Module::value`],
    /// which would ignore the seed. Selectors and terraces evaluate only the children Java evaluates for that
    /// sample (a terrace reads its ramp and cliff only inside a step's window, its ramp height only above the ramp;
    /// a legacy terrace reads one curve, and only outside a step's window),
    /// so an unsupported node in a branch the sample skips errors only at the points that reach it: one `Ok` sample
    /// does not prove the whole graph is supported. Where Java throws (a `MultiBlend` slot outside its sources, a
    /// NaN terrace sample, a terrace or legacy terrace without steps), this panics, as [`Module::value`] does.
    pub fn try_value_seed_first(&self, seed: i32, x: f32, y: f32) -> Result<f32, &'static str> {
        let sample = |module: &Module| module.try_value_seed_first(seed, x, y);
        Ok(match &self.0.kind {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.value_seed_first(seed, x, y),
            ModuleKind::Warp(source, domain) => source.try_value_seed_first(
                seed,
                domain.try_x_seed_first(seed, x, y)?,
                domain.try_y_seed_first(seed, x, y)?,
            )?,
            ModuleKind::Abs(source) => sample(source)?.abs(),
            ModuleKind::Alpha(source, alpha) => {
                let value = sample(source)?;
                let alpha = sample(alpha)?;
                value * alpha + (1.0 - alpha)
            }
            ModuleKind::Bias(source, bias) => sample(source)? + sample(bias)?,
            ModuleKind::Clamp(source, min, max) => {
                let value = sample(source)?;
                clamp(value, sample(min)?, sample(max)?)
            }
            ModuleKind::Invert(source) => {
                let value = sample(source)?;
                if value > source.max_value() {
                    source.min_value()
                } else if value < source.min_value() {
                    source.max_value()
                } else {
                    source.max_value() - value
                }
            }
            ModuleKind::Map(source, min, max, range) => {
                let alpha = (sample(source)? - source.min_value()) / range;
                let min = sample(min)?;
                let max = sample(max)?;
                min + alpha * (max - min)
            }
            ModuleKind::Power(source, power) => {
                let value = sample(source)?;
                java_pow(value, sample(power)?)
            }
            ModuleKind::Scale(source, scale) => sample(source)? * sample(scale)?,
            ModuleKind::Threshold(source, threshold) => {
                let value = sample(source)?;
                if value < sample(threshold)? { 0.0 } else { 1.0 }
            }
            ModuleKind::Add(sources) => try_combine(sources, seed, x, y, |a, b| a + b)?,
            ModuleKind::Multiply(sources) => try_combine(sources, seed, x, y, |a, b| a * b)?,
            ModuleKind::Max(sources) => try_combine(sources, seed, x, y, java_max)?,
            ModuleKind::Min(sources) => try_combine(sources, seed, x, y, java_min)?,
            ModuleKind::Subtract(sources) => try_combine(sources, seed, x, y, |a, b| a - b)?,
            ModuleKind::Base {
                base,
                source,
                min,
                max,
                falloff,
                interpolation,
            } => {
                let upper = sample(source)?;
                if upper < *max {
                    let lower = sample(base)?;
                    if *falloff > 0.0 {
                        let alpha = (*max - java_max(*min, upper)) / *falloff;
                        lerp(upper, lower, interpolation.apply(alpha))
                    } else {
                        lower
                    }
                } else {
                    upper
                }
            }
            ModuleKind::Blend {
                control,
                lower,
                upper,
                blend_lower,
                blend_upper,
                blend_range,
                interpolation,
                ..
            } => {
                let select = sample(control)?;
                if select < *blend_lower {
                    sample(lower)?
                } else if select > *blend_upper {
                    sample(upper)?
                } else {
                    let alpha = (select - *blend_lower) / *blend_range;
                    lerp(sample(lower)?, sample(upper)?, interpolation.apply(alpha))
                }
            }
            ModuleKind::VariableBlend {
                control,
                variable,
                lower,
                upper,
                midpoint,
                min_blend,
                max_blend,
                interpolation,
            } => {
                let select = sample(control)?;
                let radius = *min_blend + sample(variable)? * *max_blend;
                let min = java_max(0.0, *midpoint - radius);
                if select < min {
                    return sample(lower);
                }
                let max = java_min(1.0, *midpoint + radius);
                if select > max {
                    return sample(upper);
                }
                let alpha = (select - min) / (max - min);
                lerp(sample(lower)?, sample(upper)?, interpolation.apply(alpha))
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                falloff,
                lower_curve: (lower_min, lower_max, lower_range),
                upper_curve: (upper_min, upper_max, upper_range),
                interpolation,
                ..
            } => {
                let value = sample(control)?;
                if *falloff == 0.0 {
                    if value < *lower_max {
                        sample(lower)?
                    } else if value > *upper_min {
                        sample(upper)?
                    } else {
                        sample(lower)?
                    }
                } else if value < *lower_min {
                    sample(lower)?
                } else if value < *lower_max {
                    let alpha = (value - *lower_min) / *lower_range;
                    lerp(sample(lower)?, sample(upper)?, interpolation.apply(alpha))
                } else if value < *upper_min {
                    sample(upper)?
                } else if value < *upper_max {
                    let alpha = (value - *upper_min) / *upper_range;
                    lerp(sample(upper)?, sample(lower)?, interpolation.apply(alpha))
                } else {
                    sample(lower)?
                }
            }
            ModuleKind::MultiBlend {
                control,
                nodes,
                max_index,
                blend_range,
                interpolation,
                ..
            } => {
                let selector = sample(control)?;
                let index = round(selector * *max_index as f32);
                let mut min = multi_blend_slot(nodes, Some(index));
                let mut max = min;
                if *blend_range == 0.0 {
                    return sample(&min.source);
                }
                if selector > min.max {
                    max = multi_blend_slot(nodes, index.checked_add(1));
                } else if selector < min.min {
                    min = multi_blend_slot(nodes, index.checked_sub(1));
                } else {
                    return sample(&min.source);
                }
                let alpha = clamp((selector - min.max) / *blend_range, 0.0, 1.0);
                lerp(
                    sample(&min.source)?,
                    sample(&max.source)?,
                    interpolation.apply(alpha),
                )
            }
            ModuleKind::Terrace {
                source,
                ramp,
                cliff,
                ramp_height,
                steps,
                max_index,
                ..
            } => {
                let noise_value = clamp(sample(source)?, 0.0, 0.999999);
                let index = floor(noise_value * steps.len() as f32);
                // Only a NaN sample (index -1) or a terrace without steps leaves the array, where Java throws.
                let step = usize::try_from(index)
                    .ok()
                    .and_then(|index| steps.get(index))
                    .expect("Terrace samples a step outside its steps, where Java throws");
                let index = index as usize;
                if index as i32 == *max_index || noise_value < step.lower_bound {
                    step.value
                } else if noise_value > step.upper_bound {
                    steps[index + 1].value
                } else {
                    let ramp = 1.0 - sample(ramp)? * 0.5;
                    let cliff = 1.0 - sample(cliff)? * 0.5;
                    let alpha =
                        (noise_value - step.lower_bound) / (step.upper_bound - step.lower_bound);
                    let mut value = step.value;
                    if alpha > ramp {
                        let next = &steps[index + 1];
                        let ramp_alpha = (alpha - ramp) / (1.0 - ramp);
                        value += (next.value - value) * ramp_alpha * sample(ramp_height)?;
                    }
                    if alpha > cliff {
                        let next = &steps[index + 1];
                        let cliff_alpha = (alpha - cliff) / (1.0 - cliff);
                        value = lerp(value, next.value, cliff_alpha);
                    }
                    value
                }
            }
            ModuleKind::LegacyTerrace {
                source,
                lower_curve,
                upper_curve,
                steps,
                max_index,
                ..
            } => legacy_terrace(
                sample(source)?,
                steps,
                *max_index,
                || sample(lower_curve),
                || sample(upper_curve),
            )?,
            ModuleKind::AdvancedTerrace {
                source,
                modulation,
                mask,
                slope,
                blend_min,
                blend_max,
                blend_range,
                mod_range,
                steps,
                octaves,
            } => advanced_terrace(
                sample(source)?,
                [*blend_min, *blend_max, *blend_range, *mod_range],
                *steps,
                *octaves,
                || sample(mask),
                || sample(slope),
                || sample(modulation),
            )?,
            ModuleKind::Curve { source, function } => function.apply(sample(source)?),
            ModuleKind::VariableCurve {
                source,
                midpoint,
                gradient,
            } => noise_curve(sample(source)?, sample(midpoint)?, sample(gradient)?),
            ModuleKind::PowerCurve {
                source,
                min,
                max,
                midpoint,
                range,
                power,
            } => {
                // Noise2D 2cb4843's `PowerCurve.modify` ends in its own `NoiseUtil.map`: 1, not NaN, where the
                // curve's range is zero (a constant source) or infinite.
                map_current(
                    power_curve_value(sample(source)?, *midpoint, *power),
                    *min,
                    *max,
                    *range,
                )
            }
            // Noise2D 2cb4843's `Steps.modify` maps with its own `NoiseUtil.map`: 1, not NaN, where the slope range
            // is infinite (a zero or negative one takes the rounding branch before the map).
            ModuleKind::Steps { .. }
            | ModuleKind::Frequency { .. }
            | ModuleKind::Gradient { .. }
            | ModuleKind::Modulate { .. } => {
                transform_value(&self.0.kind, x, y, map_current, |module, x, y| {
                    module.try_value_seed_first(seed, x, y)
                })?
            }
            // Noise2D 2cb4843's `Cache` keeps one entry per thread keyed by `x == this.x && y == this.y` alone, so a
            // repeated point at another runtime seed returns the first seed's sample. The port keys the shared entry
            // by the seed too (and by the coordinates' bits, as the seedless path does): a hit is always the sample
            // the uncached graph gives. A failed child stores nothing.
            ModuleKind::Cache { source, value } => {
                cache_lookup(value, Some(seed), x, y, || sample(source))?
            }
            _ => return Err("runtime-seeded sampling is not implemented for this graph node"),
        })
    }

    pub fn min_value(&self) -> f32 {
        self.0.min
    }

    pub fn max_value(&self) -> f32 {
        self.0.max
    }

    /// `Module.abs()`.
    pub fn abs(self) -> Self {
        if matches!(self.0.kind, ModuleKind::Abs(_)) {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Abs(self), &source)
        }
    }

    /// `Module.alpha(Module)`: skipped when the alpha's bounds leave 0..1.
    pub fn alpha(self, alpha: Self) -> Self {
        if alpha.min_value() < 0.0 || alpha.max_value() > 1.0 {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Alpha(self, alpha), &source)
        }
    }

    /// `Module.bias(Module)`: skipped for a zero bias.
    pub fn bias(self, bias: Self) -> Self {
        if bias.min_value() == 0.0 && bias.max_value() == 0.0 {
            self
        } else {
            let (min, max) = (
                self.min_value() + bias.min_value(),
                self.max_value() + bias.max_value(),
            );
            Self::node(ModuleKind::Bias(self, bias), min, max)
        }
    }

    /// `Module.boost(int)`: no-op for zero iterations; otherwise `Boost` maps its source to 0..1 first.
    pub fn boost(self, iterations: usize) -> Self {
        if iterations == 0 {
            self
        } else {
            let mapped = self.map(Self::constant(0.0), Self::constant(1.0));
            Self::modifier(ModuleKind::Boost(mapped.clone(), iterations), &mapped)
        }
    }

    /// `Module.clamp(Module, Module)`: skipped when constant bounds equal the source's.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        if constant_bounds(&self, &min, &max) {
            self
        } else {
            let (low, high) = (min.min_value(), max.max_value());
            Self::node(ModuleKind::Clamp(self, min, max), low, high)
        }
    }

    /// `Module.invert()` (no shortcut in Engine: inverting twice nests).
    pub fn invert(self) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Invert(self), &source)
    }

    /// `Module.map(Module, Module)`: skipped when constant bounds equal the source's.
    pub fn map(self, min: Self, max: Self) -> Self {
        if constant_bounds(&self, &min, &max) {
            self
        } else {
            let range = self.max_value() - self.min_value();
            let (low, high) = (min.min_value(), max.max_value());
            Self::node(ModuleKind::Map(self, min, max, range), low, high)
        }
    }

    /// `Module.pow(Module)`: a constant 0 power gives `Source.ONE`, a constant 1 power the source itself.
    pub fn power(self, power: Self) -> Self {
        if power.min_value() == 0.0 && power.max_value() == 0.0 {
            Self::constant(1.0)
        } else if power.min_value() == 1.0 && power.max_value() == 1.0 {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Power(self, power), &source)
        }
    }

    /// `Module.scale(Module)`: skipped for a scale of 1; bounds are fixed when it is built.
    pub fn scale(self, scale: Self) -> Self {
        if scale.min_value() == 1.0 && scale.max_value() == 1.0 {
            return self;
        }
        let min = self.min_value() * scale.min_value();
        let max = self.max_value() * scale.max_value();
        Self::node(ModuleKind::Scale(self, scale), min, max)
    }

    /// `Module.threshold(Module)`: Engine reports the source's bounds, not 0..1.
    pub fn threshold(self, threshold: Self) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Threshold(self, threshold), &source)
    }

    /// `Module.warp(Domain)`: samples the source at the warped point; reports the source's bounds.
    pub fn warp(self, domain: Domain) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Warp(self, domain), &source)
    }

    /// `Module.base(Module, falloff, Interpolation)` (`new Base(this, other, ...)`; Engine's two-argument form
    /// uses `Interpolation.CURVE3`). Reports the base's minimum and the larger of both maxima.
    pub fn base(self, source: Self, falloff: f32, interpolation: Interpolation) -> Self {
        let min = self.max_value();
        let max = self.max_value() + falloff;
        let (low, high) = (
            self.min_value(),
            java_max(self.max_value(), source.max_value()),
        );
        let kind = ModuleKind::Base {
            base: self,
            source,
            min,
            max,
            falloff,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blend(Module, Module, midpoint, blendRange, Interpolation)` (Engine's four-argument form uses
    /// `Interpolation.LINEAR`). Reports the smaller minimum and larger maximum of `lower` and `upper`.
    pub fn blend(
        self,
        lower: Self,
        upper: Self,
        midpoint: f32,
        blend_range: f32,
        interpolation: Interpolation,
    ) -> Self {
        let mid = self.min_value() + (self.max_value() - self.min_value()) * midpoint;
        let blend_lower = java_max(self.min_value(), mid - blend_range / 2.0);
        let blend_upper = java_min(self.max_value(), mid + blend_range / 2.0);
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::Blend {
            control: self,
            lower,
            upper,
            midpoint,
            blend: blend_range,
            blend_lower,
            blend_upper,
            blend_range: blend_upper - blend_lower,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blendVar(variable, Module, Module, midpoint, min, max, Interpolation)` (`VariableBlend`).
    pub fn variable_blend(
        self,
        variable: Self,
        lower: Self,
        upper: Self,
        midpoint: f32,
        min_blend: f32,
        max_blend: f32,
        interpolation: Interpolation,
    ) -> Self {
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::VariableBlend {
            control: self,
            variable,
            lower,
            upper,
            midpoint,
            min_blend,
            max_blend,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blend(blend, Interpolation, sources...)` (`MultiBlend`; Engine's two-argument form uses
    /// `Interpolation.LINEAR`). Splits the control's 0..1 range into one evenly spaced slot per source and blends
    /// neighbouring slots. Reports the smallest minimum and largest maximum of the sources (0 and 0 when empty).
    pub fn multi_blend(self, blend: f32, interpolation: Interpolation, sources: Vec<Self>) -> Self {
        let spacing = 1.0 / sources.len() as f32;
        let radius = spacing / 2.0;
        let blend_range = radius * blend;
        let cell_radius = (radius - blend_range) / 2.0;
        // Java's `int maxIndex = sources.length - 1`: -1 without sources.
        let max_index = sources.len() as i32 - 1;
        let low = fold_bounds(&sources, Self::min_value, |a, b| java_min(a, b.min_value()));
        let high = fold_bounds(&sources, Self::max_value, |a, b| java_max(a, b.max_value()));
        let nodes = sources
            .into_iter()
            .enumerate()
            .map(|(index, source)| {
                let position = index as f32 * spacing + radius;
                let min = if index == 0 {
                    0.0
                } else {
                    position - cell_radius
                };
                let max = if index as i32 == max_index {
                    1.0
                } else {
                    position + cell_radius
                };
                BlendNode {
                    source,
                    min: java_max(0.0, min),
                    max: java_min(1.0, max),
                }
            })
            .collect();
        let kind = ModuleKind::MultiBlend {
            control: self,
            nodes,
            max_index,
            blend,
            blend_range,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.terrace(ramp, cliff, rampHeight, steps, blendRange)` (`Terrace`): `steps` evenly spaced flat steps
    /// over the source's range, fixed in the constructor, joined by a ramp and a cliff. Reports the source's bounds.
    pub fn terrace(
        self,
        ramp: Self,
        cliff: Self,
        ramp_height: Self,
        steps: usize,
        blend_range: f32,
    ) -> Self {
        let (steps, max_index) = terrace_steps(&self, steps, blend_range);
        let source = self.clone();
        let kind = ModuleKind::Terrace {
            source: self,
            ramp,
            cliff,
            ramp_height,
            steps,
            max_index,
            blend: blend_range,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.legacyTerrace(lowerCurve, upperCurve, steps, blendRange)` (`LegacyTerrace`): clamps its source to
    /// 0..1 and snaps it to the nearest of `steps` evenly spaced steps (built as `terrace`'s), with Curve3 transitions
    /// scaled by `lower_curve` towards the next step and by `upper_curve` from the previous one. Reports the source's
    /// bounds. For Engine's `legacyTerrace(double, double, ...)`, pass `Module::constant` curves.
    pub fn legacy_terrace(
        self,
        lower_curve: Self,
        upper_curve: Self,
        steps: usize,
        blend_range: f32,
    ) -> Self {
        let (steps, max_index) = terrace_steps(&self, steps, blend_range);
        let source = self.clone();
        let kind = ModuleKind::LegacyTerrace {
            source: self,
            lower_curve,
            upper_curve,
            steps,
            max_index,
            blend: blend_range,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.terrace(modulation, mask, slope, blendMin, blendMax, steps, octaves)` (`AdvancedTerrace`): above
    /// `blend_min`, snaps the source to `steps * i` steps for each octave `i`, pulling each result back towards the
    /// source by `slope`, adds `modulation`, divides by `source.maxValue() + modulation.maxValue()` (fixed in the
    /// constructor) and blends that in, fully above `blend_max` and scaled by `mask`. A zero mask, or a sample at or
    /// below `blend_min`, returns the source sample. Reports the source's bounds. Engine's shorter `terrace`
    /// overloads pass `Source.ONE` as the mask, `Module::constant(slope)` and one octave.
    #[allow(clippy::too_many_arguments)]
    pub fn advanced_terrace(
        self,
        modulation: Self,
        mask: Self,
        slope: Self,
        blend_min: f32,
        blend_max: f32,
        steps: i32,
        octaves: i32,
    ) -> Self {
        let mod_range = self.max_value() + modulation.max_value();
        let source = self.clone();
        let kind = ModuleKind::AdvancedTerrace {
            source: self,
            modulation,
            mask,
            slope,
            blend_min,
            blend_max,
            blend_range: blend_max - blend_min,
            mod_range,
            steps,
            octaves,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.curve(CurveFunc)` (`Curve`); Engine's `curve(mid, steepness)` passes a `MidPointCurve`.
    pub fn curve(self, function: CurveFunction) -> Self {
        let source = self.clone();
        Self::modifier(
            ModuleKind::Curve {
                source: self,
                function,
            },
            &source,
        )
    }

    /// `Module.curve(Module, Module)` (`VariableCurve`): `NoiseUtil.curve` with a sampled midpoint and steepness.
    pub fn variable_curve(self, midpoint: Self, gradient: Self) -> Self {
        let source = self.clone();
        let kind = ModuleKind::VariableCurve {
            source: self,
            midpoint,
            gradient,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.powCurve(n)` (`PowerCurve`): the curve's bounds and midpoint are fixed from the source's bounds when it
    /// is built; it reports the source's bounds.
    pub fn power_curve(self, power: f32) -> Self {
        let source_min = self.min_value();
        let source_max = self.max_value();
        let source_midpoint = source_min + (source_max - source_min) / 2.0;
        let min = source_midpoint - java_pow(source_midpoint - source_min, power);
        let max = source_midpoint + java_pow(source_max - source_midpoint, power);
        let range = max - min;
        let source = self.clone();
        let kind = ModuleKind::PowerCurve {
            source: self,
            min,
            max,
            midpoint: min + range / 2.0,
            range,
            power,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.steps(Module, Module, Module, CurveFunc)` (`Steps`). Engine's `steps(int, double, double...)` overloads
    /// pass `Source.constant(...)` and default to `Interpolation.LINEAR` with slopes 0 and 0.
    pub fn steps(
        self,
        steps: Self,
        slope_min: Self,
        slope_max: Self,
        curve: CurveFunction,
    ) -> Self {
        let source = self.clone();
        let kind = ModuleKind::Steps {
            source: self,
            steps,
            slope_min,
            slope_max,
            curve,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.freq(Module, Module)`.
    pub fn frequency(self, x: Self, y: Self) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Frequency { source: self, x, y }, &source)
    }

    /// `Module.grad(Module, Module, Module)`.
    pub fn gradient(self, lower: Self, upper: Self, strength: Self) -> Self {
        let source = self.clone();
        let kind = ModuleKind::Gradient {
            source: self,
            lower,
            upper,
            strength,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.mod(Module, Module)`.
    pub fn modulate(self, direction: Self, strength: Self) -> Self {
        let source = self.clone();
        let kind = ModuleKind::Modulate {
            source: self,
            direction,
            strength,
        };
        Self::modifier(kind, &source)
    }

    /// `Module.cache()`: wraps the module in a `Cache`, or returns it unchanged when it already is one (as Engine
    /// does, so the existing cache, shared with its other users, is kept).
    pub fn cache(self) -> Self {
        if matches!(self.0.kind, ModuleKind::Cache { .. }) {
            return self;
        }
        let source = self.clone();
        Self::modifier(
            ModuleKind::Cache {
                source: self,
                value: std::sync::Mutex::new(None),
            },
            &source,
        )
    }

    /// `Module.select(Module, Module, lowerBound, upperBound, falloff, Interpolation)` (Engine's five-argument
    /// form uses `Interpolation.CURVE3`).
    pub fn select(
        self,
        lower: Self,
        upper: Self,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        interpolation: Interpolation,
    ) -> Self {
        let curve = |bound: f32| {
            let (min, max) = (bound - falloff, bound + falloff);
            (min, max, max - min)
        };
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::Select {
            control: self,
            lower,
            upper,
            lower_bound,
            upper_bound,
            falloff,
            lower_curve: curve(lower_bound),
            upper_curve: curve(upper_bound),
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `new Add(sources...)`.
    pub fn add(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| a + b.min_value());
        let max = fold_bounds(&sources, Self::max_value, |a, b| a + b.max_value());
        Self::node(ModuleKind::Add(sources), min, max)
    }

    /// `new Multiply(sources...)`.
    pub fn multiply(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| a * b.min_value());
        let max = fold_bounds(&sources, Self::max_value, |a, b| a * b.max_value());
        Self::node(ModuleKind::Multiply(sources), min, max)
    }

    /// `new Max(sources...)`: Engine's `minTotal` is `maxTotal`, so the minimum folds the later maxima.
    pub fn max(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| java_max(a, b.max_value()));
        let max = fold_bounds(&sources, Self::max_value, |a, b| java_max(a, b.max_value()));
        Self::node(ModuleKind::Max(sources), min, max)
    }

    /// `new Min(sources...)`: Engine's `maxTotal` is `minTotal`, so the maximum folds the later minima.
    pub fn min(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| java_min(a, b.min_value()));
        let max = fold_bounds(&sources, Self::max_value, |a, b| java_min(a, b.min_value()));
        Self::node(ModuleKind::Min(sources), min, max)
    }

    /// `new Sub(sources...)`: the minimum subtracts the later maxima and the maximum the later minima.
    pub fn subtract(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| a - b.max_value());
        let max = fold_bounds(&sources, Self::max_value, |a, b| a - b.min_value());
        Self::node(ModuleKind::Subtract(sources), min, max)
    }
}

/// Reseeding (TerraForged's `ContextSeedable.withSeed`, 1.18.2-0.3.1): the jar serialises the module with Cereal
/// and reads it back with `seed` in the context, so each node is rebuilt by its spec's factory (its Java
/// constructor, not `Module`'s shortcut methods) and keeps only what its spec writes.
impl Module {
    /// `withSeed(seed, module, Module.class)`: a new graph whose noise sources (the octave sources, `CellNoise` and
    /// `CellEdgeNoise`) add `(int) seed` to their seeds, wrapping like Java's `int`. The graph is rebuilt, never
    /// changed in place: caches start empty, and nodes shared in this graph become separate copies, as in Java.
    ///
    /// The specs' quirks are kept: `Bias` reads its bias module back as the float 0, `CellNoise`/`CellEdgeNoise`
    /// lose their displacement (back to the builder's 1), `MultiBlend` truncates its blend to an `int`, and the
    /// children of `Add`, `Multiply`, `Max`, `Min`, `Sub`, Engine's erosion `Ridge` (`Valley`) and `Line` are read
    /// without the context, so their seeds stay (the quirks still apply there). A custom module in a combiner's or
    /// `MultiBlend`'s list writes null and is dropped, a custom `CellNoise` lookup reads back as `Source.ZERO`, and
    /// nothing under a `Bias`'s bias is read back. Where the jar throws (no spec:
    /// a custom module, `Domain.DIRECT`, `AddWarp` and `CompoundWarp`, whose specs are registered under
    /// `CumulativeWarp`'s name; a `MidPointCurve` or `SCurve`, which it cannot read back), this panics.
    pub fn with_seed(&self, seed: i64) -> Self {
        // `Context.getData().add("seed", seed)` read back by `NoiseSpec.seed` as `asInt`: the long's low 32 bits.
        self.reseed(seed as i32)
    }

    /// One node through its spec; `seed` is the context's (0 for `Context.NONE`).
    fn reseed(&self, seed: i32) -> Self {
        let r = |module: &Self| module.reseed(seed);
        let plain = |module: &Self| module.reseed(0);
        let all = |sources: &[Self], seed| reseed_list(sources.iter(), seed);
        let modifier = |kind, source: &Self| Self::modifier(kind, source);
        match &self.0.kind {
            // `Const`: the value goes through a `double` and back, so -0.0 and NaN payloads stay.
            ModuleKind::Constant(value) => Self::node(ModuleKind::Constant(*value), *value, *value),
            ModuleKind::Custom(_) => {
                panic!("Failed to reseed value: no spec registered for a custom module")
            }
            ModuleKind::Source(source) => {
                let mut config = source.config;
                config.seed = config.seed.wrapping_add(seed);
                Self::source(Source::new(source.kind, config))
            }
            ModuleKind::ErosionRidge { ridge, source } => {
                Self::erosion_ridge(*ridge, plain(source))
            }
            ModuleKind::CellSource {
                config,
                function,
                distance,
                lookup,
                ..
            } => {
                let config = SourceConfig {
                    seed: config.seed.wrapping_add(seed),
                    ..*config
                };
                // A custom lookup is left out and read back as the builder's default, `Source.ZERO`.
                let lookup = match lookup.0.kind {
                    ModuleKind::Custom(_) => Self::constant(0.0),
                    _ => r(lookup),
                };
                Self::cell_source(config, 1.0, *function, *distance, lookup)
            }
            ModuleKind::CellEdgeSource {
                config,
                edge,
                distance,
                ..
            } => {
                let config = SourceConfig {
                    seed: config.seed.wrapping_add(seed),
                    ..*config
                };
                Self::cell_edge_source(config, 1.0, *edge, *distance)
            }
            ModuleKind::SineSource { frequency, alpha } => Self::sine_source(*frequency, r(alpha)),
            ModuleKind::Line(line) => Self::line(Line::new(
                line.x1,
                line.y1,
                line.x2,
                line.y2,
                plain(&line.radius),
                plain(&line.fade_in),
                plain(&line.fade_out),
                line.feather_scale,
            )),
            ModuleKind::Abs(source) => {
                let source = r(source);
                modifier(ModuleKind::Abs(source.clone()), &source)
            }
            ModuleKind::Alpha(source, alpha) => {
                let source = r(source);
                modifier(ModuleKind::Alpha(source.clone(), r(alpha)), &source)
            }
            // `new Bias(source, spec.get("bias", asFloat))`: the bias was written as a module, so it reads back as 0.
            ModuleKind::Bias(source, _) => {
                let (source, bias) = (r(source), Self::constant(0.0));
                let (min, max) = (source.min_value() + 0.0, source.max_value() + 0.0);
                Self::node(ModuleKind::Bias(source, bias), min, max)
            }
            // `new Boost(source, iterations)` maps its (already mapped) source again, through `Module.map`.
            ModuleKind::Boost(source, iterations) => {
                let source = r(source).map(Self::constant(0.0), Self::constant(1.0));
                modifier(ModuleKind::Boost(source.clone(), *iterations), &source)
            }
            ModuleKind::Clamp(source, min, max) => {
                let (min, max) = (r(min), r(max));
                let (low, high) = (min.min_value(), max.max_value());
                Self::node(ModuleKind::Clamp(r(source), min, max), low, high)
            }
            ModuleKind::Invert(source) => r(source).invert(),
            ModuleKind::Map(source, min, max, _) => {
                let (source, min, max) = (r(source), r(min), r(max));
                let range = source.max_value() - source.min_value();
                let (low, high) = (min.min_value(), max.max_value());
                Self::node(ModuleKind::Map(source, min, max, range), low, high)
            }
            ModuleKind::Power(source, power) => {
                let source = r(source);
                modifier(ModuleKind::Power(source.clone(), r(power)), &source)
            }
            ModuleKind::Scale(source, scale) => {
                let (source, scale) = (r(source), r(scale));
                let min = source.min_value() * scale.min_value();
                let max = source.max_value() * scale.max_value();
                Self::node(ModuleKind::Scale(source, scale), min, max)
            }
            ModuleKind::Threshold(source, threshold) => r(source).threshold(r(threshold)),
            ModuleKind::Warp(source, domain) => r(source).warp(domain.reseed(seed)),
            ModuleKind::Base {
                base,
                source,
                falloff,
                interpolation,
                ..
            } => r(base).base(r(source), *falloff, *interpolation),
            ModuleKind::Blend {
                control,
                lower,
                upper,
                midpoint,
                blend,
                interpolation,
                ..
            } => r(control).blend(r(lower), r(upper), *midpoint, *blend, *interpolation),
            ModuleKind::VariableBlend {
                control,
                variable,
                lower,
                upper,
                midpoint,
                min_blend,
                max_blend,
                interpolation,
            } => r(control).variable_blend(
                r(variable),
                r(lower),
                r(upper),
                *midpoint,
                *min_blend,
                *max_blend,
                *interpolation,
            ),
            // `blend_range` is written as a float and read back with `asInt`: Java's `(int)` cast.
            ModuleKind::MultiBlend {
                control,
                nodes,
                blend,
                interpolation,
                ..
            } => {
                let sources = reseed_list(nodes.iter().map(|node| &node.source), seed);
                r(control).multi_blend(*blend as i32 as f32, *interpolation, sources)
            }
            ModuleKind::Terrace {
                source,
                ramp,
                cliff,
                ramp_height,
                steps,
                blend,
                ..
            } => r(source).terrace(r(ramp), r(cliff), r(ramp_height), steps.len(), *blend),
            ModuleKind::LegacyTerrace {
                source,
                lower_curve,
                upper_curve,
                steps,
                blend,
                ..
            } => r(source).legacy_terrace(r(lower_curve), r(upper_curve), steps.len(), *blend),
            ModuleKind::AdvancedTerrace {
                source,
                modulation,
                mask,
                slope,
                blend_min,
                blend_max,
                steps,
                octaves,
                ..
            } => r(source).advanced_terrace(
                r(modulation),
                r(mask),
                r(slope),
                *blend_min,
                *blend_max,
                *steps,
                *octaves,
            ),
            ModuleKind::Curve { source, function } => r(source).curve(reseed_curve(*function)),
            ModuleKind::VariableCurve {
                source,
                midpoint,
                gradient,
            } => r(source).variable_curve(r(midpoint), r(gradient)),
            ModuleKind::PowerCurve { source, power, .. } => r(source).power_curve(*power),
            ModuleKind::Steps {
                source,
                steps,
                slope_min,
                slope_max,
                curve,
            } => r(source).steps(r(steps), r(slope_min), r(slope_max), reseed_curve(*curve)),
            ModuleKind::Frequency { source, x, y } => r(source).frequency(r(x), r(y)),
            ModuleKind::Gradient {
                source,
                lower,
                upper,
                strength,
            } => r(source).gradient(r(lower), r(upper), r(strength)),
            ModuleKind::Modulate {
                source,
                direction,
                strength,
            } => r(source).modulate(r(direction), r(strength)),
            // `new Cache(source)`: a fresh, empty entry.
            ModuleKind::Cache { source, .. } => {
                let source = r(source);
                let kind = ModuleKind::Cache {
                    source: source.clone(),
                    value: std::sync::Mutex::new(None),
                };
                modifier(kind, &source)
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                lower_bound,
                upper_bound,
                falloff,
                interpolation,
                ..
            } => r(control).select(
                r(lower),
                r(upper),
                *lower_bound,
                *upper_bound,
                *falloff,
                *interpolation,
            ),
            // `Combiner`'s factory reads its modules without the context.
            ModuleKind::Add(sources) => Self::add(all(sources, 0)),
            ModuleKind::Multiply(sources) => Self::multiply(all(sources, 0)),
            ModuleKind::Max(sources) => Self::max(all(sources, 0)),
            ModuleKind::Min(sources) => Self::min(all(sources, 0)),
            ModuleKind::Subtract(sources) => Self::subtract(all(sources, 0)),
        }
    }
}

/// A list of modules through Cereal (a combiner's modules, `MultiBlend`'s sources): a custom module writes null and
/// is dropped.
fn reseed_list<'a>(modules: impl Iterator<Item = &'a Module>, seed: i32) -> Vec<Module> {
    modules
        .filter(|module| !matches!(module.0.kind, ModuleKind::Custom(_)))
        .map(|module| module.reseed(seed))
        .collect()
}

/// A `CurveFunc` through Cereal: an `Interpolation` reads back; `MidPointCurve` and `SCurve` are written without a
/// type name, which the jar cannot read back ("Unsupported data"), so this panics.
fn reseed_curve(function: CurveFunction) -> CurveFunction {
    match function {
        CurveFunction::Interpolation(_) => function,
        CurveFunction::Midpoint { .. } | CurveFunction::SCurve(_) => {
            panic!("Failed to reseed value: a MidPointCurve or SCurve cannot be read back")
        }
    }
}

/// Engine's `Domain`: a coordinate warp. Cloning shares the node, as Java shares domain instances.
#[derive(Clone)]
pub struct Domain(Arc<DomainKind>);

enum DomainKind {
    /// `Domain.DIRECT`.
    Direct,
    /// `DomainWarp`: both axis modules are already mapped to -0.5..0.5.
    DomainWarp {
        x: Module,
        y: Module,
        distance: Module,
    },
    /// `DirectionWarp`.
    Direction { direction: Module, strength: Module },
    /// `AddWarp`.
    Add(Domain, Domain),
    /// `CompoundWarp`.
    Compound(Domain, Domain),
    /// `CumulativeWarp`.
    Cumulative(Domain, Domain),
}

impl Domain {
    fn node(kind: DomainKind) -> Self {
        Self(Arc::new(kind))
    }

    /// A domain through its spec (see [`Module::with_seed`]): `DomainWarp` is rebuilt by its constructor, which maps
    /// its axes again unless they already report -0.5..0.5. `Domain.DIRECT` has no spec, and `AddWarp`'s and
    /// `CompoundWarp`'s are registered under `CumulativeWarp`'s name, so the jar throws for them and this panics.
    fn reseed(&self, seed: i32) -> Self {
        let r = |module: &Module| module.reseed(seed);
        match self.0.as_ref() {
            DomainKind::DomainWarp { x, y, distance } => Self::domain_warp(r(x), r(y), r(distance)),
            DomainKind::Direction {
                direction,
                strength,
            } => Self::direction(r(direction), r(strength)),
            DomainKind::Cumulative(first, second) => first.reseed(seed).then(second.reseed(seed)),
            DomainKind::Direct | DomainKind::Add(..) | DomainKind::Compound(..) => {
                panic!("Failed to reseed value: no spec registered for this domain")
            }
        }
    }

    /// `Domain.DIRECT`.
    pub fn direct() -> Self {
        Self::node(DomainKind::Direct)
    }

    /// `Domain.warp(Module, Module, Module)` (`new DomainWarp`): each axis is mapped to -0.5..0.5 unless it already
    /// reports exactly those bounds (`Module.map`'s own shortcut is the same test).
    pub fn domain_warp(x: Module, y: Module, distance: Module) -> Self {
        let map = |axis: Module| axis.map(Module::constant(-0.5), Module::constant(0.5));
        Self::node(DomainKind::DomainWarp {
            x: map(x),
            y: map(y),
            distance,
        })
    }

    /// `Domain.direction(Module, Module)` (`new DirectionWarp`).
    pub fn direction(direction: Module, strength: Module) -> Self {
        Self::node(DomainKind::Direction {
            direction,
            strength,
        })
    }

    /// `Domain.add(Domain)` (`AddWarp`): the two offsets summed.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, next: Self) -> Self {
        Self::node(DomainKind::Add(self, next))
    }

    /// `Domain.warp(Domain)` (`CompoundWarp`): `next`'s offset at the point warped by `self`.
    pub fn compound(self, next: Self) -> Self {
        Self::node(DomainKind::Compound(self, next))
    }

    /// `Domain.then(Domain)` (`CumulativeWarp`): Engine returns `next`'s warped *coordinate* as the offset, so
    /// `x()` adds the input coordinate once more.
    pub fn then(self, next: Self) -> Self {
        Self::node(DomainKind::Cumulative(self, next))
    }

    /// Noise2D 2cb4843's `Domain.getOffsetX(int seed, float x, float y)`: [`Domain::offset_x`] with the call's seed
    /// passed unchanged to every module and nested domain (`DirectionWarp` uses the same sine table). An error from
    /// any module (a node not ported to seed-first sampling) is returned as is.
    pub fn try_offset_x_seed_first(&self, seed: i32, x: f32, y: f32) -> Result<f32, &'static str> {
        Ok(match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                x: axis, distance, ..
            } => {
                axis.try_value_seed_first(seed, x, y)?
                    * distance.try_value_seed_first(seed, x, y)?
            }
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.try_value_seed_first(seed, x, y)? * PI2;
                sin(angle) * strength.try_value_seed_first(seed, x, y)?
            }
            DomainKind::Add(first, second) => {
                first.try_offset_x_seed_first(seed, x, y)?
                    + second.try_offset_x_seed_first(seed, x, y)?
            }
            DomainKind::Compound(first, second) => second.try_offset_x_seed_first(
                seed,
                first.try_x_seed_first(seed, x, y)?,
                first.try_y_seed_first(seed, x, y)?,
            )?,
            DomainKind::Cumulative(first, second) => second.try_x_seed_first(
                seed,
                first.try_x_seed_first(seed, x, y)?,
                first.try_y_seed_first(seed, x, y)?,
            )?,
        })
    }

    /// Noise2D 2cb4843's `Domain.getOffsetY(int seed, float x, float y)`; see [`Domain::try_offset_x_seed_first`].
    pub fn try_offset_y_seed_first(&self, seed: i32, x: f32, y: f32) -> Result<f32, &'static str> {
        Ok(match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                y: axis, distance, ..
            } => {
                axis.try_value_seed_first(seed, x, y)?
                    * distance.try_value_seed_first(seed, x, y)?
            }
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.try_value_seed_first(seed, x, y)? * PI2;
                cos(angle) * strength.try_value_seed_first(seed, x, y)?
            }
            DomainKind::Add(first, second) => {
                first.try_offset_y_seed_first(seed, x, y)?
                    + second.try_offset_y_seed_first(seed, x, y)?
            }
            DomainKind::Compound(first, second) => second.try_offset_y_seed_first(
                seed,
                first.try_x_seed_first(seed, x, y)?,
                first.try_y_seed_first(seed, x, y)?,
            )?,
            DomainKind::Cumulative(first, second) => second.try_y_seed_first(
                seed,
                first.try_x_seed_first(seed, x, y)?,
                first.try_y_seed_first(seed, x, y)?,
            )?,
        })
    }

    /// Noise2D 2cb4843's `Domain.getX(int seed, float x, float y)`: `x + getOffsetX(seed, x, y)`.
    pub fn try_x_seed_first(&self, seed: i32, x: f32, y: f32) -> Result<f32, &'static str> {
        Ok(x + self.try_offset_x_seed_first(seed, x, y)?)
    }

    /// Noise2D 2cb4843's `Domain.getY(int seed, float x, float y)`: `y + getOffsetY(seed, x, y)`.
    pub fn try_y_seed_first(&self, seed: i32, x: f32, y: f32) -> Result<f32, &'static str> {
        Ok(y + self.try_offset_y_seed_first(seed, x, y)?)
    }

    /// `Domain.getOffsetX`.
    pub fn offset_x(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                x: axis, distance, ..
            } => axis.value(x, y) * distance.value(x, y),
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.value(x, y) * PI2;
                sin(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_x(x, y) + second.offset_x(x, y),
            DomainKind::Compound(first, second) => second.offset_x(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.x(first.x(x, y), first.y(x, y)),
        }
    }

    /// `Domain.getOffsetY`.
    pub fn offset_y(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                y: axis, distance, ..
            } => axis.value(x, y) * distance.value(x, y),
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.value(x, y) * PI2;
                cos(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_y(x, y) + second.offset_y(x, y),
            DomainKind::Compound(first, second) => second.offset_y(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.y(first.x(x, y), first.y(x, y)),
        }
    }

    /// `Domain.getX`.
    pub fn x(&self, x: f32, y: f32) -> f32 {
        x + self.offset_x(x, y)
    }

    /// `Domain.getY`.
    pub fn y(&self, x: f32, y: f32) -> f32 {
        y + self.offset_y(x, y)
    }
}

/// The steps and `maxIndex` that Engine's `Terrace` and `LegacyTerrace` constructors build: `count` steps from 0,
/// spaced by the source's range over `count - 1`. Java's `int maxIndex = steps - 1` is -1 without steps.
fn terrace_steps(source: &Module, count: usize, blend_range: f32) -> (Vec<TerraceStep>, i32) {
    let max_index = count as i32 - 1;
    let range = source.max_value() - source.min_value();
    let spacing = range / max_index as f32;
    let steps = (0..count)
        .map(|index| {
            let value = index as f32 * spacing;
            let blend = spacing * blend_range;
            let bound = (spacing - blend) / 2.0;
            TerraceStep {
                value,
                lower_bound: value - bound,
                upper_bound: value + bound,
            }
        })
        .collect();
    (steps, max_index)
}

/// `LegacyTerrace.getValue` from its source sample on, shared by the seedless and seed-first paths (Engine 0.3.0 and
/// Noise2D 2cb4843 agree here): clamps the sample to 0..1, rounds it to a step index (`round` sends NaN to 0), and
/// eases into the neighbouring steps. Only the transition taken samples its curve. As in Java, a sample below the
/// first step's window returns that step without checking the upper window (which can matter for a blend range above
/// 1, where the windows invert).
fn legacy_terrace<E>(
    source_value: f32,
    steps: &[TerraceStep],
    max_index: i32,
    lower_curve: impl FnOnce() -> Result<f32, E>,
    upper_curve: impl FnOnce() -> Result<f32, E>,
) -> Result<f32, E> {
    let noise_value = clamp(source_value, 0.0, 1.0);
    let index = round(noise_value * max_index as f32);
    // `round` keeps a clamped sample inside the steps; only a terrace without steps (`maxIndex` -1) leaves them, where
    // Java throws.
    let step = usize::try_from(index)
        .ok()
        .and_then(|index| steps.get(index))
        .expect("LegacyTerrace samples a step outside its steps, where Java throws");
    let index = index as usize;
    if noise_value < step.lower_bound {
        if index > 0 {
            let lower = &steps[index - 1];
            let alpha = (noise_value - lower.upper_bound) / (step.lower_bound - lower.upper_bound);
            let alpha = 1.0 - Interpolation::Curve3.apply(alpha);
            let range = step.value - lower.value;
            return Ok(step.value - alpha * range * upper_curve()?);
        }
    } else if noise_value > step.upper_bound && (index as i32) < max_index {
        let upper = &steps[index + 1];
        let alpha = (noise_value - step.upper_bound) / (upper.lower_bound - step.upper_bound);
        let alpha = Interpolation::Curve3.apply(alpha);
        let range = upper.value - step.value;
        return Ok(step.value + alpha * range * lower_curve()?);
    }
    Ok(step.value)
}

/// `AdvancedTerrace.modify` from its source sample on, shared by the seedless and seed-first paths (Engine 0.3.0 and
/// Noise2D 2cb4843 agree here). The array holds the constructor's `[blendMin, blendMax, blendRange, modRange]`. As in
/// Java, a sample at or below `blendMin` reads no other child, a zero mask reads neither slope nor modulation, and the
/// children are read in Java's order: mask, slope, modulation. `steps * i` wraps like Java's `int`. Java's
/// `for (int i = 1; i <= octaves; i++)` never ends for `octaves == Integer.MAX_VALUE` (`i` wraps); this loop stops there.
fn advanced_terrace<E>(
    value: f32,
    [blend_min, blend_max, blend_range, mod_range]: [f32; 4],
    steps: i32,
    octaves: i32,
    mask: impl FnOnce() -> Result<f32, E>,
    slope: impl FnOnce() -> Result<f32, E>,
    modulation: impl FnOnce() -> Result<f32, E>,
) -> Result<f32, E> {
    if value <= blend_min {
        return Ok(value);
    }
    let mask = mask()?;
    if mask == 0.0 {
        return Ok(value);
    }
    let mut result = value;
    let slope = slope()?;
    let modulation = modulation()?;
    for octave in 1..=octaves {
        let steps = steps.wrapping_mul(octave) as f32;
        result = round(result * steps) as f32 / steps;
        result += (value - result) * slope;
    }
    result = (result + modulation) / mod_range;
    let mut alpha = if value > blend_max {
        1.0
    } else {
        (value - blend_min) / blend_range
    };
    if mask != 1.0 {
        alpha *= mask;
    }
    Ok(lerp(value, result, alpha))
}

/// The shortcut test shared by `Module.clamp` and `Module.map`.
fn constant_bounds(source: &Module, min: &Module, max: &Module) -> bool {
    min.min_value() == min.max_value()
        && min.min_value() == source.min_value()
        && max.min_value() == max.max_value()
        && max.max_value() == source.max_value()
}

fn combine(sources: &[Module], x: f32, y: f32, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first.value(x, y), |value, source| {
        operation(value, source.value(x, y))
    })
}

/// `Combiner.getValue(seed, x, y)`: [`combine`] with the runtime seed passed to every source.
fn try_combine(
    sources: &[Module],
    seed: i32,
    x: f32,
    y: f32,
    operation: impl Fn(f32, f32) -> f32,
) -> Result<f32, &'static str> {
    let Some((first, rest)) = sources.split_first() else {
        return Ok(0.0);
    };
    rest.iter()
        .try_fold(first.try_value_seed_first(seed, x, y)?, |value, source| {
            Ok(operation(value, source.try_value_seed_first(seed, x, y)?))
        })
}

/// `MultiBlend`'s `nodes[index]`. Engine throws `ArrayIndexOutOfBoundsException` when the control leaves 0..1 far
/// enough to pick a slot that does not exist (or has no sources); this port panics there instead of inventing a value.
fn multi_blend_slot(nodes: &[BlendNode], index: Option<i32>) -> &BlendNode {
    index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| nodes.get(index))
        .expect("MultiBlend control selects a slot outside its sources, where Engine throws")
}

/// `Selector`'s `Combiner` bounds over `{lower, upper}`: `Math.min` of the minima, `Math.max` of the maxima.
fn selector_bounds(lower: &Module, upper: &Module) -> (f32, f32) {
    (
        java_min(lower.min_value(), upper.min_value()),
        java_max(lower.max_value(), upper.max_value()),
    )
}

/// `Combiner`'s constructor: the first source's bound, then `minTotal`/`maxTotal` over the rest; 0 when empty.
fn fold_bounds(
    sources: &[Module],
    first: impl Fn(&Module) -> f32,
    total: impl Fn(f32, &Module) -> f32,
) -> f32 {
    let Some((head, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first(head), total)
}

/// `NoiseUtil.pow(float, float)`: `(float) Math.pow(value, power)`. Java returns NaN for a NaN power and for a base of
/// ±1 with an infinite power, where C's `pow` (Rust's `powf`) returns 1 for a base of 1 (or -1 and an infinite power).
/// A NaN result keeps Java's bits: `Math.pow` returns its NaN operand (the base's first, quieted), except that a zero
/// power gives 1 and a base of 1 with a NaN power gives the canonical NaN.
pub fn java_pow(value: f32, power: f32) -> f32 {
    let quiet = |nan: f32| f32::from_bits(nan.to_bits() | 0x0040_0000);
    if power == 0.0 {
        return 1.0;
    }
    if value.is_nan() {
        return quiet(value);
    }
    if power.is_nan() {
        return if value == 1.0 { f32::NAN } else { quiet(power) };
    }
    if value.abs() == 1.0 && power.is_infinite() {
        return f32::NAN;
    }
    (f64::from(value).powf(f64::from(power))) as f32
}

/// `PowerCurve.modify` up to its `NoiseUtil.map` (both paths; the seedless one maps with the jar's [`map`], the
/// seed-first one with [`map_current`]): the sample's distance from the curve's midpoint raised to `power`
/// (`Math.pow`), on the sample's side of the midpoint. A NaN sample takes the lower branch and stays NaN.
fn power_curve_value(value: f32, midpoint: f32, power: f32) -> f32 {
    if value >= midpoint {
        midpoint + java_pow(value - midpoint, power)
    } else {
        midpoint - java_pow(midpoint - value, power)
    }
}

/// `Steps`, `Freq`, `Grad` and `Modulate`, shared by the seedless and seed-first paths (Engine 0.3.0 and Noise2D
/// 2cb4843 agree here apart from the seed and `Steps`'s `map`, the jar's [`map`] or 2cb4843's [`map_current`]).
/// `sample(module, x, y)` reads a child. Children are read in Java's order and only where Java reads them:
/// `Steps` reads its source, then slope min, slope max and step count; `Freq` its x and y frequencies (at the
/// original point), then the source at `(x * fx, y * fy)`; `Grad` its source and upper bound, then (unless the
/// source is above the upper bound) strength and lower bound; `Modulate` its direction and strength, then the source
/// at the point moved by `NoiseUtil.sin`/`cos` of `direction * PI2` times the strength. `(int)` casts saturate (NaN
/// to 0) like Java's.
fn transform_value<E>(
    kind: &ModuleKind,
    x: f32,
    y: f32,
    map: fn(f32, f32, f32, f32) -> f32,
    mut sample: impl FnMut(&Module, f32, f32) -> Result<f32, E>,
) -> Result<f32, E> {
    Ok(match kind {
        ModuleKind::Steps {
            source,
            steps,
            slope_min,
            slope_max,
            curve,
        } => {
            let noise_value = sample(source, x, y)?;
            let min = sample(slope_min, x, y)?;
            let max = sample(slope_max, x, y)?;
            let step_count = sample(steps, x, y)?;
            let range = max - min;
            if range <= 0.0 {
                return Ok((noise_value * step_count) as i32 as f32 / step_count);
            }
            let inverted = 1.0 - noise_value;
            let value = (inverted * step_count) as i32 as f32 / step_count;
            let delta = inverted - value;
            let alpha = map(delta * step_count, min, max, range);
            1.0 - lerp(value, inverted, curve.apply(alpha))
        }
        ModuleKind::Frequency {
            source,
            x: x_frequency,
            y: y_frequency,
        } => {
            let x_frequency = sample(x_frequency, x, y)?;
            let y_frequency = sample(y_frequency, x, y)?;
            sample(source, x * x_frequency, y * y_frequency)?
        }
        ModuleKind::Gradient {
            source,
            lower,
            upper,
            strength,
        } => {
            let noise_value = sample(source, x, y)?;
            let upper_bound = sample(upper, x, y)?;
            if noise_value > upper_bound {
                return Ok(noise_value);
            }
            let amount = sample(strength, x, y)?;
            let lower_bound = sample(lower, x, y)?;
            if noise_value < lower_bound {
                return Ok(java_pow(noise_value, 1.0 - amount));
            }
            let alpha = 1.0 - (noise_value - lower_bound) / (upper_bound - lower_bound);
            java_pow(noise_value, 1.0 - amount * alpha)
        }
        ModuleKind::Modulate {
            source,
            direction,
            strength,
        } => {
            let angle = sample(direction, x, y)? * PI2;
            let strength = sample(strength, x, y)?;
            let dx = sin(angle) * strength;
            let dy = cos(angle) * strength;
            sample(source, x + dx, y + dy)?
        }
        _ => unreachable!("transform_value only takes Steps, Freq, Grad and Modulate"),
    })
}

/// `NoiseUtil.pow(float, int)`: repeated float products (`value * value * value` for 3), not `Math.pow`; a negative
/// power gives 1.
pub fn pow_int(value: f32, power: i32) -> f32 {
    match power {
        0 => 1.0,
        1 => value,
        2 => value * value,
        3 => value * value * value,
        4 => value * value * value * value,
        _ => {
            let mut result = 1.0;
            for _ in 0..power {
                result *= value;
            }
            result
        }
    }
}

/// `NoiseUtil.copySign`: negates `value` only when its sign and `sign`'s differ by a strict comparison with 0, so
/// zeros, NaNs and a zero or NaN `sign` leave it unchanged (unlike `f32::copysign`).
pub fn copy_sign(value: f32, sign: f32) -> f32 {
    if (sign < 0.0 && value > 0.0) || (sign > 0.0 && value < 0.0) {
        -value
    } else {
        value
    }
}

/// Java's `Math.max(float, float)`: NaN wins and +0.0 is larger than -0.0.
pub fn java_max(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_positive() || second.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else if first >= second {
        first
    } else {
        second
    }
}

pub fn java_min(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_negative() || second.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else if first <= second {
        first
    } else {
        second
    }
}

/// `NoiseUtil.PI2` and `radFull`: `(float) Math.PI * 2` (the cast binds first; doubling is exact either way).
pub const PI2: f32 = std::f64::consts::PI as f32 * 2.0;
const SIN_MASK: i32 = 4095;
const SIN_COUNT: usize = 4096;

/// `NoiseUtil.SIN`, built once in the order of Engine's static initializer.
fn sin_table() -> &'static [f32; SIN_COUNT] {
    static TABLE: OnceLock<[f32; SIN_COUNT]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0; SIN_COUNT];
        for (i, entry) in table.iter_mut().enumerate() {
            let angle = (i as f32 + 0.5) / SIN_COUNT as f32 * PI2;
            *entry = f64::from(angle).sin() as f32;
        }
        let deg_to_index = SIN_COUNT as f32 / 360.0;
        for degrees in (0..360).step_by(90) {
            let index = ((degrees as f32 * deg_to_index) as i32 & SIN_MASK) as usize;
            table[index] = (f64::from(degrees) * std::f64::consts::PI / 180.0).sin() as f32;
        }
        table
    })
}

/// `NoiseUtil.sin`: a 4096-entry lookup (`as i32` saturates and maps NaN to 0, like Java's `(int)`).
pub fn sin(radians: f32) -> f32 {
    let rad_to_index = SIN_COUNT as f32 / PI2;
    sin_table()[((radians * rad_to_index) as i32 & SIN_MASK) as usize]
}

/// `NoiseUtil.cos`: `sin(r + 1.5708f)`.
pub fn cos(radians: f32) -> f32 {
    sin(radians + f32::from_bits(0x3fc9_0ff9))
}

/// `Cache`'s lookup for both sampling paths: a hit needs the same `seed` key (`None` for the seedless path) and the
/// same coordinate bits. The lock is only tried, and only while the entry is copied in or out, never while `compute`
/// runs: a busy cache samples its source, threads never wait on each other and a panicking source cannot poison it.
fn cache_lookup<E>(
    entry: &std::sync::Mutex<Option<CacheValue>>,
    seed: Option<i32>,
    x: f32,
    y: f32,
    compute: impl FnOnce() -> Result<f32, E>,
) -> Result<f32, E> {
    if let Ok(Some(cached)) = entry.try_lock().as_deref()
        && cached.seed == seed
        && (cached.x.to_bits(), cached.y.to_bits()) == (x.to_bits(), y.to_bits())
    {
        return Ok(cached.value);
    }
    let value = compute()?;
    if let Ok(mut cached) = entry.try_lock() {
        *cached = Some(CacheValue { seed, x, y, value });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
    }

    /// Like `bits`, but every NaN becomes 0x7fc00000: the sign and payload of a NaN depend on the CPU.
    fn canonical_bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values
            .into_iter()
            .map(|v| if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
            .collect()
    }

    #[test]
    fn extreme_coordinates_wrap_like_engine_0_3_0() {
        // Java ints wrap where Rust's debug builds would panic. Expected values come from Engine 0.3.0.
        let points = [
            (-3.0e9, 3.0e9),
            (3.0e9, -3.0e9),
            (f32::MIN, f32::MAX),
            (f32::MAX, f32::MIN),
            (-2.0, -2.0),
        ];
        assert_eq!(
            points.map(|(x, _)| floor(x)),
            [i32::MAX, i32::MAX, i32::MAX, i32::MAX, -3]
        );
        const NAN: u32 = 0x7fc0_0000;
        assert_eq!(
            canonical_bits(points.map(|(x, y)| perlin(x, y, 918273, Interpolation::Curve3))),
            [0xff800000, 0x7f800000, 0xff800000, NAN, 0]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| perlin2(x, y, 918273, Interpolation::Curve4))),
            [NAN, 0xff800000, NAN, NAN, 0]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| simplex(x, y, 918273))),
            [0, 0, 0, 0, 0xbe92101e]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| legacy_simplex(x, y, 918273))),
            [0, 0, 0, 0, 0xbe69b364]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| cubic(x, y, 918273))),
            [NAN, 0x7f800000, NAN, NAN, 0xbeb9b92b]
        );
    }

    #[test]
    fn grid_sweep_matches_engine_0_3_0() {
        // Every source and interpolation over a 48x48 grid of negative and positive points for four seeds, folded
        // into one FNV-1a style checksum of the raw bits. Engine 0.3.0 runs the same loop and prints 0x79cb944a.
        let mut acc: u32 = 0x811c_9dc5;
        let mut mix = |v: u32| acc = (acc ^ v).wrapping_mul(0x0100_0193);
        let mut count = 0;
        for seed in [0, -1, 918273, i32::MIN] {
            for iy in -24..24 {
                for ix in -24..24 {
                    let x = ix as f32 * 0.37 - 0.05;
                    let y = iy as f32 * 0.29 + 0.11;
                    for i in [
                        Interpolation::Linear,
                        Interpolation::Curve3,
                        Interpolation::Curve4,
                    ] {
                        mix(perlin(x, y, seed, i).to_bits());
                        mix(perlin2(x, y, seed, i).to_bits());
                    }
                    mix(simplex(x, y, seed).to_bits());
                    mix(legacy_simplex(x, y, seed).to_bits());
                    mix(cubic(x, y, seed).to_bits());
                    mix(hash_2d(seed, ix, iy) as u32);
                    mix(value_coord_2d(seed, ix, iy).to_bits());
                    mix(floor(x) as u32);
                    count += 1;
                }
            }
        }
        assert_eq!((count, acc), (9216, 0x79cb_944a));
    }

    /// A node `try_value_seed_first` rejects: a custom module (a Java class implementing `Module`), which has no
    /// seed parameter to pass the runtime seed to.
    fn unsupported_seeded_node(value: f32) -> Module {
        Module::custom(value, value, move |_, _| value)
    }

    #[test]
    fn hashing_matches_engine_0_3_0() {
        let inputs = [(0, 0, 0), (42, -17, 91), (i32::MIN, 1_234_567, -7_654_321)];
        assert_eq!(
            inputs.map(|(s, x, y)| hash_2d(s, x, y) as u32),
            [0, 0x40612059, 0x0572ac7c]
        );
        assert_eq!(
            bits(inputs.map(|(s, x, y)| value_coord_2d(s, x, y))),
            [0, 0x3f00c646, 0x3d2e50fd]
        );
    }

    #[test]
    fn sources_match_engine_0_3_0() {
        assert_eq!(Interpolation::Linear.apply(0.25).to_bits(), 0x3e800000);
        let points = [
            (0.0, 0.0),
            (0.125, -0.75),
            (-17.25, 91.5),
            (12345.678, -9876.543),
        ];
        assert_eq!(
            bits(points.map(|(x, y)| perlin(x, y, 918273, Interpolation::Curve3))),
            [0x80000000, 0x3e6ad000, 0, 0x3eead8e4]
        );
        assert_eq!(
            bits(points.map(|(x, y)| perlin2(x, y, 918273, Interpolation::Curve4))),
            [0, 0x3d84e722, 0xbdf8df90, 0xbed422fc]
        );
        assert_eq!(
            bits(points.map(|(x, y)| simplex(x, y, 918273))),
            [0, 0xbf0c83ca, 0x3d2f52b0, 0xbf2e3484]
        );
        assert_eq!(
            bits(points.map(|(x, y)| legacy_simplex(x, y, 918273))),
            [0, 0xbee0d2dd, 0x3d0c4227, 0xbf0b5d37]
        );
        assert_eq!(
            bits(points.map(|(x, y)| cubic(x, y, 918273))),
            [0xbea370a8, 0x3ea40cd4, 0xbe4d83e2, 0x3d2b86ab]
        );
    }

    #[test]
    fn octave_sources_match_engine_0_3_0() {
        let config = SourceConfig {
            seed: 8128,
            octaves: 5,
            gain: 0.47,
            frequency: 0.0078125,
            lacunarity: 2.13,
            interpolation: Interpolation::Curve4,
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                SourceKind::Perlin,
                [0x3f000000, 0x3ecb61fa, 0x3eb19ea9, 0x3f352d0f],
                [0x3f000000, 0x3ebc4868, 0x3ef9e565, 0x3ea831a9],
            ),
            (
                SourceKind::Perlin2,
                [0x3f000000, 0x3f00ddf5, 0x3edd96ab, 0x3f186b18],
                [0x3f000000, 0x3ebc4868, 0x3ef9e565, 0x3ea831a9],
            ),
            (
                SourceKind::Simplex,
                [0x3f000000, 0x3f14e4f8, 0x3f168604, 0x3f5d81ab],
                [0x3f000000, 0x3f351311, 0x3f12ee34, 0x3f3d7a61],
            ),
            (
                SourceKind::Simplex2,
                [0x3f000000, 0x3f1a1e36, 0x3f1c2785, 0x3f74e215],
                [0x3f000000, 0x3f4257d5, 0x3f17a9c1, 0x3f4cd8fa],
            ),
            (
                SourceKind::Cubic,
                [0x3f097eca, 0x3f1ca60a, 0x3ef5738d, 0x3f19b1d4],
                [0x3f0bac93, 0x3f04f827, 0x3ee8f0cb, 0x3ef94bc2],
            ),
        ];
        for (kind, expected, expected_seeded) in cases {
            let source = Source::new(kind, config);
            assert_eq!(bits(points.map(|(x, y)| source.value(x, y))), expected);
            assert_eq!(
                bits(points.map(|(x, y)| source.value_seeded(x, y, -991))),
                expected_seeded
            );
        }
    }

    #[test]
    fn source_bounds_and_edge_configs_match_engine_0_3_0() {
        // Negative gain (PerlinNoise's `Math.abs` range), more octaves than the signal table, a seed that
        // wraps per octave, zero octaves (0/0 = NaN after `map`) and a plain two-octave config.
        let configs = [
            (2147483646, 9, -0.6, 0.031, 1.9, Interpolation::Curve4),
            (-7, 0, 0.5, 0.25, 2.0, Interpolation::Linear),
            (1337, 2, 0.5, 1.0, 2.0, Interpolation::Curve3),
        ];
        let kinds = [
            SourceKind::Perlin,
            SourceKind::Perlin2,
            SourceKind::Simplex,
            SourceKind::Simplex2,
            SourceKind::Cubic,
        ];
        let points = [(0.0, 0.0), (3.5, -17.25), (-1000.125, 250.75)];
        // Per config and kind: minValue, maxValue, getValue(x, y) and getValue(x, y, Integer.MAX_VALUE).
        let expected: [[(u32, u32, [u32; 3], [u32; 3]); 5]; 3] = [
            [
                (0, 0x3f800000, [0; 3], [0; 3]),
                (0, 0x3f800000, [0; 3], [0; 3]),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0, 0],
                    [0x3f000000, 0, 0x3ded045d],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0, 0],
                    [0x3f000000, 0, 0x3ca115dd],
                ),
                (
                    0xbef26b2d,
                    0x3ef26b2d,
                    [0x3ebd193a, 0x3ed8de12, 0x3e98209c],
                    [0x3f35b51d, 0x3f3d793c, 0],
                ),
            ],
            [
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (
                    0xbf400000,
                    0x3f400000,
                    [0x3efe88c4, 0x3f032de8, 0x3ed472b4],
                    [0x3f4bd987, 0x3ee2e739, 0x3e8f95ed],
                ),
            ],
            [
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3f2e2f4b, 0x3f085555],
                    [0x3f000000, 0x3f01346f, 0x3f192829],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3eec18aa, 0x3f06c11b],
                    [0x3f000000, 0x3f01346f, 0x3f192829],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3efc1cbf, 0x3f415051],
                    [0x3f000000, 0x3eef7b45, 0x3e10910a],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3efb23ee, 0x3f51a465],
                    [0x3f000000, 0x3eeb5a17, 0x3d52d533],
                ),
                (
                    0xbf900000,
                    0x3f900000,
                    [0x3ecc2ffc, 0x3f162025, 0x3f1fb20c],
                    [0x3f329104, 0x3ee08b97, 0x3f2547da],
                ),
            ],
        ];
        // Any NaN matches NAN: Java printed x86's default 0xffc00000, which rustc may constant-fold differently.
        const NAN: u32 = 0x7fc00000;
        let canon = |v: f32| if v.is_nan() { NAN } else { v.to_bits() };
        for ((seed, octaves, gain, frequency, lacunarity, interpolation), rows) in
            configs.into_iter().zip(expected)
        {
            let config = SourceConfig {
                seed,
                octaves,
                gain,
                frequency,
                lacunarity,
                interpolation,
            };
            for (kind, (min, max, plain, seeded)) in kinds.into_iter().zip(rows) {
                let source = Source::new(kind, config);
                assert_eq!(source.min_value().to_bits(), min);
                assert_eq!(source.max_value().to_bits(), max);
                assert_eq!(points.map(|(x, y)| canon(source.value(x, y))), plain);
                assert_eq!(
                    points.map(|(x, y)| canon(source.value_seeded(x, y, i32::MAX))),
                    seeded
                );
            }
        }
    }

    #[test]
    fn erosion_ridge_matches_engine_0_3_0() {
        let perlin = |seed, scale: f32, octaves| {
            Module::source(Source::new(
                SourceKind::Perlin,
                SourceConfig {
                    seed,
                    octaves,
                    gain: 0.5,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let source = perlin(991, 200.0, 4).warp(Domain::domain_warp(
            perlin(77, 50.0, 2),
            perlin(78, 50.0, 2),
            Module::constant(20.0),
        ));
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                ErosionRidge::full(123, 2, 0.65, 128.0, 0.15, 3.1, 0.8, ErosionMode::Constant),
                source.clone(),
                [0x3e7b3143, 0x3ed0cd11, 0x3e9902f0, 0x3e94ff56],
                [0x3ddd83e0, 0x3ebbbbbc, 0x3e2ef49e, 0x3e75ed6f],
            ),
            (
                ErosionRidge::full(
                    -456,
                    3,
                    0.4,
                    96.0,
                    0.2,
                    2.25,
                    0.75,
                    ErosionMode::InputLinear,
                ),
                source.clone(),
                [0x3f131dca, 0x3ed624a8, 0x3ee20805, 0x3eb8f883],
                [0x3f5f94f2, 0x3e13044a, 0x3dbf7efc, 0x3e6a298c],
            ),
            (
                ErosionRidge::with_octaves(789, 2, 0.8, 64.0, ErosionMode::OutputLinear),
                source,
                [0x3ee92098, 0x3ee00a0d, 0x3ef53cd6, 0x3eb63175],
                [0x3e2c711f, 0x3e6628f0, 0x3e8895d0, 0x3e51cc46],
            ),
            (
                ErosionRidge::new(321, 0.7, 80.0, ErosionMode::Constant),
                Module::constant(0.35),
                [0x3e01136a, 0x3e119ee2, 0x3e821828, 0x3e8eee68],
                [0x3cf65a30, 0x3d59b7c8, 0x3e5a1922, 0x3e7ec66a],
            ),
        ];
        for (ridge, source, expected, expected_erosion) in cases {
            let wrapped = ridge.wrap(source.clone());
            assert_eq!(
                [wrapped.min_value().to_bits(), wrapped.max_value().to_bits()],
                [0, 0x3f800000]
            );
            assert_eq!(bits(points.map(|(x, y)| wrapped.value(x, y))), expected);
            assert_eq!(
                bits(points.map(|(x, y)| ridge.erosion_value(x, y, &source))),
                expected_erosion
            );
        }
    }

    /// `ErosionRidgeOracle`'s rows: per case, the wrapped module's bounds and FNV-1a hashes of the raw bits (NaN
    /// canonicalized to 0x7fc00000) of `Ridge.Noise.getValue` and `Ridge.getErosionValue` over `GRID` x `GRID`, then
    /// over `EXTREME`.
    #[allow(clippy::type_complexity)]
    #[rustfmt::skip]
    const EROSION_RIDGE_ORACLE: [(&str, u32, u32, u64, u64, u64, u64); 12] = [
        ("landforms_mountains", 0x00000000, 0x3f800000, 0x003e2fcff2a8d2a5, 0xa58fa7626ca1f22e, 0x3d364bfd7ac192fd, 0xfbbc4c4ef997f82b),
        ("input_linear", 0x00000000, 0x3f800000, 0x206ec839b3d01d9f, 0xf4fab5cea517dc28, 0xb8c213958d3ad18b, 0x2d2d4528ffbaad20),
        ("output_linear", 0x00000000, 0x3f800000, 0xe9b0a201a5d22578, 0x3f56b4e87f12bb57, 0xea565b1588a5fe1f, 0x76c75d4b16c672a6),
        ("constant_source", 0x00000000, 0x3f800000, 0x35976c5c4da5269d, 0xf50ea9a5b6d4b595, 0x17004c30fac8f031, 0x35e7cb73cd588278),
        ("min_seed_sentinel", 0x00000000, 0x3f800000, 0xadf1ee001797deba, 0x864c542aa98214b0, 0x7e212cb491435c6c, 0xfd8d5433f52499f6),
        ("nan_source", 0x00000000, 0x3f800000, 0x12956ce69e90b3c5, 0x234a240d9f3155c5, 0xb9e7322deb9a5665, 0xeb87129436c8da65),
        ("zero_octaves", 0x00000000, 0x3f800000, 0x12956ce69e90b3c5, 0x12956ce69e90b3c5, 0xb9e7322deb9a5665, 0xb9e7322deb9a5665),
        ("negative_octaves", 0x00000000, 0x3f800000, 0x12956ce69e90b3c5, 0x12956ce69e90b3c5, 0xb9e7322deb9a5665, 0xb9e7322deb9a5665),
        ("negative_grid", 0x00000000, 0x3f800000, 0xd6d23f5fb2d4d9c9, 0x2c1b93daafb34265, 0xe701b35dbf2fafe5, 0x0c8210784d8af5a5),
        ("zero_grid", 0x00000000, 0x3f800000, 0xcd9c395a4002bc6d, 0x234a240d9f3155c5, 0x43ae21051c064ba5, 0xeb87129436c8da65),
        ("tiny_grid", 0x00000000, 0x3f800000, 0x5f1568a5a49a71cc, 0xf4d94b73b57e7103, 0x26c16afccadfb90b, 0xf2946cd9fc52be8e),
        ("many_octaves", 0x00000000, 0x3f800000, 0x120b51670657bd9d, 0x1497217c082f5320, 0xafe0b5d983daa051, 0xe000c30eec299f79),
    ];

    #[test]
    fn erosion_ridge_grid_matches_engine_0_3_0() {
        const GRID: [f32; 10] = [
            0.0, 12.25, -98.75, -4096.5, 8192.25, 123456.75, -0.125, -77777.5, 1e6, -3.0e5,
        ];
        const EXTREME: [(f32, f32); 8] = [
            (3.0e9, -3.0e9),
            (-3.0e9, 2.5e9),
            (f32::NAN, 5.5),
            (1.0, f32::NAN),
            (f32::INFINITY, 1.0),
            (-7.0, f32::NEG_INFINITY),
            (-0.0, -0.0),
            (f32::MAX, -f32::MAX),
        ];
        fn fnv(hash: u64, value: f32) -> u64 {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            bits.to_le_bytes().iter().fold(hash, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            })
        }
        let perlin = |seed, scale: f32, octaves| {
            Module::source(Source::new(
                SourceKind::Perlin,
                SourceConfig {
                    seed,
                    octaves,
                    gain: 0.5,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let warped = perlin(991, 200.0, 4).warp(Domain::domain_warp(
            perlin(77, 50.0, 2),
            perlin(78, 50.0, 2),
            Module::constant(20.0),
        ));
        use ErosionMode::{Constant, InputLinear, OutputLinear};
        let cases = [
            (
                ErosionRidge::full(123, 2, 0.65, 128.0, 0.15, 3.1, 0.8, Constant),
                warped.clone(),
            ),
            (
                ErosionRidge::full(-456, 3, 0.4, 96.0, 0.2, 2.25, 0.75, InputLinear),
                warped.clone(),
            ),
            (
                ErosionRidge::with_octaves(789, 2, 0.8, 64.0, OutputLinear),
                warped.clone(),
            ),
            (
                ErosionRidge::new(321, 0.7, 80.0, Constant),
                Module::constant(0.35),
            ),
            (
                ErosionRidge::with_octaves(i32::MIN, 2, 1.5, 1.0, InputLinear),
                Module::constant(-1.0),
            ),
            (
                ErosionRidge::new(5, 0.5, 40.0, OutputLinear),
                Module::constant(f32::NAN),
            ),
            (
                ErosionRidge::with_octaves(5, 0, 0.5, 40.0, Constant),
                warped.clone(),
            ),
            (
                ErosionRidge::with_octaves(5, -3, 0.5, 40.0, InputLinear),
                warped.clone(),
            ),
            (
                ErosionRidge::with_octaves(i32::MAX, 2, -0.25, -50.0, OutputLinear),
                warped.clone(),
            ),
            (
                ErosionRidge::with_octaves(17, 1, 0.5, 0.0, Constant),
                warped.clone(),
            ),
            (
                ErosionRidge::full(-1, 2, 0.9, 1e-3, 0.5, 2.25, 0.75, InputLinear),
                warped.clone(),
            ),
            (
                ErosionRidge::with_octaves(42, 6, 0.3, 300.0, Constant),
                warped,
            ),
        ];
        for ((ridge, source), (name, min, max, value, erosion, x_value, x_erosion)) in
            cases.into_iter().zip(EROSION_RIDGE_ORACLE)
        {
            let module = ridge.wrap(source.clone());
            let start = 0xcbf2_9ce4_8422_2325_u64;
            let (mut got_value, mut got_erosion) = (start, start);
            for x in GRID {
                for y in GRID {
                    got_value = fnv(got_value, module.value(x, y));
                    got_erosion = fnv(got_erosion, ridge.erosion_value(x, y, &source));
                }
            }
            let (mut got_x_value, mut got_x_erosion) = (start, start);
            for (x, y) in EXTREME {
                got_x_value = fnv(got_x_value, module.value(x, y));
                got_x_erosion = fnv(got_x_erosion, ridge.erosion_value(x, y, &source));
            }
            let got = (
                module.min_value().to_bits(),
                module.max_value().to_bits(),
                got_value,
                got_erosion,
                got_x_value,
                got_x_erosion,
            );
            assert_eq!(
                got,
                (min, max, value, erosion, x_value, x_erosion),
                "{name}"
            );
        }
        // `1.0f / (float)(octaves + 1)` wraps like Java's `int`.
        assert_eq!(
            ErosionRidge::with_octaves(0, i32::MAX, 1.0, 1.0, Constant)
                .amplitude
                .to_bits(),
            0xb000_0000
        );
    }

    #[test]
    fn cellular_primitives_match_engine_0_3_0() {
        let table_hash = CELL_2D.iter().fold(1_i32, |hash, vector| {
            hash.wrapping_mul(31)
                .wrapping_add(vector[0].to_bits() as i32)
                .wrapping_mul(31)
                .wrapping_add(vector[1].to_bits() as i32)
        });
        assert_eq!(table_hash as u32, 0xbbfa7b06);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let expected_cells = [
            0x3e81d418, 0x3f73b7e0, 0xbd584f85, 0x3ea77c26, 0xbf78befa, 0x3f70cd15, 0x3ca6f05a,
            0x3e9c4488,
        ];
        for distance in [
            DistanceFunction::Euclidean,
            DistanceFunction::Manhattan,
            DistanceFunction::Natural,
        ] {
            assert_eq!(
                bits(points.map(|(x, y)| cell_value(x, y, 1201, 0.9, distance))),
                expected_cells
            );
        }
        let edge_cases = [
            (
                EdgeFunction::Distance2,
                [
                    0xbea2157a, 0x3fa9a0b8, 0x3dd311a0, 0x3e84a8e0, 0x3f2e1fcc, 0xbd44f470,
                    0x3f003e46, 0x3d9283a0,
                ],
            ),
            (
                EdgeFunction::Distance2Add,
                [
                    0x3d3dd9c0, 0x4002d23c, 0x3f309854, 0x3f572bf4, 0x3fc080a4, 0x3e9d5938,
                    0x3fb38518, 0x3f37e12c,
                ],
            ),
            (
                EdgeFunction::Distance2Sub,
                [
                    0xbf2df315, 0x3f1b39ee, 0xbef7a7d8, 0xbea50626, 0xbe1306b8, 0xbece9656,
                    0xbecd1b4c, 0xbf134045,
                ],
            ),
            (
                EdgeFunction::Distance2Mul,
                [
                    0xbf408112, 0x3f2be87e, 0xbeb49d84, 0xbe892faa, 0x3ec4a1d8, 0xbf296418,
                    0x3eb4d40c, 0xbe9d2e24,
                ],
            ),
            (
                EdgeFunction::Distance2Div,
                [
                    0xbef01d20, 0xbf30dae1, 0xbeefa56c, 0xbf09c986, 0xbf027d17, 0xbf206b8e,
                    0xbecc7750, 0xbecafa16,
                ],
            ),
        ];
        for (edge, expected) in edge_cases {
            assert_eq!(
                bits(points.map(|(x, y)| {
                    cell_edge(x, y, 1202, 0.75, edge, DistanceFunction::Natural)
                })),
                expected
            );
        }
    }

    #[test]
    fn cellular_extremes_and_ties_match_engine_0_3_0() {
        // Lattice ints that wrap past i32::MAX (Java's `xi + dx`), NaN and infinite coordinates, and zero
        // displacement, where neighbouring cells tie and only the first strictly nearer one wins.
        let points = [
            (3.0e9, -3.0e9),
            (2147483520.0, -2147483648.0),
            (f32::NAN, 5.5),
            (f32::INFINITY, f32::NEG_INFINITY),
            (0.5, 0.5),
            (-0.5, 7.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
        ];
        const NAN: u32 = 0x7fc00000;
        let canon = |v: f32| if v.is_nan() { NAN } else { v.to_bits() };
        let edges = [
            EdgeFunction::Distance2,
            EdgeFunction::Distance2Add,
            EdgeFunction::Distance2Sub,
            EdgeFunction::Distance2Mul,
            EdgeFunction::Distance2Div,
        ];
        // Java's `31 * h + bits` over every cell and cellEdge sample, per distance function, in the order
        // seed, displacement, then CELL_VALUE followed by each EdgeFunc.
        let cases = [
            (DistanceFunction::Euclidean, 0xdd682a28_u32),
            (DistanceFunction::Manhattan, 0x78dddcdf),
            (DistanceFunction::Natural, 0x98b233f2),
        ];
        for (distance, expected) in cases {
            let mut hash = 1_i32;
            let mut mix = |v: f32| hash = hash.wrapping_mul(31).wrapping_add(canon(v) as i32);
            for seed in [1201, i32::MIN] {
                for displacement in [0.0, 0.9, -1.5] {
                    for (x, y) in points {
                        mix(cell_value(x, y, seed, displacement, distance));
                    }
                    for edge in edges {
                        for (x, y) in points {
                            mix(cell_edge(x, y, seed, displacement, edge, distance));
                        }
                    }
                }
            }
            assert_eq!(hash as u32, expected);
        }
        let row = |f: &dyn Fn(f32, f32) -> f32| points.map(|(x, y)| canon(f(x, y)));
        assert_eq!(
            row(&|x, y| cell_value(x, y, i32::MIN, 0.0, DistanceFunction::Manhattan)),
            [
                0x3ec2d266, 0xbf470fc7, 0x3f1ff100, 0xbf561ba4, 0, 0x3e584b85, 0xbf5f69e4,
                0x3dfbaa33,
            ]
        );
        assert_eq!(
            row(&|x, y| cell_edge(
                x,
                y,
                1201,
                0.0,
                EdgeFunction::Distance2,
                DistanceFunction::Euclidean
            )),
            [
                0x5da160e5, 0xbf800000, 0x7f7fffff, 0x7f7fffff, 0xbf000000, 0xbf400000, 0xbec00000,
                0xbf300000,
            ]
        );
        assert_eq!(
            row(&|x, y| cell_edge(
                x,
                y,
                i32::MIN,
                -1.5,
                EdgeFunction::Distance2Div,
                DistanceFunction::Natural
            )),
            [0, NAN, 0, 0, 0xbf2ad6c4, 0xbebf4dfa, 0xbf13465d, 0xbee2b4a8]
        );
        let bounds = edges.map(|edge| {
            let (min, max, range) = edge.bounds();
            [min.to_bits(), max.to_bits(), range.to_bits()]
        });
        assert_eq!(
            bounds,
            [
                [0xbf800000, 0x3f800000, 0x40000000],
                [0xbf800000, 0x3fcccccd, 0x40266666],
                [0xbf800000, 0x3f4ccccd, 0x3fe66666],
                [0xbf800000, 0x3f333333, 0x3fd9999a],
                [0xbf800000, 0, 0x3f800000],
            ]
        );
    }

    #[test]
    fn cellular_sources_match_engine_0_3_0() {
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cell_config = SourceConfig {
            seed: 1301,
            octaves: 4,
            gain: 0.3,
            frequency: 1.0 / 96.0,
            lacunarity: 3.0,
            interpolation: Interpolation::Curve3,
        };
        let lookup = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 1303,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
        .map(Module::constant(-0.5), Module::constant(1.5));
        let cases = [
            (
                Module::cell_source(
                    cell_config,
                    0.85,
                    CellFunction::CellValue,
                    DistanceFunction::Manhattan,
                    Module::constant(0.0),
                ),
                [
                    0x3ef9506c, 0x3f20d6b0, 0x3eb03077, 0x3ead7c8c, 0x3f621908, 0x3f5b9c57,
                    0x3f18b79e, 0x3eb5c000,
                ],
            ),
            (
                Module::cell_source(
                    cell_config,
                    0.85,
                    CellFunction::NoiseLookup,
                    DistanceFunction::Natural,
                    lookup,
                ),
                [
                    0x3eecc702, 0x3f14aac2, 0x3f78df12, 0x3dde7898, 0x3f04884a, 0x3eb0e454,
                    0x3f5f8b3c, 0x3f0f3ce4,
                ],
            ),
            (
                Module::cell_source(
                    cell_config,
                    0.85,
                    CellFunction::Distance,
                    DistanceFunction::Euclidean,
                    Module::constant(0.0),
                ),
                [0; 8],
            ),
        ];
        for (module, expected) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                [0, 0x3f800000]
            );
            assert_eq!(bits(points.map(|(x, y)| module.value(x, y))), expected);
        }

        let edge_config = SourceConfig {
            seed: 1302,
            octaves: 5,
            gain: 0.2,
            frequency: 1.0 / 80.0,
            lacunarity: 4.0,
            interpolation: Interpolation::Curve3,
        };
        let edge_cases = [
            (
                EdgeFunction::Distance2,
                [
                    0x3f50c706, 0x3f3bb0b2, 0x3f510a1e, 0x3f14d227, 0x3f0c0452, 0x3f800000,
                    0x3f54a0ca, 0x3f579e1a,
                ],
            ),
            (
                EdgeFunction::Distance2Add,
                [
                    0x3f800000, 0x3f3cd56e, 0x3f4ba8b7, 0x3f009a9a, 0x3f25deec, 0x3f7ea082,
                    0x3f800000, 0x3f56957a,
                ],
            ),
            (
                EdgeFunction::Distance2Sub,
                [
                    0x3db09687, 0x3f105460, 0x3f2a5bad, 0x3f10f3b2, 0x3e8f1d6d, 0x3f539dd2,
                    0x3eb6d292, 0x3f293219,
                ],
            ),
            (
                EdgeFunction::Distance2Mul,
                [
                    0x3f800000, 0x3ec76615, 0x3ed619e6, 0x3dc8f431, 0x3ec29f0c, 0x3f286c6f,
                    0x3f7ec242, 0x3efb0ebf,
                ],
            ),
            (
                EdgeFunction::Distance2Div,
                [
                    0x3f67a3f1, 0x3e9da7b6, 0x3e8877d4, 0x3dfcb7f8, 0x3f0a4044, 0x3e8a01f6,
                    0x3f1cf2c9, 0x3e9668b6,
                ],
            ),
        ];
        for (edge, expected) in edge_cases {
            let module =
                Module::cell_edge_source(edge_config, 0.7, edge, DistanceFunction::Natural);
            assert_eq!(bits(points.map(|(x, y)| module.value(x, y))), expected);
        }
    }

    #[test]
    fn cellular_source_extremes_match_engine_0_3_0() {
        // Negative and minimal seeds, negative displacement, every metric, and NaN, infinite, huge and -0.0
        // coordinates.
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let extreme = [
            (3.0e9, -3.0e9),
            (f32::NAN, 5.5),
            (f32::INFINITY, 1.0),
            (-0.0, -0.0),
        ];
        let config = |seed, frequency| SourceConfig {
            seed,
            octaves: 4,
            gain: 0.3,
            frequency,
            lacunarity: 3.0,
            interpolation: Interpolation::Curve3,
        };
        let lookup = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 1303,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
        .map(Module::constant(-0.5), Module::constant(1.5));
        let cases = [
            (
                DistanceFunction::Euclidean,
                [
                    0x3f000000, 0x3e3ba476, 0x3e44c392, 0x3f44ff90, 0x3f2846d0, 0x3ecff46c,
                    0x3d3a0000, 0x3ebf3a86,
                ],
                [
                    0x00000000, 0x3f5e8cc3, 0x3e50017c, 0x3b1a1c00, 0x3eb9a76a, 0x3eb696cc,
                    0x3f6f272f, 0x3df90e18,
                ],
                [0x3f800000, 0x3f800000, 0x3f800000, 0x3e846d37],
            ),
            (
                DistanceFunction::Manhattan,
                [
                    0x3da79170, 0x3e3ba476, 0x3e44c392, 0x3f44ff90, 0x3f2846d0, 0x3ecff46c,
                    0x3e8fcdb4, 0x3ebf3a86,
                ],
                [
                    0x00000000, 0x3f6f46b6, 0x3f02a617, 0x3d46a060, 0x3f23d700, 0x3f26667a,
                    0x3f78e1cd, 0x3ed171e2,
                ],
                [0x3f800000, 0x3f800000, 0x3f800000, 0x3ed0962d],
            ),
            (
                DistanceFunction::Natural,
                [
                    0x3da79170, 0x3e3ba476, 0x3e44c392, 0x3f44ff90, 0x3f2846d0, 0x3ecff46c,
                    0x3d3a0000, 0x3ebf3a86,
                ],
                [
                    0x00000000, 0x3f69b377, 0x3ec96e50, 0x3cd4bb80, 0x3f0a355a, 0x3f0bf02c,
                    0x3f75fe0c, 0x3e964078,
                ],
                [0x3f800000, 0x3f800000, 0x3f800000, 0x3f2a81b2],
            ),
        ];
        let lookup_bits = [
            0x3edaa908, 0x3eeed8d0, 0x3ea7af50, 0x3f2e894a, 0xbe212e52, 0x3ebc1e7c, 0x3ed45a30,
            0x3e9d2338,
        ];
        for (metric, value_bits, edge_bits, extreme_edge_bits) in cases {
            let cell = |seed, frequency, displacement, function, lookup| {
                Module::cell_source(
                    config(seed, frequency),
                    displacement,
                    function,
                    metric,
                    lookup,
                )
            };
            let value = cell(
                i32::MIN,
                0.37,
                -1.25,
                CellFunction::CellValue,
                Module::constant(0.0),
            );
            let noise = cell(-77, 0.37, -1.25, CellFunction::NoiseLookup, lookup.clone());
            let edge = Module::cell_edge_source(
                config(-77, 0.37),
                0.0,
                EdgeFunction::Distance2Div,
                metric,
            );
            assert_eq!(bits(points.map(|(x, y)| value.value(x, y))), value_bits);
            assert_eq!(bits(points.map(|(x, y)| noise.value(x, y))), lookup_bits);
            assert_eq!(bits(points.map(|(x, y)| edge.value(x, y))), edge_bits);

            let value = cell(5, 1.0, 1.0, CellFunction::CellValue, Module::constant(0.0));
            let noise = cell(5, 1.0, 1.0, CellFunction::NoiseLookup, lookup.clone());
            let distance = cell(5, 1.0, 1.0, CellFunction::Distance, Module::constant(0.0));
            let edge =
                Module::cell_edge_source(config(5, 1.0), 1.0, EdgeFunction::Distance2, metric);
            assert_eq!(
                bits(extreme.map(|(x, y)| value.value(x, y))),
                [0x3eb612e6, 0x3e981034, 0x3f6b2c3e, 0x3f007362]
            );
            assert_eq!(
                bits([extreme[0], extreme[3]].map(|(x, y)| noise.value(x, y))),
                [0x3f000000, 0x3f0034e4]
            );
            assert_eq!(bits(extreme.map(|(x, y)| distance.value(x, y))), [0; 4]);
            assert_eq!(
                bits(extreme.map(|(x, y)| edge.value(x, y))),
                extreme_edge_bits
            );
            for module in [value, noise, distance, edge] {
                assert_eq!([module.min_value(), module.max_value()], [0.0, 1.0]);
            }
        }
        // `Builder`'s default lookup is `Source.ZERO`.
        let default_lookup = Module::cell_source(
            config(1337, 1.0),
            1.0,
            CellFunction::NoiseLookup,
            DistanceFunction::Euclidean,
            Module::constant(0.0),
        );
        assert_eq!(
            bits(points.map(|(x, y)| default_lookup.value(x, y))),
            [0; 8]
        );
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn cell_lookup_panics_where_engine_throws() {
        // Engine: every distance is NaN, so `vec2f` stays null and `NOISE_LOOKUP` throws a NullPointerException.
        Module::cell_source(
            SourceConfig {
                seed: 5,
                octaves: 1,
                gain: 0.5,
                frequency: 1.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
            1.0,
            CellFunction::NoiseLookup,
            DistanceFunction::Euclidean,
            Module::constant(0.0),
        )
        .value(f32::NAN, 5.5);
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn cell_lookup_panics_where_squared_distances_overflow() {
        // Engine: at 1e20 every squared Euclidean distance is infinite, so `NOISE_LOOKUP` throws; Manhattan
        // distances stay finite there and sample the lookup.
        let lookup = |metric| {
            Module::cell_source(
                SourceConfig {
                    seed: 1337,
                    octaves: 1,
                    gain: 0.5,
                    frequency: 1.0,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
                1.0,
                CellFunction::NoiseLookup,
                metric,
                Module::constant(0.0),
            )
        };
        assert_eq!(
            lookup(DistanceFunction::Manhattan)
                .value(1e20, 0.0)
                .to_bits(),
            0
        );
        lookup(DistanceFunction::Euclidean).value(1e20, 0.0);
    }

    #[test]
    fn sine_source_matches_engine_0_3_0() {
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let alpha = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 1401,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 96.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let cases = [
            (
                Module::sine_source(1.0 / 64.0, Module::constant(0.0)),
                [
                    0x3f000000, 0x3f184c14, 0x3d1eea08, 0x3f0983a1, 0x3f6baa7e, 0x3d381108,
                    0x3e964994, 0x3f24afba,
                ],
            ),
            (
                Module::sine_source(1.0 / 64.0, Module::constant(1.0)),
                [
                    0x3f000000, 0x396a8800, 0x3f5bf4a8, 0x3effcdc3, 0x3f6baa7e, 0x3f3d49fe,
                    0x3f710644, 0x3e667e58,
                ],
            ),
            (
                Module::sine_source(1.0 / 64.0, alpha),
                [
                    0x3f000000, 0x3f0c5d95, 0x3ea3ee6e, 0x3f05c011, 0x3f6baa7e, 0x3e89b516,
                    0x3efbd02a, 0x3f16ecca,
                ],
            ),
        ];
        for (module, expected) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                [0, 0x3f800000]
            );
            assert_eq!(bits(points.map(|(x, y)| module.value(x, y))), expected);
        }
    }

    #[test]
    fn sine_source_extremes_match_engine_0_3_0() {
        // Constant alphas off 0 and 1 (extrapolating, -0.0, tiny, NaN, infinite), NaN, infinite, huge, negative and
        // -0.0 coordinates, `Source.sin(scale, alpha)` and the builder's default (alpha `Source.ZERO`, frequency 1).
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let extreme = [
            (3.0e9, -3.0e9),
            (f32::NAN, 5.5),
            (5.5, f32::NAN),
            (f32::INFINITY, 1.0),
            (1.0, f32::NEG_INFINITY),
            (-0.0, -0.0),
            (-1.0e30, 1.0e30),
            (-3.0, -7.25),
        ];
        let alpha = || {
            Module::source(Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    seed: 1401,
                    octaves: 3,
                    gain: 0.5,
                    frequency: 1.0 / 96.0,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let cases: [(f32, f32, [u32; 8], [u32; 8]); 7] = [
            (
                0.5,
                0.37,
                [
                    0x3f000000, 0x3ef79a93, 0x3ebea3f5, 0x3f0c8340, 0x3b615d00, 0x3ec8a94d,
                    0x3ed133e1, 0x3ed23916,
                ],
                [
                    0x3effe6e1, 0x3f393b80, 0x3f393b80, 0x3f171f1e, 0x3f172bae, 0x3f000000,
                    0x3effe6e1, 0x3e29060e,
                ],
            ),
            (
                2.0,
                0.37,
                [
                    0x3f000000, 0x3f800000, 0x3f800000, 0x3eab351b, 0x3b615d00, 0x00000000,
                    0x3f800000, 0x3f800000,
                ],
                [
                    0x3f00191f, 0x3f800000, 0x3d589010, 0x3f5cc7d4, 0x3ea3514a, 0x3f000000,
                    0x3eff9b86, 0x3f00ae6e,
                ],
            ),
            (
                -1.0,
                0.37,
                [
                    0x3f000000, 0x00000000, 0x00000000, 0x3f436bf2, 0x3b615d00, 0x3f800000,
                    0x00000000, 0x00000000,
                ],
                [
                    0x3eff9b86, 0x3d589010, 0x3f800000, 0x3ea2ecd0, 0x3f5caeb6, 0x3f000000,
                    0x3f00191f, 0x00000000,
                ],
            ),
            (
                -0.0,
                0.37,
                [
                    0x3f000000, 0x3c04b560, 0x3b55bf80, 0x3f1ed0d1, 0x3b615d00, 0x3f1dabc2,
                    0x399c2000, 0x3ccbc940,
                ],
                [
                    0x3effcdc3, 0x3f000000, 0x3f7276ff, 0x3effcdc3, 0x3f2e575b, 0x3f000000,
                    0x3f000000, 0x3d5728b0,
                ],
            ),
            (
                1e-30,
                0.37,
                [
                    0x3f000000, 0x3c04b560, 0x3b55bf80, 0x3f1ed0d1, 0x3b615d00, 0x3f1dabc2,
                    0x399c2000, 0x3ccbc940,
                ],
                [
                    0x3effcdc3, 0x3f000000, 0x3f7276ff, 0x3effcdc3, 0x3f2e575b, 0x3f000000,
                    0x3f000000, 0x3d5728b0,
                ],
            ),
            (
                f32::NAN,
                0.37,
                [
                    0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000,
                    0x7fc00000, 0x7fc00000,
                ],
                [
                    0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000,
                    0x7fc00000, 0x7fc00000,
                ],
            ),
            (
                f32::INFINITY,
                0.37,
                [
                    0xffc00000, 0x3f800000, 0x3f800000, 0x00000000, 0xffc00000, 0x00000000,
                    0x3f800000, 0x3f800000,
                ],
                [
                    0x3f800000, 0x3f800000, 0x00000000, 0x3f800000, 0x00000000, 0xffc00000,
                    0x00000000, 0x3f800000,
                ],
            ),
        ];
        for (value, frequency, at_points, at_extremes) in cases {
            let module = Module::sine_source(frequency, Module::constant(value));
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                at_points,
                "alpha {value}"
            );
            assert_eq!(
                bits(extreme.map(|(x, y)| module.value(x, y))),
                at_extremes,
                "alpha {value}"
            );
        }
        let blend = Module::sine_source(3.5, alpha());
        assert_eq!(
            bits(extreme.map(|(x, y)| blend.value(x, y))),
            [
                0x3effe6e1, 0x7fc00000, 0x7fc00000, 0x3ed30740, 0x3ed3205e, 0x3f000000, 0x3effe6e1,
                0x3f151d76
            ]
        );
        // `Source.sin(100, alpha)`: `Builder.scale(100)` sets the frequency to `1f / 100`.
        let scaled = Module::sine_source(1.0 / 100.0, alpha());
        assert_eq!(
            bits(points.map(|(x, y)| scaled.value(x, y))),
            [
                0x3f000000, 0x3f05fcfe, 0x3f14dcc2, 0x3f062561, 0x3f4c7c62, 0x3e67cac2, 0x3f5fc70c,
                0x3f40a824
            ]
        );
        let default = Module::sine_source(1.0, Module::constant(0.0));
        assert_eq!(
            bits(points.map(|(x, y)| default.value(x, y))),
            [
                0x3f000000, 0x3eb041cb, 0x3f11b66e, 0x38324000, 0x3f75c326, 0x3e0e8ac4, 0x3d93b610,
                0x3f146e47
            ]
        );
        assert_eq!(
            bits(extreme.map(|(x, y)| default.value(x, y))),
            [
                0x3effcdc3, 0x3f000000, 0x3e16cb9c, 0x3effcdc3, 0x3f6baa7e, 0x3f000000, 0x3f000000,
                0x3edb688c
            ]
        );
    }

    #[test]
    fn line_source_matches_engine_0_3_0() {
        // Expected bits come from Engine 0.3.0 (`Line` and its static helpers); NaNs are compared as one value
        // because their sign and payload depend on the CPU.
        let radius = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 1501,
                octaves: 2,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
        .map(Module::constant(16.0), Module::constant(100.0));
        let c = Module::constant;
        let lines = [
            // Fades in and out, feathered.
            Line::new(-10.0, 5.0, 30.0, 25.0, radius, c(0.2), c(0.3), 0.4),
            // No feathering, no fades (zero and negative fade lengths are ignored).
            Line::new(-10.0, 5.0, 30.0, 25.0, c(50.0), c(0.0), c(-1.0), 0.0),
            // A zero-length line: every distance is 0/0.
            Line::new(3.0, 4.0, 3.0, 4.0, c(9.0), c(0.5), c(0.5), 0.25),
            // A NaN fade-in is ignored; a fade-out longer than the line fades it everywhere.
            Line::new(0.0, 0.0, 20.0, 0.0, c(30.0), c(f32::NAN), c(2.0), 1.0),
        ];
        let points = [
            (-10.0, 5.0),
            (30.0, 25.0),
            (10.0, 15.0),
            (10.0, 20.0),
            (-15.0, 2.0),
            (35.0, 28.0),
            (0.0, 0.0),
            (22.0, 14.0),
            (f32::NAN, 3.0),
            (f32::INFINITY, 0.0),
            (-0.0, -0.0),
            (1e20, -1e20),
        ];
        let expected: [[[u32; 6]; 12]; 4] = [
            [
                [0x7fc00000, 0x00000000, 0x3f570a3e, 0x3f570a3e, 0, 0],
                [0x7fc00000, 0x00000000, 0x3f570a3e, 0x3f570a3e, 0, 0],
                [0x3f6eeef0, 0x3f555555, 0x3f570a3e, 0x3f570a3e, 0, 0],
                [0x3f096fb8, 0x3f355555, 0x3ee6ee27, 0x3ef7cf76, 0, 0],
                [0x00000000, 0x3dae147b, 0x00000000, 0x3c34235f, 1, 0],
                [0x00000000, 0x3d681b4f, 0x00000000, 0x00000000, 0, 1],
                [0x00000000, 0x3ea00000, 0x00000000, 0x00000000, 0, 0],
                [0x00000000, 0x3e9dddde, 0x3d01b31a, 0x3dd4df5f, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 1, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
                [0x00000000, 0x3ea00000, 0x00000000, 0x00000000, 0, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
            ],
            [
                [0x7fc00000, 0x00000000, 0x3f800000, 0x3f800000, 0, 0],
                [0x7fc00000, 0x00000000, 0x3f800000, 0x3f800000, 0, 0],
                [0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0, 0],
                [0x3f19999a, 0x3f800000, 0x3eaaaaac, 0x3ed2d2d2, 0, 0],
                [0x3ea3d70a, 0x3f800000, 0x00000000, 0x00000000, 1, 0],
                [0x3ea3d70a, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 0],
                [0x3e5d2f18, 0x3f800000, 0x00000000, 0x00000000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 1, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
            ],
            [
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 1, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 1, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0, 0],
            ],
            [
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 1, 0],
                [0x00000000, 0x3f680000, 0x00000000, 0x00000000, 0, 1],
                [0x00000000, 0x3ed00000, 0x00000000, 0x00000000, 0, 0],
                [0x00000000, 0x3f200000, 0x00000000, 0x00000000, 0, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 1, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
                [0x7fc00000, 0x00000000, 0x3f19999a, 0x3f19999a, 0, 0],
                [0x00000000, 0x3e800000, 0x00000000, 0x00000000, 0, 1],
                [0x7fc00000, 0x3f800000, 0x7fc00000, 0x7fc00000, 1, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
                [0x7fc00000, 0x00000000, 0x3f19999a, 0x3f19999a, 0, 0],
                [0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0, 1],
            ],
        ];
        for (line, expected) in lines.iter().zip(expected) {
            let module = Module::line(line.clone());
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                [0, 0x3f800000]
            );
            let actual = points.map(|(x, y)| {
                let values = canonical_bits([
                    module.value(x, y),
                    line.width_modifier(x, y),
                    line.value_with_width(x, y, 0.6),
                    line.value_with_min_width(x, y, 4.0, 0.6),
                ]);
                [
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    u32::from(line.clip_start(x, y)),
                    u32::from(line.clip_end(x, y)),
                ]
            });
            assert_eq!(actual, expected);
        }

        assert_eq!(
            bits([
                Line::dist2(1.0, 2.0, 4.0, 6.0),
                Line::dist2(-0.0, 0.0, 0.0, -0.0)
            ]),
            [0x41c80000, 0x00000000]
        );
        assert_eq!(
            [
                Line::sign(2.0, 3.0, 0.0, 0.0, 4.0, 4.0),
                Line::sign(2.0, 2.0, 0.0, 0.0, 4.0, 4.0),
                Line::sign(3.0, 2.0, 0.0, 0.0, 4.0, 4.0),
                Line::sign(f32::NAN, 2.0, 0.0, 0.0, 4.0, 4.0),
            ],
            [-1, 0, 1, 1]
        );
        // Crossing; collinear disjoint, overlapping, touching at an end, before the start; parallel; a T
        // junction; all points equal; a NaN endpoint.
        let segments: [[f32; 8]; 9] = [
            [0.0, 0.0, 4.0, 4.0, 0.0, 4.0, 4.0, 0.0],
            [0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0],
            [0.0, 0.0, 2.0, 2.0, 1.0, 1.0, 3.0, 3.0],
            [0.0, 0.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0],
            [0.0, 0.0, 2.0, 2.0, -1.0, -1.0, -3.0, -3.0],
            [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 4.0, 0.0, 2.0, 0.0, 2.0, 5.0],
            [0.0; 8],
            [f32::NAN, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0],
        ];
        assert_eq!(
            segments.map(|[a, b, c, d, e, f, g, h]| Line::intersect(a, b, c, d, e, f, g, h)),
            [true, false, true, true, false, false, true, true, true]
        );
        // On the segment, past its end, before its start, a zero-length segment (0/0), a NaN point.
        let queries: [[f32; 6]; 6] = [
            [2.0, 3.0, 0.0, 0.0, 4.0, 4.0],
            [8.0, 9.0, 0.0, 0.0, 4.0, 4.0],
            [-3.0, -1.0, 0.0, 0.0, 4.0, 4.0],
            [5.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            [f32::NAN, 0.0, 0.0, 0.0, 4.0, 4.0],
            [1.0, 7.0, -2.0, 3.0, 6.0, -5.0],
        ];
        assert_eq!(
            queries.map(|[x, y, ax, ay, bx, by]| {
                let values = canonical_bits([
                    Line::distance_on_line(x, y, ax, ay, bx, by),
                    Line::distance2_line(x, y, ax, ay, bx, by),
                    Line::distance2_line_including_ends(x, y, ax, ay, bx, by),
                ]);
                [values[0], values[1], values[2]]
            }),
            [
                [0x3f200000, 0x3f000000, 0x3f000000],
                [0x40080000, 0xbf800000, 0x42240000],
                [0xbf000000, 0xbf800000, 0x41200000],
                [0x7fc00000, 0x7fc00000, 0x7fc00000],
                [0x7fc00000, 0x7fc00000, 0x7fc00000],
                [0xbd800000, 0xbf800000, 0x41c80000],
            ]
        );
    }

    /// Raw seed-first values at config seed `i32::MAX`, runtime seed 17 (the `v` lines of
    /// `.local/terraforged/oracles/seed_first/`).
    #[test]
    fn seed_offset_sources_match_current_noise2d() {
        let expected = [
            [0x3f000000, 0x3f000000, 0x3edf0d3a],
            [0x3f000000, 0x3f000000, 0x3f09e3f6],
            [0x3f000000, 0x3e309b54, 0x3ef4b0d1],
            [0x3f000000, 0x3db98455, 0x3ef1dd05],
            [0x3ed71df8, 0x3f16df8b, 0x3f090c39],
            [0x3f800000, 0x3f800000, 0x3f11065b],
            [0, 0, 0x3eddf34a],
            [0x3f800000, 0x3d9947bb, 0x3f0c6ac6],
        ];
        for (source_index, kind) in [
            SourceKind::Perlin,
            SourceKind::Perlin2,
            SourceKind::Simplex,
            SourceKind::Simplex2,
            SourceKind::Cubic,
            SourceKind::Ridge,
            SourceKind::Billow,
            SourceKind::SimplexRidge,
        ]
        .into_iter()
        .enumerate()
        {
            let source = Source::new(
                kind,
                SourceConfig {
                    seed: i32::MAX,
                    octaves: 2,
                    gain: 0.65,
                    frequency: 128.0,
                    ..SourceConfig::default()
                },
            );
            for (point_index, (x, y)) in [(0.0, 0.0), (0.125, -0.75), (12.34, 56.78)]
                .into_iter()
                .enumerate()
            {
                assert_eq!(
                    source.value_seed_first(17, x, y).to_bits(),
                    expected[source_index][point_index],
                    "source={source_index}, point={point_index}"
                );
            }
        }
    }

    /// Folds of every source kind over 7 configs (0 octaves, negative gain and lacunarity, an infinite
    /// range), 5 config seeds × 8 runtime seeds (both `int` extremes) × 10 points (NaN, infinity, -0).
    /// `SEED_FIRST` is the newer Noise2D (2cb4843) `getValue(seed, x, y)`; `JAR` is Engine 0.3.0's shaded
    /// Noise2D built with the seed `config + seed` (`config` for Perlin/Perlin2) and sampled seedless.
    /// They differ only where the newer `NoiseUtil.map` does (zero or infinite range). Oracle:
    /// `.local/terraforged/oracles/seed_first/` (outside Git).
    #[test]
    fn seed_first_sources_match_current_noise2d_and_jar_seeding() {
        const SEED_FIRST: [[u64; 7]; 8] = [
            [
                0x243943d826eb6b75,
                0x38d416a31c45df05,
                0x21701be7abbff025,
                0x3634690f94334265,
                0xdf984c0ff2b34265,
                0xc5c1b14154ced265,
                0xb2c55cfcf0334265,
            ],
            [
                0x3bdafa4b42e56ba5,
                0x718eced4b12bd305,
                0x1038b11cc89eb155,
                0x3634690f94334265,
                0xdf984c0ff2b34265,
                0x8dcd798b4055eab5,
                0x376a51b4bd334265,
            ],
            [
                0xf09bacdc83ab3058,
                0x0d068c6120bc3a97,
                0x3f4374db5f718a0a,
                0x28c7db6f01ff2dd0,
                0xdf984c0ff2b34265,
                0xae4c9e5aa60080c5,
                0x92490883ef334265,
            ],
            [
                0x25e887b22090cca7,
                0x37f16f226133bc71,
                0x08f2313a81e3270f,
                0xb2f41e40a5b1736c,
                0xdf984c0ff2b34265,
                0xd75b87b16179d253,
                0x92490883ef334265,
            ],
            [
                0xb375fb6b738ed66d,
                0xf8921a5f6a72be25,
                0x38ee61d8a4c0b296,
                0x304394e304b8dbc0,
                0xf324896d31927ee5,
                0x9efe69d68bba55fb,
                0x68ffad44fef34265,
            ],
            [
                0x7b9c5757f077b1dc,
                0x8ed93992156f3aea,
                0xe5939fe1f8a78f47,
                0xf3686f0d6ddc9153,
                0xdf984c0ff2b34265,
                0x3137a60464982347,
                0x930945f2a7d0cec5,
            ],
            [
                0xe2dbb6f99ab0e69c,
                0xe7205f33b7711e5f,
                0x39fa5e43c3ecd6f2,
                0x8beb6d33cee9bb1e,
                0x2c1b93daafb34265,
                0x1d9684974953b644,
                0x2020e11a2906abf0,
            ],
            [
                0x5cbecd9bfd83899c,
                0x9d727313621283c0,
                0x035fcecf821ad34d,
                0x422baf47f9cb8152,
                0xdf984c0ff2b34265,
                0x315a791a7adb2540,
                0x2bde73c0fcb34265,
            ],
        ];
        const JAR: [[u64; 7]; 8] = [
            [
                0x243943d826eb6b75,
                0x38d416a31c45df05,
                0x21701be7abbff025,
                0x3634690f94334265,
                0xb2c55cfcf0334265,
                0xc5c1b14154ced265,
                0xb2c55cfcf0334265,
            ],
            [
                0x3bdafa4b42e56ba5,
                0x718eced4b12bd305,
                0x1038b11cc89eb155,
                0x3634690f94334265,
                0xb2c55cfcf0334265,
                0x8dcd798b4055eab5,
                0xb2c55cfcf0334265,
            ],
            [
                0xf09bacdc83ab3058,
                0x0d068c6120bc3a97,
                0x3f4374db5f718a0a,
                0x28c7db6f01ff2dd0,
                0xb2c55cfcf0334265,
                0xae4c9e5aa60080c5,
                0xb2c55cfcf0334265,
            ],
            [
                0x25e887b22090cca7,
                0x37f16f226133bc71,
                0x08f2313a81e3270f,
                0xb2f41e40a5b1736c,
                0xb2c55cfcf0334265,
                0xd75b87b16179d253,
                0xb2c55cfcf0334265,
            ],
            [
                0xb375fb6b738ed66d,
                0xf8921a5f6a72be25,
                0x38ee61d8a4c0b296,
                0x304394e304b8dbc0,
                0xf324896d31927ee5,
                0x9efe69d68bba55fb,
                0xb2c55cfcf0334265,
            ],
            [
                0x7b9c5757f077b1dc,
                0x8ed93992156f3aea,
                0xe5939fe1f8a78f47,
                0xf3686f0d6ddc9153,
                0xb2c55cfcf0334265,
                0x3137a60464982347,
                0x930945f2a7d0cec5,
            ],
            [
                0xe2dbb6f99ab0e69c,
                0xe7205f33b7711e5f,
                0x39fa5e43c3ecd6f2,
                0x8beb6d33cee9bb1e,
                0xb2c55cfcf0334265,
                0x1d9684974953b644,
                0x2020e11a2906abf0,
            ],
            [
                0x5cbecd9bfd83899c,
                0x9d727313621283c0,
                0x035fcecf821ad34d,
                0x422baf47f9cb8152,
                0xb2c55cfcf0334265,
                0x315a791a7adb2540,
                0x2bde73c0fcb34265,
            ],
        ];
        let kinds = [
            SourceKind::Perlin,
            SourceKind::Perlin2,
            SourceKind::Simplex,
            SourceKind::Simplex2,
            SourceKind::Cubic,
            SourceKind::Ridge,
            SourceKind::Billow,
            SourceKind::SimplexRidge,
        ];
        let configs = [
            (1, 0.5, 1.0, 2.0, Interpolation::Curve3),
            (2, 0.65, 128.0, 2.0, Interpolation::Curve3),
            (5, 0.975, 1.0 / 128.0, 1.7, Interpolation::Curve4),
            (3, -0.3, 0.01, -1.5, Interpolation::Linear),
            (0, 0.5, 3.0, 2.0, Interpolation::Curve3),
            (4, 1.25, 0.37, 2.5, Interpolation::Linear),
            (3, 1e20, 1.0, 2.0, Interpolation::Curve3),
        ];
        let config_seeds = [1337, i32::MAX, i32::MIN, -5, 0];
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, 123_456_789, -987_654_321];
        let points = [
            (0.0, 0.0),
            (0.125, -0.75),
            (12.34, 56.78),
            (-1000.5, 2000.25),
            (1e6, -1e6),
            (-0.0, 3.5),
            (f32::NAN, 0.0),
            (f32::INFINITY, 1.0),
            (-7.9, -0.001),
            (65536.5, -32768.25),
        ];
        // Java's `Float.floatToIntBits`: one canonical NaN.
        let bits = |value: f32| {
            if value.is_nan() {
                0x7fc0_0000
            } else {
                u64::from(value.to_bits())
            }
        };
        let fold = |hash: u64, value: f32| (hash ^ bits(value)).wrapping_mul(0x0000_0100_0000_01b3);
        for (kind_index, kind) in kinds.into_iter().enumerate() {
            let ignores_seed = matches!(kind, SourceKind::Perlin | SourceKind::Perlin2);
            for (config_index, (octaves, gain, frequency, lacunarity, interpolation)) in
                configs.into_iter().enumerate()
            {
                let config = |seed| SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    lacunarity,
                    interpolation,
                };
                let (mut seed_first, mut jar) =
                    (0xcbf2_9ce4_8422_2325_u64, 0xcbf2_9ce4_8422_2325_u64);
                for config_seed in config_seeds {
                    let source = Source::new(kind, config(config_seed));
                    for seed in seeds {
                        let bound = if ignores_seed {
                            config_seed
                        } else {
                            config_seed.wrapping_add(seed)
                        };
                        let legacy = Source::new(kind, config(bound));
                        for (x, y) in points {
                            seed_first = fold(seed_first, source.value_seed_first(seed, x, y));
                            jar = fold(jar, legacy.value(x, y));
                        }
                    }
                }
                assert_eq!(
                    seed_first, SEED_FIRST[kind_index][config_index],
                    "seed-first kind={kind_index}, config={config_index}"
                );
                assert_eq!(
                    jar, JAR[kind_index][config_index],
                    "jar kind={kind_index}, config={config_index}"
                );
            }
        }
    }

    /// #109's `with_seed` (the jar's Cereal reseeding) adds the seed to a source's own, so where the maps
    /// agree it gives the seed-first bits, except for Perlin/Perlin2, whose seed-first form ignores the seed.
    #[test]
    fn seed_first_sources_relate_to_reseeding() {
        let config = SourceConfig {
            seed: i32::MAX,
            octaves: 3,
            gain: 0.65,
            frequency: 1.0 / 64.0,
            ..SourceConfig::default()
        };
        for kind in [
            SourceKind::Perlin,
            SourceKind::Perlin2,
            SourceKind::Simplex,
            SourceKind::Simplex2,
            SourceKind::Cubic,
            SourceKind::Ridge,
            SourceKind::Billow,
            SourceKind::SimplexRidge,
        ] {
            let source = Source::new(kind, config);
            for seed in [0, 17, -1, i32::MIN, i32::MAX] {
                let reseeded = Module::source(source).with_seed(i64::from(seed));
                for (x, y) in [(0.5, -3.25), (-1234.5, 87.125), (40_000.0, -0.75)] {
                    let expected = if matches!(kind, SourceKind::Perlin | SourceKind::Perlin2) {
                        source.value(x, y)
                    } else {
                        reseeded.value(x, y)
                    };
                    assert_eq!(
                        source.value_seed_first(seed, x, y).to_bits(),
                        expected.to_bits()
                    );
                }
            }
        }
    }

    #[test]
    fn ridged_and_random_sources_match_engine_0_3_0() {
        let config = SourceConfig {
            seed: -7391,
            octaves: 6,
            gain: 0.83,
            frequency: 0.00390625,
            lacunarity: 2.25,
            interpolation: Interpolation::Curve3,
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                SourceKind::Ridge,
                [0x3f800000, 0x3f4e3bad, 0x3f6e7958, 0x3f0119da],
                [0x3f800000, 0x3f58fe28, 0x3f784536, 0x3f51f9ce],
            ),
            (
                SourceKind::Billow,
                [0, 0x3e47114c, 0x3d8c3540, 0x3efdcc4c],
                [0, 0x3e1c0760, 0x3cf75940, 0x3e3818c8],
            ),
            (
                SourceKind::SimplexRidge,
                [0x3f25da6e, 0x3e8542ea, 0x3ddba237, 0x3e609c6f],
                [0x3f25da6e, 0x3e7e4e32, 0x3e116da0, 0x3df253c5],
            ),
        ];
        for (kind, expected, expected_seeded) in cases {
            let source = Source::new(kind, config);
            assert_eq!(bits(points.map(|(x, y)| source.value(x, y))), expected);
            assert_eq!(
                bits(points.map(|(x, y)| source.value_seeded(x, y, 991))),
                expected_seeded
            );
        }

        let rand = Rand::new(5519, 0.125);
        assert_eq!(
            bits(points.map(|(x, y)| rand.value(x, y))),
            [0x3f0322bd, 0x3f1d629d, 0x3ee51611, 0x3e86b835]
        );
        assert_eq!(
            bits(points.map(|(x, y)| rand.value_seeded(x, y, -17))),
            [0x3e4239b0, 0x3f64c89d, 0xbe335bca, 0x3f33d648]
        );
        assert_eq!(points.map(|(x, y)| rand.next_int(x, y, 19)), [0; 4]);
        assert_eq!(
            points.map(|(x, y)| rand.next_int_seeded(x, y, -17, 19)),
            [0; 4]
        );
    }

    #[test]
    fn ridged_bounds_and_edge_configs_match_engine_0_3_0() {
        // Negative gain, zero octaves (0/0 = NaN), a negative lacunarity (negative spectral weights), Java's maximum
        // of 30 octaves, and a negative bound (gain -2, lacunarity -0.5), where `map` needs the `Math.abs` range:
        // without it every value would be 1. Expected values come from Engine 0.3.0.
        let configs = [
            (2147483646, 9, -0.6, 0.031, 1.9, Interpolation::Curve4),
            (-7, 0, 0.5, 0.25, 2.0, Interpolation::Linear),
            (1337, 2, 0.5, 1.0, 2.0, Interpolation::Curve3),
            (99, 5, 0.9, 0.07, -1.7, Interpolation::Curve3),
            (-123456, 30, 0.975, 0.013, 1.3, Interpolation::Curve4),
            (55, 2, -2.0, 0.05, -0.5, Interpolation::Curve3),
        ];
        let kinds = [
            SourceKind::Ridge,
            SourceKind::Billow,
            SourceKind::SimplexRidge,
        ];
        let points = [(0.0, 0.0), (3.5, -17.25), (-1000.125, 250.75)];
        const NAN: u32 = 0x7fc0_0000;
        // Per config and kind: getValue(x, y) and getValue(x, y, Integer.MAX_VALUE). minValue/maxValue are 0/1.
        let expected: [[([u32; 3], [u32; 3]); 3]; 6] = [
            [
                (
                    [0x3f800000, 0x3f4d2122, 0x3f520b5f],
                    [0x3f800000, 0x3ecdaf4c, 0x3f37390b],
                ),
                ([0, 0x3e4b7b78, 0x3e37d284], [0, 0x3f19285a, 0x3e918dea]),
                (
                    [0x3f800000, 0x3d71889a, 0x3f800000],
                    [0x3f800000, 0x3e001264, 0x3f32c6be],
                ),
            ],
            [([NAN; 3], [NAN; 3]); 3],
            [
                (
                    [0x3f800000, 0x3ecf1800, 0x3ef1416b],
                    [0x3f800000, 0x3f7c0600, 0x3f3b1a38],
                ),
                ([0, 0x3f187400, 0x3f075f4a], [0, 0x3c7e8000, 0x3e89cb90]),
                (
                    [0x3f800000, 0x3f0ce83e, 0x3e06d15c],
                    [0x3f800000, 0x3f43ba6f, 0x3d1403b4],
                ),
            ],
            [
                (
                    [0x3f800000, 0x3f5c9df8, 0x3cc72c78],
                    [0x3f800000, 0x3ef517b8, 0x3f4bcfcd],
                ),
                ([0, 0x3e0d8820, 0x3f79c69c], [0, 0x3f057424, 0x3e50c0cc]),
                (
                    [0x3e701273, 0x3e1968a2, 0x3d8e7ff2],
                    [0x3e701273, 0x3bdaf8e8, 0x3d417929],
                ),
            ],
            [
                (
                    [0x3f800000, 0x3f257ac0, 0x3f106165],
                    [0x3f800000, 0x3ef0c36f, 0x3f089c10],
                ),
                ([0, 0x3eb50a80, 0x3edf3d36], [0, 0x3f079e48, 0x3eeec7e0]),
                (
                    [0x3e95b5b5, 0x3c9d537b, 0x3ce43e05],
                    [0x3e95b5b5, 0x3a6fe6b4, 0x3bce989d],
                ),
            ],
            [
                ([0; 3], [0; 3]),
                ([0x3f800000; 3], [0x3f800000; 3]),
                ([0; 3], [0, 0xbf800000, 0xbf800000]),
            ],
        ];
        for ((seed, octaves, gain, frequency, lacunarity, interpolation), rows) in
            configs.into_iter().zip(expected)
        {
            let config = SourceConfig {
                seed,
                octaves,
                gain,
                frequency,
                lacunarity,
                interpolation,
            };
            for (kind, (plain, seeded)) in kinds.into_iter().zip(rows) {
                let source = Source::new(kind, config);
                assert_eq!(source.min_value(), 0.0);
                assert_eq!(source.max_value(), 1.0);
                assert_eq!(
                    canonical_bits(points.map(|(x, y)| source.value(x, y))),
                    plain
                );
                assert_eq!(
                    canonical_bits(points.map(|(x, y)| source.value_seeded(x, y, i32::MAX))),
                    seeded
                );
            }
        }
    }

    #[test]
    fn ridged_sweep_matches_engine_0_3_0() {
        // FNV-1a style checksums of raw bits; Engine 0.3.0 runs the same loops.
        let mut acc: u32 = 0x811c_9dc5;
        let mut count = 0;
        for i in -400..=400 {
            let lacunarity = i as f32 * 0.0137 + 0.001;
            let mut frequency = 1.0_f32;
            for _ in 0..30 {
                acc = (acc ^ spectral_weight(frequency).to_bits()).wrapping_mul(0x0100_0193);
                frequency *= lacunarity;
                count += 1;
            }
        }
        assert_eq!((count, acc), (24030, 0x9153df22));

        let mut acc: u32 = 0x811c_9dc5;
        let mut mix = |v: f32| acc = (acc ^ v.to_bits()).wrapping_mul(0x0100_0193);
        let mut count = 0;
        for lacunarity in [1.5, 2.0, 2.25, 3.1, 0.7] {
            for gain in [0.5, 0.83, 1.1, -0.4] {
                let config = SourceConfig {
                    seed: 4242,
                    octaves: 7,
                    gain,
                    frequency: 0.011,
                    lacunarity,
                    interpolation: Interpolation::Curve3,
                };
                let ridge = Source::new(SourceKind::Ridge, config);
                let billow = Source::new(SourceKind::Billow, config);
                let simplex_ridge = Source::new(SourceKind::SimplexRidge, config);
                for iy in -10..10 {
                    for ix in -10..10 {
                        let x = ix as f32 * 37.3 - 5.1;
                        let y = iy as f32 * 29.7 + 1.3;
                        mix(ridge.value(x, y));
                        mix(billow.value_seeded(x, y, -5));
                        mix(simplex_ridge.value(x, y));
                        count += 1;
                    }
                }
            }
        }
        assert_eq!((count, acc), (8000, 0x7d958e37));
    }

    #[test]
    fn rand_next_int_matches_engine_0_3_0() {
        // `range * noise / (range + range)` rounds to 0 for small ranges; `Integer.MAX_VALUE + Integer.MAX_VALUE`
        // wraps to -2 in Java, so the largest range gives large negative values.
        let rand = Rand::new(5519, 0.125);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (-3.5, 2.5),
        ];
        for range in [0, 1, 19, 1000, -5] {
            assert_eq!(points.map(|(x, y)| rand.next_int(x, y, range)), [0; 5]);
            assert_eq!(
                points.map(|(x, y)| rand.next_int_seeded(x, y, -17, range)),
                [0; 5]
            );
        }
        assert_eq!(
            points.map(|(x, y)| rand.next_int(x, y, i32::MAX)),
            [-550022976, -660121408, -480428576, -282527392, -550022976]
        );
        assert_eq!(
            points.map(|(x, y)| rand.next_int_seeded(x, y, -17, i32::MAX)),
            [-203660032, -959588160, 188071072, -754291200, -894462208]
        );
        assert_eq!(
            bits(points.map(|(x, y)| rand.value_seeded(x, y, i32::MAX))),
            [0xbe1a0663, 0x3baa8000, 0xbceece10, 0x3f529ef7, 0xbf5f7d26]
        );
    }

    #[test]
    fn scalar_graph_runtime_seeds_match_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                frequency: 0.125,
                octaves: 2,
                gain: 0.65,
                ..SourceConfig::default()
            },
        ));
        let control = Module::source(Source::new(
            SourceKind::Cubic,
            SourceConfig {
                seed: -91,
                frequency: 0.25,
                octaves: 2,
                ..SourceConfig::default()
            },
        ));
        let perlin = Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed: 53,
                frequency: 0.5,
                octaves: 2,
                ..SourceConfig::default()
            },
        ));
        let graphs = [
            source.clone().bias(Module::constant(-0.5)).abs(),
            Module::modifier(ModuleKind::Alpha(source.clone(), control.clone()), &source),
            source.clone().bias(control.clone()),
            source.clone().clamp(
                control.clone().scale(Module::constant(0.25)),
                control.clone().bias(Module::constant(0.5)),
            ),
            source.clone().invert(),
            source
                .clone()
                .map(control.clone(), control.clone().bias(Module::constant(1.0))),
            source.clone().power(control.clone()),
            source.clone().scale(control.clone()),
            source.clone().threshold(control.clone()),
            Module::add(vec![source.clone(), control.clone(), perlin.clone()]),
            Module::multiply(vec![source.clone(), control.clone(), perlin.clone()]),
            Module::max(vec![source.clone(), control.clone(), perlin.clone()]),
            Module::min(vec![source.clone(), control.clone(), perlin.clone()]),
            Module::subtract(vec![source.clone(), control.clone(), perlin.clone()]),
            Module::modifier(ModuleKind::Alpha(source.clone(), control), &source)
                .bias(perlin)
                .scale(Module::constant(0.4)),
        ];
        let expected: [[u64; 4]; 15] = [
            [
                0x8b824554e95d207f,
                0xedc0a832360a66e3,
                0x8eb1146735546699,
                0x0fae72a3c3dcfcad,
            ],
            [
                0x59ee6d740a1343aa,
                0x48705ce73409335b,
                0xd1edde565fafa762,
                0x538889865e0b70e1,
            ],
            [
                0x3b92f1abc8e45f50,
                0x68764f9bb8fbb0df,
                0x623158244a3e2b8c,
                0x25d5ee315e2d28da,
            ],
            [
                0x50f2a377b15912ae,
                0x3bc130116da65f12,
                0xbdc6b0ff4ad9622c,
                0x450fd4be505a6f55,
            ],
            [
                0x1e239698f275a2ea,
                0xe0fee26e714bca65,
                0xd702afc3eae3fec4,
                0x2e1d94886897eb0b,
            ],
            [
                0x3b92f1abc8e45f50,
                0x68764f9bb8fbb0df,
                0x623158244a3e2b8c,
                0x25d5ee315e2d28da,
            ],
            [
                0xf4624c568d09236b,
                0x82b99bb85dd1a662,
                0xbf534a21076e4401,
                0xbdbd4dba26d45d8a,
            ],
            [
                0x17a555d585ff21a9,
                0x44d7c9e5521cb3c0,
                0x5dccc6b4e66049e4,
                0x74607cd50a9808d9,
            ],
            [
                0xcd257655fc4e13f5,
                0xcd257655fc4e13f5,
                0xcd257655fc4e13f5,
                0x1c4480da054e13f5,
            ],
            [
                0xaa39381cc76c5f1a,
                0x849145194b59731f,
                0xb855bcdfd94a5dbd,
                0x819a14a07b4a4655,
            ],
            [
                0x88d6e252bf908ec6,
                0xccddacd105664bd7,
                0xdf0831a9b00faf0b,
                0x8af4f81c18508596,
            ],
            [
                0xb8d75a41879ceaea,
                0xe41b2c22133a98c1,
                0x92db9db450d14888,
                0xc0e062df4d7f2146,
            ],
            [
                0x5c54076e34378b65,
                0x64650211b2b4bce8,
                0x5a8766b7fafd681b,
                0x51b92185d19e4202,
            ],
            [
                0x251e33703dd6424b,
                0x3786c672f07bcc40,
                0x74c5cc64837b82f0,
                0xb2d9ad39da2786e3,
            ],
            [
                0xbb94bb9ca033522e,
                0x513402c20cb58f6e,
                0x894487b8aba56da1,
                0xfe50e3ae1f83f07c,
            ],
        ];
        for (graph_index, (graph, hashes)) in graphs.iter().zip(expected).enumerate() {
            for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip(hashes) {
                let mut hash = 0xcbf29ce484222325u64;
                for (x, y) in [
                    (0.0, 0.0),
                    (0.125, -0.75),
                    (12.34, 56.78),
                    (-1234.5, 987.25),
                ] {
                    hash ^= u64::from(graph.try_value_seed_first(seed, x, y).unwrap().to_bits());
                    hash = hash.wrapping_mul(0x100000001b3);
                }
                assert_eq!(hash, expected, "graph {graph_index}, seed {seed}");
            }
        }
    }

    #[test]
    fn seeded_scalar_graph_rejects_unsupported_children() {
        let unsupported = unsupported_seeded_node(0.5);
        assert!(unsupported.try_value_seed_first(1, 0.0, 0.0).is_err());
        assert!(
            Module::constant(0.25)
                .bias(unsupported.clone())
                .try_value_seed_first(1, 0.0, 0.0)
                .is_err()
        );
        assert!(
            Module::add(vec![Module::constant(0.0), unsupported])
                .try_value_seed_first(1, 0.0, 0.0)
                .is_err()
        );
        for empty in [
            Module::add(vec![]),
            Module::multiply(vec![]),
            Module::min(vec![]),
            Module::max(vec![]),
            Module::subtract(vec![]),
        ] {
            assert_eq!(empty.try_value_seed_first(1, 0.0, 0.0), Ok(0.0));
        }
    }

    /// Noise2D 2cb4843 over edge cases: zero-range sources (0 octaves, where seed-first sampling must use
    /// `map_current`), an infinite range, Invert's out-of-bound branches, a zero-range Map, crossed Clamp bounds,
    /// Power's special cases, a NaN threshold, empty and single-source combiners, deep nesting, both `int` extreme
    /// seeds and NaN/infinite/-0 points. Hashes from `SeededGraphEdgeOracle.java` (`.local/terraforged/oracles/
    /// seed_first/`), one per graph over the seeds, then the points, with canonical NaN bits.
    #[test]
    fn scalar_graph_runtime_seed_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2, 0.65, 0.125);
        let control = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let perlin2 = src(SourceKind::Perlin2, -7, 3, 0.5, 0.3);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let zero_ridge = src(SourceKind::Ridge, 5, 0, 0.5, 1.0);
        let huge = src(SourceKind::Billow, -5, 3, 1e20, 1.0);
        let c = Module::constant;
        let threshold = source.clone().scale(c(0.25)).threshold(control.clone());
        let alpha = source.clone().bias(perlin.clone());
        let graphs = [
            Module::add(vec![zero.clone(), source.clone()]),
            zero_ridge.clone().bias(perlin2.clone()),
            Module::multiply(vec![huge.clone(), control.clone()]),
            threshold.clone().invert(),
            Module::subtract(vec![c(0.25), threshold]).invert(),
            control.clone().invert().invert(),
            c(0.3).map(source.clone(), control.clone()),
            zero.clone().map(control.clone(), source.clone()),
            huge.clone().map(c(-1.0), c(2.0)),
            source.clone().clamp(c(0.5), c(-0.5)),
            source.clone().abs().power(c(0.5)),
            source.clone().power(control.clone().scale(c(40.0))),
            c(-1.0).power(huge.clone()),
            source.clone().threshold(c(f32::NAN)),
            Module::add(vec![]),
            Module::multiply(vec![]),
            Module::max(vec![]),
            Module::min(vec![]),
            Module::subtract(vec![]),
            Module::subtract(vec![source.clone()]),
            Module::max(vec![perlin2.clone()]),
            Module::max(vec![
                Module::min(vec![
                    Module::add(vec![source.clone(), perlin2.clone()]),
                    Module::multiply(vec![control.clone(), zero.clone()]),
                ]),
                Module::subtract(vec![huge, perlin]),
                zero_ridge,
            ]),
            // `Module.alpha` would skip this control (its bounds leave 0..1); the oracle uses `new Alpha`.
            Module::modifier(ModuleKind::Alpha(alpha.clone(), control.clone()), &alpha)
                .clamp(control, perlin2.abs())
                .scale(zero.invert()),
        ];
        let expected: [u64; 23] = [
            0x4fb359ebc491fb4e,
            0x57515118049449ab,
            0x932b91965afb271f,
            0x9452c0977ede72c7,
            0x6981bfd7e75e72c7,
            0x2f1dd1f8753a883c,
            0xf12b5210ed1e72c7,
            0x2d31f1a1d5d0294c,
            0x571ebe50c7bf9b08,
            0x5edb6107d11e72c7,
            0x5e92c4df24a1e42e,
            0x9c4324424c566c0e,
            0x7d16428f3a5e72c7,
            0xd421388f395e72c7,
            0x4e2fd09b7fde72c7,
            0x4e2fd09b7fde72c7,
            0x4e2fd09b7fde72c7,
            0x4e2fd09b7fde72c7,
            0x4e2fd09b7fde72c7,
            0xd616aa7e1b6abd88,
            0x3cfb29857940568a,
            0x0d5f106d169e72c7,
            0x3f65b81f249e72c7,
        ];
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, -987654321];
        let points = [
            (0.0, 0.0),
            (0.125, -0.75),
            (12.34, 56.78),
            (-1234.5, 987.25),
            (-0.0, 3.5),
            (f32::NAN, 1.0),
            (f32::INFINITY, 2.0),
            (1e30, -1e30),
            (65536.5, -32768.25),
        ];
        for (graph_index, (graph, expected)) in graphs.iter().zip(expected).enumerate() {
            let values = seeds.into_iter().flat_map(|seed| {
                points.map(|(x, y)| graph.try_value_seed_first(seed, x, y).unwrap())
            });
            let hash = canonical_bits(values)
                .into_iter()
                .fold(0xcbf29ce484222325u64, |hash, bits| {
                    (hash ^ u64::from(bits)).wrapping_mul(0x100000001b3)
                });
            assert_eq!(hash, expected, "graph {graph_index}");
        }
    }

    /// Without Perlin sources (which ignore the runtime seed), a `Bias` (whose bias reseeding drops), a combiner
    /// (whose sources reseeding leaves unseeded) or a zero or infinite source range (where the jar's `map` and
    /// `map_current` differ), seed-first sampling with seed `s` gives the bits of the jar path reseeded with `s`
    /// (`with_seed`, #109): both add `s` to every source's seed.
    #[test]
    fn scalar_graph_runtime_seeds_match_reseeded_jar_graphs() {
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let control = src(SourceKind::Cubic, -91, 3);
        let ridge = src(SourceKind::Ridge, i32::MIN, 2);
        let c = Module::constant;
        let graphs = [
            source.clone().abs().power(ridge.clone()),
            ridge.clone().alpha(ridge.clone().abs()),
            source.clone().clamp(control.clone(), ridge.clone()),
            control.clone().invert(),
            source.clone().map(c(-2.0), control.clone()),
            source
                .clone()
                .scale(control.clone())
                .threshold(ridge.clone()),
        ];
        for (graph_index, graph) in graphs.iter().enumerate() {
            for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                let reseeded = graph.with_seed(i64::from(seed));
                for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, f32::NAN), (1e30, 3.0)] {
                    assert_eq!(
                        canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                        canonical_bits([reseeded.value(x, y)]),
                        "graph {graph_index}, seed {seed}, ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn runtime_seeded_domains_match_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                frequency: 0.125,
                octaves: 2,
                gain: 0.65,
                ..SourceConfig::default()
            },
        ));
        let control = Module::source(Source::new(
            SourceKind::Cubic,
            SourceConfig {
                seed: -91,
                frequency: 0.25,
                octaves: 2,
                ..SourceConfig::default()
            },
        ));
        let strength = source
            .clone()
            .scale(Module::constant(3.0))
            .bias(Module::constant(0.25));
        let axes = Domain::domain_warp(source.clone(), control, strength.clone());
        let direction = Domain::direction(source.clone(), strength);
        let domains = [
            Domain::direct(),
            axes.clone(),
            direction.clone(),
            axes.clone().add(direction.clone()),
            axes.clone().compound(direction.clone()),
            axes.clone().then(direction.clone()),
            axes.clone()
                .then(direction.clone())
                .compound(axes.add(direction)),
        ];
        let expected: [[u64; 4]; 7] = [
            [
                0x3f5d83450043d7aa,
                0x44dad7a858cffb76,
                0xa8a35e02a9c30670,
                0x75ca35c0637684a9,
            ],
            [
                0x71b13ef3a4d059b7,
                0xc3966dec6f9a6ac3,
                0x71a518cb6a4ddaae,
                0x8afa5878c019f5b4,
            ],
            [
                0x5bde5c5c3ed91f59,
                0x253a26e150042d96,
                0x95260d0fe0f62f9b,
                0x261be60bed0c9c85,
            ],
            [
                0x9f922f76221cb3d8,
                0x530f5bcd818c7c73,
                0x79175f5e01a546fa,
                0x8010fff83d7a99b4,
            ],
            [
                0x108203f02897bad6,
                0x368f068e13e5b1ac,
                0x4f6d78468e2a6101,
                0xcaa4f70304ae1b28,
            ],
            [
                0x8c60c24a7a8fb0f5,
                0x5a3183af79028f6f,
                0x4063e0a2bef5d53b,
                0x25d2a5b3da5e1334,
            ],
            [
                0xab5bca91abeb98b8,
                0xa1820ccb2733b19a,
                0xdb674a00985b6fb1,
                0x540db0a18ded0fde,
            ],
        ];
        for (index, (domain, hashes)) in domains.iter().zip(expected).enumerate() {
            let warped = source.clone().warp(domain.clone());
            for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip(hashes) {
                let mut hash = 0xcbf29ce484222325u64;
                for (x, y) in [
                    (0.0, 0.0),
                    (0.125, -0.75),
                    (12.34, 56.78),
                    (-1234.5, 987.25),
                ] {
                    for value in [
                        domain.try_offset_x_seed_first(seed, x, y).unwrap(),
                        domain.try_offset_y_seed_first(seed, x, y).unwrap(),
                        domain.try_x_seed_first(seed, x, y).unwrap(),
                        domain.try_y_seed_first(seed, x, y).unwrap(),
                        warped.try_value_seed_first(seed, x, y).unwrap(),
                    ] {
                        hash ^= u64::from(value.to_bits());
                        hash = hash.wrapping_mul(0x100000001b3);
                    }
                }
                assert_eq!(hash, expected, "domain {index}, seed {seed}");
            }
        }
    }

    /// Seed-first domain warps at the edges: Perlin axes and angles (which ignore the runtime seed), zero- and
    /// infinite-range axes (`DomainWarp`'s map), an axis already at -0.5..0.5 (its shortcut) and a constant one, NaN
    /// and huge angles (the sine table's `int` cast), NaN/infinite strengths, `Direct` inside combinations, a six-deep
    /// Add/Compound/Cumulative chain and a warp of a warp, at both `int` extreme seeds and NaN/infinite/-0 points.
    /// Hashes from `SeededDomainEdgeOracle.java` (`.local/terraforged/oracles/seed_first/run-graphs.sh`), one per
    /// domain over the seeds, the points, then seven outputs, with canonical NaN bits.
    #[test]
    fn runtime_seeded_domain_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2, 0.65, 0.125);
        let control = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let perlin2 = src(SourceKind::Perlin2, -7, 3, 0.5, 0.3);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let huge = src(SourceKind::Billow, -5, 3, 1e20, 1.0);
        let c = Module::constant;
        let strength = source.clone().scale(c(3.0)).bias(c(0.25));
        let dw0 = Domain::domain_warp(source.clone(), control.clone(), c(4.0));
        let dw1 = Domain::domain_warp(perlin, perlin2.clone(), strength);
        let dw2 = Domain::domain_warp(zero.clone(), huge.clone(), c(1.0));
        let dw3 = Domain::domain_warp(source.clone().map(c(-0.5), c(0.5)), c(0.5), control.clone());
        let dir4 = Domain::direction(source.clone(), c(2.0));
        let dir5 = Domain::direction(perlin2.clone(), huge);
        let dir6 = Domain::direction(c(f32::NAN), c(1.0));
        let dir7 = Domain::direction(source.clone().scale(c(1e9)), c(1.0));
        let dir8 = Domain::direction(zero, control.clone());
        let mut chain = dw0.clone();
        for i in 0..6 {
            chain = match i % 3 {
                0 => chain.add(dir4.clone()),
                1 => dir5.clone().compound(chain),
                _ => chain.then(dw1.clone()),
            };
        }
        let domains = [
            dw0.clone(),
            dw1.clone(),
            dw2.clone(),
            dw3.clone(),
            dir4.clone(),
            dir5.clone(),
            dir6.clone(),
            dir7.clone(),
            dir8.clone(),
            dw0.clone().add(dir4.clone()),
            dw1.clone().compound(dir5),
            dir4.clone().then(dw0.clone()),
            dw0.clone()
                .then(dir4)
                .then(dw1.add(dir8).compound(Domain::direct())),
            Domain::direct().compound(Domain::direct().then(Domain::direct())),
            dw0.then(dw3).compound(dir7).add(dir6.then(dw2)),
            chain,
        ];
        let expected: [u64; 16] = [
            0xb1f591fab036a4f9,
            0xe973aa67aa70dadd,
            0x8490a9ce079370c4,
            0x0fc359b953666019,
            0x5adcf430011e4d9e,
            0x5fd1f0562f99a42e,
            0xed8db6e8b8335a24,
            0x3ae6e06051aa2af3,
            0xde0cdecc339ae81d,
            0x42c552fb3baa3f6c,
            0x5afd9dc6c54d164a,
            0x98e741c422d3beb3,
            0x54080d2afd11bd66,
            0x9add91001765fda5,
            0x5e153fb2d03d4478,
            0x95ba6c715e065992,
        ];
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, -987654321];
        let points = [
            (0.0, 0.0),
            (0.125, -0.75),
            (12.34, 56.78),
            (-1234.5, 987.25),
            (-0.0, 3.5),
            (f32::NAN, 1.0),
            (f32::INFINITY, 2.0),
            (1e30, -1e30),
            (65536.5, -32768.25),
        ];
        for (index, (domain, expected)) in domains.iter().zip(expected).enumerate() {
            let warped = [
                source.clone().warp(domain.clone()),
                perlin2.clone().warp(domain.clone()),
                control.clone().warp(domain.clone()).warp(domain.clone()),
            ];
            let mut values = Vec::new();
            for seed in seeds {
                for (x, y) in points {
                    values.extend([
                        domain.try_offset_x_seed_first(seed, x, y).unwrap(),
                        domain.try_offset_y_seed_first(seed, x, y).unwrap(),
                        domain.try_x_seed_first(seed, x, y).unwrap(),
                        domain.try_y_seed_first(seed, x, y).unwrap(),
                    ]);
                    values.extend(
                        warped
                            .iter()
                            .map(|m| m.try_value_seed_first(seed, x, y).unwrap()),
                    );
                }
            }
            let hash = canonical_bits(values)
                .into_iter()
                .fold(0xcbf29ce484222325u64, |hash, bits| {
                    (hash ^ u64::from(bits)).wrapping_mul(0x100000001b3)
                });
            assert_eq!(hash, expected, "domain {index}");
        }
    }

    /// The river network (#125) and `ContinentNoise` (#128) inline their `Domain.warp(x, y, constant)` as
    /// `x + (-0.5 + axis) * strength`; that is the generic seed-first `DomainWarp` bit for bit (its `Map` of a 0..1
    /// axis is `-0.5 + value`), so either can move onto [`Domain`] without changing output.
    #[test]
    fn seeded_domain_warp_matches_inlined_river_and_continent_warps() {
        let simplex = |seed| {
            Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    seed,
                    frequency: 30.0,
                    ..SourceConfig::default()
                },
            )
        };
        let perlin2 = |seed| {
            Source::new(
                SourceKind::Perlin2,
                SourceConfig {
                    seed,
                    octaves: 3,
                    lacunarity: 2.2,
                    frequency: 3.0,
                    gain: 0.3,
                    ..SourceConfig::default()
                },
            )
        };
        let cases = [
            (simplex(8_657_124), simplex(5_123_678), 0.004),
            (perlin2(-1_234_567), perlin2(i32::MAX), 0.2),
        ];
        for (warp_x, warp_y, strength) in cases {
            let domain = Domain::domain_warp(
                Module::source(warp_x.clone()),
                Module::source(warp_y.clone()),
                Module::constant(strength),
            );
            for seed in [0, 17, -91, i32::MIN, i32::MAX] {
                for i in -20..20 {
                    let (x, y) = (i as f32 * 0.173 - 0.031, i as f32 * -2.71 + 1e4);
                    let inlined_x = x + (-0.5 + warp_x.value_seed_first(seed, x, y)) * strength;
                    let inlined_y = y + (-0.5 + warp_y.value_seed_first(seed, x, y)) * strength;
                    assert_eq!(
                        domain.try_x_seed_first(seed, x, y).unwrap().to_bits(),
                        inlined_x.to_bits()
                    );
                    assert_eq!(
                        domain.try_y_seed_first(seed, x, y).unwrap().to_bits(),
                        inlined_y.to_bits()
                    );
                }
            }
        }
    }

    #[test]
    fn seeded_domains_propagate_unsupported_children() {
        let unsupported = unsupported_seeded_node(0.5);
        let axes = Domain::domain_warp(
            unsupported.clone(),
            unsupported.clone(),
            Module::constant(1.0),
        );
        let strength = Domain::direction(Module::constant(0.25), unsupported.clone());
        for domain in [
            axes.clone(),
            strength,
            Domain::direct().add(axes.clone()),
            axes.clone().compound(Domain::direct()),
            Domain::direct().then(axes),
        ] {
            assert!(domain.try_x_seed_first(17, 1.0, 2.0).is_err());
            assert!(domain.try_y_seed_first(17, 1.0, 2.0).is_err());
            assert!(
                Module::constant(1.0)
                    .warp(domain)
                    .try_value_seed_first(17, 1.0, 2.0)
                    .is_err()
            );
        }
        assert!(
            unsupported
                .warp(Domain::direct())
                .try_value_seed_first(17, 1.0, 2.0)
                .is_err()
        );
    }

    #[test]
    fn runtime_seeded_blends_match_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                frequency: 0.125,
                octaves: 2,
                gain: 0.65,
                ..SourceConfig::default()
            },
        ));
        let control = Module::source(Source::new(
            SourceKind::Simplex2,
            SourceConfig {
                seed: -91,
                frequency: 0.25,
                octaves: 2,
                ..SourceConfig::default()
            },
        ));
        let lower = control.clone().scale(Module::constant(0.35));
        let upper = source.clone().bias(Module::constant(0.2));
        let expected: [[u64; 4]; 9] = [
            [
                0x48f9d5b731244d61,
                0x361b7c8d328d38fe,
                0x8ea734d33951a530,
                0xaba4a4efe51114c5,
            ],
            [
                0x99eadc2fef48965b,
                0x99c95bd03389d9aa,
                0x8cb265962e132f26,
                0xd34e833ba0156b93,
            ],
            [
                0xa7500c30060b86eb,
                0x34fab80cbc41c5a1,
                0x3001e73b44db7160,
                0x99e60b7e0f6d16a2,
            ],
            [
                0xe15c51ffe70e3059,
                0x3ce2c78d3e14c098,
                0x8b95d4cfa5ea52cf,
                0x6121c3262f3756cd,
            ],
            [
                0x74bd362fb01c0549,
                0x80f83e2fc12b2f0e,
                0x67b59dce2c33c676,
                0x1b8fdc9ccf859a66,
            ],
            [
                0x9d40d22ff4f3a75d,
                0xad8df0623f3aaf8b,
                0x060aeb0ba438f9ca,
                0x3155165855450bc9,
            ],
            [
                0xe0e8c81ef2ae01dc,
                0x56e46b8d502c39aa,
                0xdb0bfac0b97ee593,
                0x5f613aa1ce090b4f,
            ],
            [
                0x6960832f9ccd8120,
                0xf138fbd262ea42c9,
                0xfb4b706caec1186d,
                0xa964187467cdc4d1,
            ],
            [
                0x8d91022fda4be8ed,
                0xf9cc82ff792b4787,
                0x4b2e36eaf1509300,
                0xa5822effa410c53f,
            ],
        ];
        for (interpolation_index, interpolation) in [
            Interpolation::Linear,
            Interpolation::Curve3,
            Interpolation::Curve4,
        ]
        .into_iter()
        .enumerate()
        {
            let graphs = [
                lower.clone().base(source.clone(), 0.3, interpolation),
                control
                    .clone()
                    .blend(lower.clone(), upper.clone(), 0.5, 0.4, interpolation),
                control.clone().variable_blend(
                    source.clone(),
                    lower.clone(),
                    upper.clone(),
                    0.5,
                    0.05,
                    0.3,
                    interpolation,
                ),
            ];
            for (index, graph) in graphs.iter().enumerate() {
                for (seed, expected) in [0, 17, -91, i32::MAX]
                    .into_iter()
                    .zip(expected[interpolation_index * 3 + index])
                {
                    let mut hash = 0xcbf29ce484222325u64;
                    for (x, y) in [
                        (0.0, 0.0),
                        (0.125, -0.75),
                        (12.34, 56.78),
                        (-1234.5, 987.25),
                    ] {
                        hash ^=
                            u64::from(graph.try_value_seed_first(seed, x, y).unwrap().to_bits());
                        hash = hash.wrapping_mul(0x100000001b3);
                    }
                    assert_eq!(
                        hash, expected,
                        "interpolation {interpolation_index}, graph {index}, seed {seed}"
                    );
                }
            }
        }
        let fixed = source.clone().blend(
            lower.clone(),
            upper.clone(),
            0.5,
            0.0,
            Interpolation::Linear,
        );
        let variable = source.variable_blend(
            Module::constant(0.0),
            lower,
            upper,
            0.5,
            0.0,
            0.0,
            Interpolation::Linear,
        );
        for graph in [fixed, variable] {
            assert!(graph.try_value_seed_first(17, 0.0, 0.0).unwrap().is_nan());
        }
    }

    #[test]
    fn seeded_blends_only_evaluate_selected_children() {
        let unsupported = unsupported_seeded_node(0.25);
        let valid = Module::constant(0.75);
        let fixed = |control, lower: Module, upper: Module| {
            let kind = ModuleKind::Blend {
                control: Module::constant(control),
                lower,
                upper,
                midpoint: 0.5,
                blend: 0.5,
                blend_lower: 0.25,
                blend_upper: 0.75,
                blend_range: 0.5,
                interpolation: Interpolation::Linear,
            };
            Module::node(kind, 0.0, 1.0)
        };
        assert_eq!(
            fixed(0.0, valid.clone(), unsupported.clone()).try_value_seed_first(1, 0.0, 0.0),
            Ok(0.75)
        );
        assert_eq!(
            fixed(1.0, unsupported.clone(), valid.clone()).try_value_seed_first(1, 0.0, 0.0),
            Ok(0.75)
        );
        for boundary in [0.25, 0.5, 0.75] {
            assert!(
                fixed(boundary, valid.clone(), unsupported.clone())
                    .try_value_seed_first(1, 0.0, 0.0)
                    .is_err()
            );
        }
        for (control, lower, upper) in [
            (0.0, valid.clone(), unsupported.clone()),
            (1.0, unsupported.clone(), valid.clone()),
        ] {
            let graph = Module::constant(control).variable_blend(
                Module::constant(1.0),
                lower,
                upper,
                0.5,
                0.0,
                0.25,
                Interpolation::Linear,
            );
            assert_eq!(graph.try_value_seed_first(1, 0.0, 0.0), Ok(0.75));
        }
        assert_eq!(
            unsupported
                .clone()
                .base(valid.clone(), 0.0, Interpolation::Linear)
                .try_value_seed_first(1, 0.0, 0.0),
            Ok(0.75)
        );
        assert!(
            unsupported
                .base(Module::constant(0.0), 0.0, Interpolation::Linear)
                .try_value_seed_first(1, 0.0, 0.0)
                .is_err()
        );
    }

    /// `SeededBlendEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): Base/Blend/VariableBlend with a
    /// control fixed at, and one ulp beside, each window edge (plus NaN, ±inf, ±0), zero/negative/NaN/infinite
    /// windows and falloffs, NaN/±inf/zero/seeded variators, windows clamped to 0..1 and nested selectors.
    #[test]
    fn runtime_seeded_blend_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2, 0.65, 0.125);
        let control = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let variator = src(SourceKind::Simplex2, -7, 2, 0.5, 0.3);
        let c = Module::constant;
        let lo = control.clone().scale(c(0.35));
        let up = source.clone().bias(c(0.2));
        let base = source.clone().scale(c(0.35));
        let (l, c3, c4) = (
            Interpolation::Linear,
            Interpolation::Curve3,
            Interpolation::Curve4,
        );
        let (bm, vr) = (base.max_value(), 0.05 + 0.5 * 0.3);
        let vw = vec![java_max(0.0, 0.5 - vr), java_min(1.0, 0.5 + vr)];
        // Blend's constructor window for a 0..1 control.
        let win = |mid: f32, range: f32| {
            vec![
                java_max(0.0, mid - range / 2.0),
                java_min(1.0, mid + range / 2.0),
            ]
        };
        let blend = |mp, br, i| {
            let (lo, up) = (lo.clone(), up.clone());
            Box::new(move |k: Module| k.blend(lo.clone(), up.clone(), mp, br, i))
                as Box<dyn Fn(Module) -> Module>
        };
        let based = |falloff, i| {
            let base = base.clone();
            Box::new(move |k: Module| base.clone().base(k, falloff, i))
                as Box<dyn Fn(Module) -> Module>
        };
        let vary = |v: Module, mp, min, max, i| {
            let (lo, up) = (lo.clone(), up.clone());
            Box::new(move |k: Module| {
                k.variable_blend(v.clone(), lo.clone(), up.clone(), mp, min, max, i)
            }) as Box<dyn Fn(Module) -> Module>
        };
        let (nested_lo, nested_up) = (
            lo.clone().base(up.clone(), 0.3, c3),
            control.clone().variable_blend(
                perlin.clone(),
                up.clone(),
                lo.clone(),
                0.5,
                0.1,
                0.2,
                l,
            ),
        );
        let (p, z, u, cl, low) = (
            perlin.clone(),
            zero,
            up.clone(),
            control.clone(),
            lo.clone(),
        );
        let families: Vec<(Box<dyn Fn(Module) -> Module>, Vec<f32>)> = vec![
            (blend(0.5, 0.4, l), win(0.5, 0.4)),
            (blend(0.5, 0.4, c3), win(0.5, 0.4)),
            (blend(0.5, 0.4, c4), win(0.5, 0.4)),
            (blend(0.5, 0.0, l), win(0.5, 0.0)),
            (blend(0.5, -0.4, c3), win(0.5, -0.4)),
            (blend(f32::NAN, 0.4, l), vec![]),
            (blend(0.25, f32::INFINITY, c4), win(0.25, f32::INFINITY)),
            (blend(1.2, 0.4, l), win(1.2, 0.4)),
            (based(0.3, l), vec![bm, bm + 0.3]),
            (based(0.3, c3), vec![bm, bm + 0.3]),
            (based(0.3, c4), vec![bm, bm + 0.3]),
            (based(0.0, l), vec![bm]),
            (based(-0.2, l), vec![bm, bm - 0.2]),
            (based(f32::NAN, c3), vec![bm]),
            (based(f32::INFINITY, l), vec![bm]),
            (vary(c(0.5), 0.5, 0.05, 0.3, l), vw.clone()),
            (vary(c(0.5), 0.5, 0.05, 0.3, c3), vw.clone()),
            (vary(c(0.5), 0.5, 0.05, 0.3, c4), vw),
            (vary(c(f32::NAN), 0.5, 0.05, 0.3, l), vec![0.5]),
            (vary(c(f32::INFINITY), 0.5, 0.05, 0.3, c3), vec![0.0, 1.0]),
            (vary(c(f32::NEG_INFINITY), 0.5, 0.05, 0.3, l), vec![]),
            (vary(c(0.0), 0.5, 0.0, 0.3, l), vec![0.5]),
            (vary(variator, 0.4, 0.1, 0.5, c4), vec![0.4]),
            (vary(c(1.0), 0.5, 0.3, 0.4, l), vec![0.0, 1.0]),
            (
                Box::new(move |k: Module| {
                    k.blend(nested_lo.clone(), nested_up.clone(), 0.5, 0.4, c4)
                }),
                win(0.5, 0.4),
            ),
            (
                Box::new(move |k: Module| {
                    let inner = k.clone().blend(p.clone(), z.clone(), 0.5, 0.2, l);
                    cl.clone()
                        .blend(inner, u.clone(), 0.3, 0.2, c3)
                        .scale(k.base(low.clone(), 0.25, l))
                }),
                win(0.5, 0.2),
            ),
            (
                Box::new(move |k: Module| control.clone().blend(k, up.clone(), 0.25, 0.1, c3)),
                vec![],
            ),
        ];
        let expected: [u64; 27] = [
            0x6f18d3dd954af663,
            0xc3e0c297bd14859f,
            0x6aaaf4e5a7744318,
            0x69b3670b0e05a6d7,
            0x784308d585ff0905,
            0xc7890e94de74f24d,
            0xa1ba46c5c7aa69d4,
            0x2418cadf2fb982c7,
            0x3a3300612e5d67c4,
            0xe2dacb683d47bd83,
            0xd179158eb6aee6a2,
            0x56ad70171f1af2dd,
            0xf2f694f0a3b4334b,
            0xba6ae5f751e2df31,
            0x55393397180ac885,
            0x6f18d3dd954af663,
            0xc3e0c297bd14859f,
            0x6aaaf4e5a7744318,
            0x463f236bcb8ac885,
            0xa37b4b140fe6a74b,
            0xe3e76edba7593bed,
            0x22a0b05c56ceff1f,
            0x7f75e55ba1a579b1,
            0x0a2c791c02420a18,
            0x0eb4ca970fbd4ab0,
            0xf81b6449eef60226,
            0x16f8303fa64ab20e,
        ];
        let specials = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            0.5,
            1.0,
            0.3,
            0.7,
        ];
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, -987654321];
        let points = [
            (0.0, 0.0),
            (12.34, 56.78),
            (-1234.5, 987.25),
            (-0.0, 3.5),
            (f32::NAN, 1.0),
            (f32::INFINITY, 2.0),
        ];
        for (index, ((family, edges), expected)) in families.iter().zip(expected).enumerate() {
            let controls = edges
                .iter()
                .flat_map(|&e| [e, e.next_down(), e.next_up()])
                .chain(specials);
            let mut hash = 0xcbf29ce484222325u64;
            for v in controls {
                // A Java module overriding only `getValue`: the value `v` with `Noise`'s default 0..1 bounds.
                let graph = family(Module::node(ModuleKind::Constant(v), 0.0, 1.0));
                for seed in seeds {
                    for (x, y) in points {
                        let value = graph.try_value_seed_first(seed, x, y).unwrap();
                        let bits = if value.is_nan() {
                            0x7fc0_0000
                        } else {
                            value.to_bits()
                        };
                        hash = (hash ^ u64::from(bits)).wrapping_mul(0x100000001b3);
                    }
                }
            }
            assert_eq!(hash, expected, "family {index}");
        }
    }

    #[test]
    fn runtime_seeded_selection_matches_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                frequency: 0.125,
                octaves: 2,
                gain: 0.65,
                ..SourceConfig::default()
            },
        ));
        let control = Module::source(Source::new(
            SourceKind::Simplex2,
            SourceConfig {
                seed: -91,
                frequency: 0.25,
                octaves: 2,
                ..SourceConfig::default()
            },
        ));
        let lower = control.clone().scale(Module::constant(0.35));
        let upper = source.clone().bias(Module::constant(0.2));
        let expected: [[u64; 4]; 15] = [
            [
                0x454ee30bb501f8ab,
                0x91167f59a2602b19,
                0x903487924e71cd82,
                0xcd4b473be6caf42c,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0xd572f200e25732e2,
                0xb8d31fd8e60b1af6,
                0x672717bb46dfb212,
                0x7c9068600a5b1ed0,
            ],
            [
                0xd572f200e25732e2,
                0xa4d12615eb1ed738,
                0xcbef9eb5c64f7039,
                0x450fd4be505a6f55,
            ],
            [
                0x5a12c5215b880dbb,
                0x95709a59a9c546fa,
                0x9ee78c913a3e2861,
                0xdde3d8548c4f2aaf,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0xd572f200e25732e2,
                0xac7116b4dbfa2b20,
                0xdbb67c1dc1d3be6b,
                0x46faf1dff18ebd14,
            ],
            [
                0xd572f200e25732e2,
                0xa4d12615eb1ed738,
                0xcbef9eb5c64f7039,
                0x450fd4be505a6f55,
            ],
            [
                0x4a0eaf34a20c8772,
                0x9704f259ac745882,
                0x3e0f2991d6cfd738,
                0x3d98e93d606a694d,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0x57209fa7d34d63c5,
                0x09bd100dd2bb63b7,
                0x2e8768c6417d27c1,
                0x55d8cdb8050840ba,
            ],
            [
                0xd572f200e25732e2,
                0x0d745838d1999f72,
                0x9f1bd15277ff1be1,
                0xd14921d7d5e43646,
            ],
            [
                0xd572f200e25732e2,
                0xa4d12615eb1ed738,
                0xcbef9eb5c64f7039,
                0x450fd4be505a6f55,
            ],
        ];
        for (interpolation_index, interpolation) in [
            Interpolation::Linear,
            Interpolation::Curve3,
            Interpolation::Curve4,
        ]
        .into_iter()
        .enumerate()
        {
            let graphs = [
                control.clone().select(
                    lower.clone(),
                    upper.clone(),
                    0.25,
                    0.75,
                    0.125,
                    interpolation,
                ),
                control.clone().select(
                    lower.clone(),
                    upper.clone(),
                    0.25,
                    0.75,
                    0.0,
                    interpolation,
                ),
                control.clone().select(
                    lower.clone(),
                    upper.clone(),
                    0.75,
                    0.25,
                    0.0,
                    interpolation,
                ),
                control.clone().multi_blend(
                    0.8,
                    interpolation,
                    vec![lower.clone(), source.clone(), upper.clone()],
                ),
                control.clone().multi_blend(
                    0.0,
                    interpolation,
                    vec![lower.clone(), source.clone(), upper.clone()],
                ),
            ];
            for (index, graph) in graphs.iter().enumerate() {
                for (seed, expected) in [0, 17, -91, i32::MAX]
                    .into_iter()
                    .zip(expected[interpolation_index * 5 + index])
                {
                    let mut hash = 0xcbf29ce484222325u64;
                    for (x, y) in [
                        (0.0, 0.0),
                        (0.125, -0.75),
                        (12.34, 56.78),
                        (-1234.5, 987.25),
                    ] {
                        hash ^=
                            u64::from(graph.try_value_seed_first(seed, x, y).unwrap().to_bits());
                        hash = hash.wrapping_mul(0x100000001b3);
                    }
                    assert_eq!(
                        hash, expected,
                        "interpolation {interpolation_index}, graph {index}, seed {seed}"
                    );
                }
            }
        }
    }

    #[test]
    fn seeded_selection_preserves_boundaries_and_lazy_children() {
        for (control, expected) in [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
            .into_iter()
            .zip([0.25, 0.25, 0.5, 0.75, 0.75, 0.75, 0.5, 0.25, 0.25])
        {
            let graph = Module::constant(control).select(
                Module::constant(0.25),
                Module::constant(0.75),
                0.25,
                0.75,
                0.125,
                Interpolation::Linear,
            );
            assert_eq!(graph.try_value_seed_first(17, 0.0, 0.0), Ok(expected));
        }
        let invalid = unsupported_seeded_node(0.5);
        for (control, lower, upper, expected) in [
            (0.0, Module::constant(0.25), invalid.clone(), 0.25),
            (0.5, invalid.clone(), Module::constant(0.75), 0.75),
            (1.0, Module::constant(0.25), invalid.clone(), 0.25),
        ] {
            assert_eq!(
                Module::constant(control)
                    .select(lower, upper, 0.25, 0.75, 0.125, Interpolation::Linear)
                    .try_value_seed_first(1, 0.0, 0.0),
                Ok(expected)
            );
        }
        for blend in [0.0, 0.8] {
            let graph = Module::constant(0.0).multi_blend(
                blend,
                Interpolation::Linear,
                vec![Module::constant(0.25), invalid.clone(), invalid.clone()],
            );
            assert_eq!(graph.try_value_seed_first(1, 0.0, 0.0), Ok(0.25));
        }
        let boundary = Module::constant(0.25).select(
            Module::constant(0.25),
            invalid.clone(),
            0.25,
            0.75,
            0.125,
            Interpolation::Linear,
        );
        assert!(boundary.try_value_seed_first(1, 0.0, 0.0).is_err());
        let reversed = Module::constant(0.5).select(
            Module::constant(0.25),
            invalid,
            0.75,
            0.25,
            0.0,
            Interpolation::Linear,
        );
        assert_eq!(reversed.try_value_seed_first(1, 0.0, 0.0), Ok(0.25));
    }

    /// `SeededSelectionEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): Select/MultiBlend with a
    /// control fixed at, and one ulp beside, every curve edge, slot edge and rounding edge (plus NaN, ±inf, ±0 and
    /// values outside 0..1); zero, -0, negative, NaN and infinite falloffs and blends; reversed, equal, overlapping,
    /// NaN and infinite bounds; one to four sources and nested selectors. Samples where Engine throws (a slot outside
    /// the sources) fold as `0xffffffff`, where the port panics.
    #[test]
    fn runtime_seeded_selection_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2, 0.65, 0.125);
        let control = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let c = Module::constant;
        let lo = control.clone().scale(c(0.35));
        let up = source.clone().bias(c(0.2));
        let (l, c3, c4) = (
            Interpolation::Linear,
            Interpolation::Curve3,
            Interpolation::Curve4,
        );
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        let sel = |lb: f32, ub: f32, f: f32| vec![lb - f, lb + f, ub - f, ub + f];
        // MultiBlend's constructor slots and `NoiseUtil.round`'s edges `(k + 0.5) / maxIndex`, as the oracle's `mb`.
        let mb = |n: i32, blend: f32| {
            let spacing = 1.0 / n as f32;
            let radius = spacing / 2.0;
            let cell_radius = (radius - radius * blend) / 2.0;
            let mut out = Vec::new();
            for i in 0..n {
                let pos = i as f32 * spacing + radius;
                let min = if i == 0 { 0.0 } else { pos - cell_radius };
                let max = if i == n - 1 { 1.0 } else { pos + cell_radius };
                out.extend([java_max(0.0, min), java_min(1.0, max)]);
            }
            out.extend((0..n - 1).map(|k| (k as f32 + 0.5) / (n - 1) as f32));
            out
        };
        type Family = Box<dyn Fn(Module) -> Module>;
        let select = |lower: &Module, upper: &Module, lb, ub, f, i| {
            let (lower, upper) = (lower.clone(), upper.clone());
            Box::new(move |k: Module| k.select(lower.clone(), upper.clone(), lb, ub, f, i))
                as Family
        };
        let multi = |blend, i, sources: Vec<Module>| {
            Box::new(move |k: Module| k.multi_blend(blend, i, sources.clone())) as Family
        };
        let three = vec![lo.clone(), source.clone(), up.clone()];
        let (nested_lo, nested_up) = (lo.clone(), up.clone());
        let inner = control
            .clone()
            .select(lo.clone(), up.clone(), 0.3, 0.6, 0.1, c3);
        let (pz, outer_up) = ((perlin.clone(), zero.clone()), up.clone());
        let families: Vec<(Family, Vec<f32>)> = vec![
            (
                select(&lo, &up, 0.25, 0.75, 0.125, l),
                sel(0.25, 0.75, 0.125),
            ),
            (
                select(&lo, &up, 0.25, 0.75, 0.125, c3),
                sel(0.25, 0.75, 0.125),
            ),
            (
                select(&lo, &up, 0.25, 0.75, 0.125, c4),
                sel(0.25, 0.75, 0.125),
            ),
            (select(&lo, &up, 0.25, 0.75, 0.0, l), sel(0.25, 0.75, 0.0)),
            (
                select(&lo, &up, 0.25, 0.75, -0.0, c3),
                sel(0.25, 0.75, -0.0),
            ),
            (select(&lo, &up, 0.75, 0.25, 0.0, l), sel(0.75, 0.25, 0.0)),
            (
                select(&lo, &up, 0.75, 0.25, 0.125, c3),
                sel(0.75, 0.25, 0.125),
            ),
            (
                select(&lo, &up, 0.25, 0.75, -0.125, l),
                sel(0.25, 0.75, -0.125),
            ),
            (select(&lo, &up, 0.25, 0.75, nan, l), vec![0.25, 0.75]),
            (select(&lo, &up, 0.25, 0.75, inf, c3), vec![]),
            (
                select(&lo, &up, nan, 0.75, 0.125, l),
                sel(0.75, 0.75, 0.125),
            ),
            (select(&lo, &up, -inf, inf, 0.1, c4), vec![]),
            (select(&lo, &up, 0.5, 0.5, 0.125, c4), sel(0.5, 0.5, 0.125)),
            (select(&lo, &up, 0.4, 0.5, 0.2, l), sel(0.4, 0.5, 0.2)),
            (
                select(&perlin, &zero, 0.25, 0.75, 0.125, l),
                sel(0.25, 0.75, 0.125),
            ),
            (multi(0.8, l, three.clone()), mb(3, 0.8)),
            (multi(0.8, c3, three.clone()), mb(3, 0.8)),
            (multi(0.8, c4, three.clone()), mb(3, 0.8)),
            (multi(0.0, l, three.clone()), mb(3, 0.0)),
            (multi(-0.5, l, three.clone()), mb(3, -0.5)),
            (multi(nan, c3, three.clone()), mb(3, nan)),
            (multi(inf, l, three), mb(3, inf)),
            (multi(1.0, l, vec![lo.clone(), up.clone()]), mb(2, 1.0)),
            (multi(0.8, c4, vec![up.clone()]), mb(1, 0.8)),
            (
                multi(
                    0.5,
                    c3,
                    vec![lo.clone(), perlin.clone(), zero.clone(), up.clone()],
                ),
                mb(4, 0.5),
            ),
            (
                Box::new(move |k: Module| {
                    let blend =
                        k.clone()
                            .multi_blend(0.6, l, vec![nested_lo.clone(), nested_up.clone()]);
                    k.select(blend, nested_up.clone(), 0.3, 0.7, 0.1, c4)
                }),
                sel(0.3, 0.7, 0.1),
            ),
            (
                Box::new(move |k: Module| {
                    k.multi_blend(0.8, l, vec![inner.clone(), pz.0.clone(), pz.1.clone()])
                }),
                mb(3, 0.8),
            ),
            (
                Box::new(move |k: Module| {
                    control
                        .clone()
                        .select(k, outer_up.clone(), 0.25, 0.75, 0.125, l)
                }),
                vec![],
            ),
        ];
        let expected: [u64; 28] = [
            0xfae2353372aa902a,
            0xfacaf9eb07abeb9d,
            0xd1394f80092eee6b,
            0x0b076edbbb9b39a7,
            0x0b076edbbb9b39a7,
            0x0254ef578f48f1ff,
            0x7de7aae8af94c617,
            0x856037abf854d397,
            0x5722d36fc293342f,
            0x8140df62a62c4a45,
            0xbf0a737ef81956b1,
            0x289952a1f15fe007,
            0x963ee0503b41860c,
            0x78eb54330c218951,
            0x154d8d8d27fd1a03,
            0x45a82743bb704526,
            0x742c3bd7f9dcf105,
            0x51627b2be93017c7,
            0x2a856dec816ccce7,
            0x819e56f9a80457bd,
            0xed17bf3a299a7456,
            0x5dff42a84d785641,
            0x8e6efa194c9210ef,
            0xb4d700345f8454d7,
            0x737657bbdfe4029e,
            0x94d25371ac4dc341,
            0xfd8e8f36004b0f27,
            0xf6aa889af07bb771,
        ];
        let specials = [nan, inf, -inf, 0.0, -0.0, 0.5, 1.0, 0.3, 0.7, -0.3, 1.3];
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, -987654321];
        let points = [
            (0.0, 0.0),
            (12.34, 56.78),
            (-1234.5, 987.25),
            (-0.0, 3.5),
            (f32::NAN, 1.0),
            (f32::INFINITY, 2.0),
        ];
        for (index, ((family, edges), expected)) in families.iter().zip(expected).enumerate() {
            let controls = edges
                .iter()
                .flat_map(|&e| [e, e.next_down(), e.next_up()])
                .chain(specials);
            let mut hash = 0xcbf29ce484222325u64;
            for v in controls {
                // A Java module overriding only `getValue`: the value `v` with `Noise`'s default 0..1 bounds.
                let graph = family(Module::node(ModuleKind::Constant(v), 0.0, 1.0));
                for seed in seeds {
                    for (x, y) in points {
                        let bits = match std::panic::catch_unwind(|| {
                            graph.try_value_seed_first(seed, x, y)
                        }) {
                            Ok(value) => {
                                let value = value.unwrap();
                                if value.is_nan() {
                                    0x7fc0_0000
                                } else {
                                    value.to_bits()
                                }
                            }
                            Err(_) => 0xffff_ffff,
                        };
                        hash = (hash ^ u64::from(bits)).wrapping_mul(0x100000001b3);
                    }
                }
            }
            assert_eq!(hash, expected, "family {index}");
        }
    }

    /// The graphs of `TerraForgedSeededCacheOracle.java` (`.local/terraforged/oracles/seed_first/`), with or
    /// without their caches.
    fn seeded_cache_graph(graph: usize, cached: bool) -> Module {
        let cache = |m: Module| if cached { m.cache() } else { m };
        let source = |kind, seed, frequency, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    frequency,
                    octaves,
                    gain,
                    ..SourceConfig::default()
                },
            ))
        };
        let simplex = || source(SourceKind::Simplex, i32::MAX, 0.125, 2, 0.65);
        match graph {
            0 => cache(simplex()),
            1 => {
                let s = cache(simplex());
                Module::add(vec![s.clone(), s.scale(Module::constant(0.5))])
            }
            2 => cache(Module::add(vec![
                source(SourceKind::Perlin2, -91, 0.25, 3, 0.5),
                simplex(),
            ])),
            3 => cache(cache(simplex()).bias(Module::constant(0.1))),
            4 => cache(source(SourceKind::Simplex, 1337, 3.0, 0, 0.5)),
            _ => {
                let s = cache(source(SourceKind::Ridge, 5, 0.5, 2, 0.5));
                cache(Module::add(vec![
                    Module::multiply(vec![s.clone(), cache(simplex())]),
                    s,
                ]))
            }
        }
    }

    const SEEDED_CACHE_SEEDS: [i32; 5] = [0, 17, -91, i32::MIN, i32::MAX];
    const SEEDED_CACHE_POINTS: [(f32, f32); 8] = [
        (0.0, 0.0),
        (0.125, -0.75),
        (12.34, 56.78),
        (-1234.5, 987.25),
        (f32::NAN, 1.0),
        (f32::INFINITY, -2.0),
        (-0.0, 0.0),
        (3.0e9, -3.0e9),
    ];

    /// What Java shows deterministically (one thread, fresh caches, a repeated point keeps its seed) matches the
    /// Noise2D 2cb4843 oracle; where 2cb4843's coordinate-only key returns another seed's sample ("stale"), the port
    /// gives the uncached sample instead.
    #[test]
    fn runtime_seeded_caches_match_current_noise2d() {
        let perlin2 = Module::source(Source::new(
            SourceKind::Perlin2,
            SourceConfig {
                seed: i32::MAX,
                octaves: 3,
                gain: 0.65,
                frequency: 0.125,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        // Perlin2 ignores the call seed, so these repeats are seed-safe in Java too.
        let cache = perlin2.clone().cache();
        for ((seed, x, y), expected) in [
            ((17, 1.25, -3.5), 0x3f0a_9872),
            ((-91, 1.25, -3.5), 0x3f0a_9872),
            ((-91, 12.34, 56.78), 0x3f0f_a18e),
            ((17, 12.34, 56.78), 0x3f0f_a18e),
            ((i32::MAX, f32::NAN, 0.0), 0x7fc0_0000),
            ((0, f32::NAN, 0.0), 0x7fc0_0000),
        ] {
            let value = cache.try_value_seed_first(seed, x, y).unwrap();
            assert_eq!(canonical_bits([value]), [expected]);
        }
        let zero_cache = perlin2.cache();
        assert_eq!(
            bits([
                zero_cache.try_value_seed_first(17, -0.0, 0.0).unwrap(),
                zero_cache.try_value_seed_first(-91, 0.0, 0.0).unwrap(),
            ]),
            [0x3f00_0000, 0x3f00_0000]
        );

        let seq: [u64; 6] = [
            0xafd0b59969690279,
            0x585880270ff8956d,
            0x22f5f16f69d0212b,
            0xfcc69f28dbdddd7b,
            0xd82ac363a10b8965,
            0x47368fac9224d9a7,
        ];
        // (Java's first sample at seed 17, its cached sample at seed -91, the uncached sample at seed -91).
        let stale: [[u32; 3]; 6] = [
            [0x3e4e29a8, 0x3e4e29a8, 0x3e81cb2a],
            [0x3e9a9f3e, 0x3e9a9f3e, 0x3ec2b0bf],
            [0x3f40c2cd, 0x3f40c2cd, 0x3f4e1df8],
            [0x3e9a4807, 0x3e9a4807, 0x3eb4fe5d],
            [0x3f800000, 0x3f800000, 0x3f800000],
            [0x3f847a91, 0x3f847a91, 0x3ebcdede],
        ];
        for graph in 0..6 {
            for cached in [true, false] {
                let module = seeded_cache_graph(graph, cached);
                let mut hash: u64 = 0xcbf29ce484222325;
                for seed in SEEDED_CACHE_SEEDS {
                    for (x, y) in SEEDED_CACHE_POINTS {
                        for _ in 0..2 {
                            let value = module.try_value_seed_first(seed, x, y).unwrap();
                            hash = (hash ^ canonical_bits([value])[0] as u64)
                                .wrapping_mul(0x100000001b3);
                        }
                    }
                }
                assert_eq!(hash, seq[graph], "graph {graph}, cached {cached}");
            }
            let module = seeded_cache_graph(graph, true);
            let (x, y) = SEEDED_CACHE_POINTS[2];
            let [first, _, uncached] = stale[graph];
            assert_eq!(
                bits([module.try_value_seed_first(17, x, y).unwrap()]),
                [first]
            );
            assert_eq!(
                bits([module.try_value_seed_first(-91, x, y).unwrap()]),
                [uncached]
            );
        }
    }

    /// One cache shared by threads that interleave runtime seeds at the same points, and the seedless path on the
    /// same entry, always give the uncached sample.
    #[test]
    fn seeded_cache_matches_uncached_across_seeds_and_threads() {
        for graph in 0..6 {
            let cached = seeded_cache_graph(graph, true);
            let uncached = seeded_cache_graph(graph, false);
            std::thread::scope(|scope| {
                for thread in 0..8 {
                    let (cached, uncached) = (&cached, &uncached);
                    scope.spawn(move || {
                        for i in 0..1500usize {
                            let seed = SEEDED_CACHE_SEEDS[(i + thread) % 5];
                            let (x, y) = SEEDED_CACHE_POINTS[(i / 5) % 8];
                            let expected = uncached.try_value_seed_first(seed, x, y).unwrap();
                            let value = cached.try_value_seed_first(seed, x, y).unwrap();
                            assert_eq!(canonical_bits([value]), canonical_bits([expected]));
                            if i % 7 == 0 {
                                assert_eq!(
                                    canonical_bits([cached.value(x, y)]),
                                    canonical_bits([uncached.value(x, y)])
                                );
                            }
                        }
                    });
                }
            });
        }
        // A zero-octave source gives 1 seed-first (Noise2D 2cb4843's map) and NaN seedless (the jar's): the two paths
        // never read each other's entry, and the seedless `value` and `with_seed` keep their bits.
        let zero = seeded_cache_graph(4, true);
        assert_eq!(
            bits([zero.try_value_seed_first(17, 1.0, 2.0).unwrap()]),
            [0x3f800000]
        );
        assert!(zero.value(1.0, 2.0).is_nan());
        assert_eq!(
            bits([zero.try_value_seed_first(17, 1.0, 2.0).unwrap()]),
            [0x3f800000]
        );
        for graph in 0..6 {
            let cached = seeded_cache_graph(graph, true);
            let uncached = seeded_cache_graph(graph, false);
            for (x, y) in SEEDED_CACHE_POINTS {
                let _ = cached.try_value_seed_first(-91, x, y);
                assert_eq!(
                    canonical_bits([cached.value(x, y), cached.with_seed(17).value(x, y)]),
                    canonical_bits([uncached.value(x, y), uncached.with_seed(17).value(x, y)])
                );
            }
        }
    }

    #[test]
    fn seeded_cache_does_not_store_failed_children() {
        let unsupported = unsupported_seeded_node(0.5);
        let cache = unsupported.cache();
        assert!(cache.try_value_seed_first(17, 1.0, 2.0).is_err());
        assert!(cache.try_value_seed_first(-91, 1.0, 2.0).is_err());
    }

    #[test]
    fn runtime_seeded_terrace_matches_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                octaves: 3,
                gain: 0.65,
                frequency: 0.125,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let ramp = Module::source(Source::new(
            SourceKind::Perlin2,
            SourceConfig {
                seed: -91,
                octaves: 2,
                gain: 0.5,
                frequency: 0.25,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let cliff = Module::source(Source::new(
            SourceKind::Simplex2,
            SourceConfig {
                seed: 17,
                octaves: 2,
                gain: 0.5,
                frequency: 0.0625,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let height = Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed: 1234,
                octaves: 1,
                gain: 0.5,
                frequency: 0.5,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let terrace = source.terrace(ramp, cliff, height, 7, 0.35);
        for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip([
            0xa9cf707ca11f7949,
            0x89d50db38b44a5ba,
            0xe3348fd1ddadf84f,
            0x2cf2f48543c12e94,
        ]) {
            let mut hash = 0xcbf29ce484222325u64;
            for (x, y) in [
                (0.0, 0.0),
                (0.125, -0.75),
                (12.34, 56.78),
                (-1234.5, 987.25),
            ] {
                hash ^= u64::from(terrace.try_value_seed_first(seed, x, y).unwrap().to_bits());
                hash = hash.wrapping_mul(0x100000001b3);
            }
            assert_eq!(hash, expected, "seed {seed}");
        }
        // The oracle's `boundary` lines: a fixed source (0..1 bounds) at, inside and outside five steps' windows.
        let boundaries = [-1.0, 0.0, 0.05, 0.1, 0.2, 0.5, 0.9, 0.999999, 1.0, 2.0].map(|value| {
            Module::node(ModuleKind::Constant(value), 0.0, 1.0)
                .terrace(
                    Module::constant(0.4),
                    Module::constant(0.7),
                    Module::constant(0.8),
                    5,
                    0.25,
                )
                .try_value_seed_first(17, 0.0, 0.0)
                .unwrap()
                .to_bits()
        });
        assert_eq!(
            boundaries,
            [
                0x00000000, 0x00000000, 0x3daaaaab, 0x3e800000, 0x3e800000, 0x3f000000, 0x3f800000,
                0x3f800000, 0x3f800000, 0x3f800000,
            ]
        );
    }

    /// `SeededTerraceEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): Terrace with a source fixed
    /// at, and one ulp beside, every step's value and window bounds (the constructor's table) and every index edge
    /// `k / steps` (plus NaN, ±inf, ±0, subnormals and values outside 0..1); 0, 1, 2, 5, 7, 64 and 1000 steps; zero,
    /// one, inverted, negative and NaN blend ranges; ramps and cliffs pinned to 0 or 1, NaN and infinite ramp heights;
    /// zero, wide, narrow and infinite source ranges; seeded ramp/cliff/height sources; nested terraces; and terraces
    /// of seeded sources. Samples where Java throws (a NaN sample, no steps) fold as `0xffffffff`, where the port
    /// panics.
    #[test]
    fn runtime_seeded_terrace_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let ramp = src(SourceKind::Perlin2, -91, 2, 0.5, 0.25);
        let cliff = src(SourceKind::Simplex2, 17, 2, 0.5, 0.0625);
        let height = src(SourceKind::Perlin, 1234, 1, 0.5, 0.5);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let cubic = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let simplex = src(SourceKind::Simplex, i32::MAX, 3, 0.65, 0.125);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let c = Module::constant;
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        type Build = Box<dyn Fn(Module) -> Module>;
        let plain = |steps, blend: f32, ramp: Module, cliff: Module, height: Module| {
            Box::new(move |k: Module| {
                k.terrace(ramp.clone(), cliff.clone(), height.clone(), steps, blend)
            }) as Build
        };
        let consts = |steps, blend| plain(steps, blend, c(0.4), c(0.7), c(0.8));
        let seeded =
            |steps, blend| plain(steps, blend, ramp.clone(), cliff.clone(), height.clone());
        let fixed = |source: &Module,
                     steps,
                     blend,
                     ramp: &Module,
                     cliff: &Module,
                     height: &Module| {
            let (source, ramp, cliff, height) =
                (source.clone(), ramp.clone(), cliff.clone(), height.clone());
            Box::new(move |_: Module| {
                source
                    .clone()
                    .terrace(ramp.clone(), cliff.clone(), height.clone(), steps, blend)
            }) as Build
        };
        let (nested_cubic, nested_cliff) = (cubic.clone(), cliff.clone());
        // (steps of the edge table or None for no edges, blend, source bounds, terrace)
        let families: Vec<(Option<usize>, f32, (f32, f32), Build)> = vec![
            (Some(0), 0.25, (0.0, 1.0), consts(0, 0.25)),
            (Some(1), 0.25, (0.0, 1.0), consts(1, 0.25)),
            (Some(1), 0.25, (0.5, 0.5), consts(1, 0.25)),
            (Some(2), 0.25, (0.0, 1.0), consts(2, 0.25)),
            (Some(5), 0.25, (0.0, 1.0), consts(5, 0.25)),
            (Some(7), 0.35, (0.0, 1.0), seeded(7, 0.35)),
            (Some(64), 0.5, (0.0, 1.0), seeded(64, 0.5)),
            (Some(1000), 0.1, (0.0, 1.0), consts(1000, 0.1)),
            (Some(5), 0.0, (0.0, 1.0), consts(5, 0.0)),
            (Some(5), 1.0, (0.0, 1.0), consts(5, 1.0)),
            (Some(5), 1.5, (0.0, 1.0), consts(5, 1.5)),
            (Some(5), -0.5, (0.0, 1.0), consts(5, -0.5)),
            (Some(5), nan, (0.0, 1.0), consts(5, nan)),
            (
                Some(5),
                0.25,
                (0.0, 1.0),
                plain(5, 0.25, c(0.0), c(2.0), c(0.8)),
            ),
            (
                Some(5),
                0.25,
                (0.0, 1.0),
                plain(5, 0.25, c(2.0), c(0.0), c(nan)),
            ),
            (
                Some(5),
                0.25,
                (0.0, 1.0),
                plain(5, 0.25, c(nan), c(inf), c(inf)),
            ),
            (Some(4), 0.25, (-1.0, 1.0), consts(4, 0.25)),
            (Some(4), 0.25, (0.0, 0.5), consts(4, 0.25)),
            (Some(3), 0.25, (0.0, inf), consts(3, 0.25)),
            (
                Some(6),
                0.3,
                (0.0, 1.0),
                plain(6, 0.3, zero.clone(), cubic.clone(), simplex.clone()),
            ),
            (
                Some(5),
                0.25,
                (0.0, 1.0),
                Box::new(move |k: Module| {
                    let inner = k.terrace(c(0.4), c(0.7), c(0.8), 3, 0.2);
                    let ramp = nested_cubic.clone().terrace(c(0.4), c(0.7), c(0.8), 4, 0.5);
                    inner.terrace(ramp, nested_cliff.clone(), c(1.0), 5, 0.25)
                }),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&simplex, 7, 0.35, &ramp, &cliff, &height),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&simplex, 1, 0.35, &ramp, &cliff, &height),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&perlin, 64, 0.5, &cubic, &zero, &simplex),
            ),
        ];
        let expected: [(usize, u64); 24] = [
            (18, 0x2119df8b19ee9821),
            (30, 0x962f09fee93f2a61),
            (30, 0x962f09fee93f2a61),
            (42, 0x5cf4a394fc005bd3),
            (78, 0x17bcf3e0f34db697),
            (102, 0x64253079cba9b63e),
            (786, 0xf95cff3f3aafa9e2),
            (12018, 0x83e3f9604c1f2e55),
            (78, 0x45d0e71922dcb3fd),
            (78, 0x22e2943cef5ec9f3),
            (78, 0x9ec99c7685dec9f3),
            (78, 0x1c86a9fe14672b7d),
            (78, 0xb42be868e5a124c7),
            (78, 0x3fb4a8854088c719),
            (78, 0x07a1a73b5e5ec9f3),
            (78, 0x71101621135ec9f3),
            (66, 0xc90882f17fe04349),
            (66, 0xa5e3254c0268d349),
            (54, 0xd2092bd1e2c6ff49),
            (90, 0xd74a9e768040c84f),
            (78, 0xb637f2a9331518c1),
            (12, 0xffed50e4d69499ad),
            (12, 0x5f8be96e8a503db5),
            (12, 0x785df491993d4365),
        ];
        // The constructor's step table (value, lower, upper per step), the index edges `k / steps` and 0.999999.
        let edges = |steps: usize, blend_range: f32, (min, max): (f32, f32)| {
            let spacing = (max - min) / (steps as i32 - 1) as f32;
            let mut out = Vec::new();
            for i in 0..steps {
                let value = i as f32 * spacing;
                let bound = (spacing - spacing * blend_range) / 2.0;
                out.extend([value, value - bound, value + bound]);
            }
            out.extend((0..=steps).map(|k| k as f32 / steps as f32));
            out.push(0.999999);
            out
        };
        let specials = [
            nan,
            inf,
            -inf,
            0.0,
            -0.0,
            0.5,
            1.0,
            -1.0,
            2.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            0.999999f32.next_up(),
        ];
        for (index, ((steps, blend, bounds, build), expected)) in
            families.iter().zip(expected).enumerate()
        {
            let table = steps.map_or_else(Vec::new, |steps| edges(steps, *blend, *bounds));
            let fold = seeded_edge_fold(&table, &specials, *bounds, build);
            assert_eq!(fold, expected, "family {index}");
        }
    }

    /// The fold of the seed-first edge oracles (`SeededTerraceEdgeOracle`, `SeededLegacyTerraceEdgeOracle`): each
    /// table value and one ulp below and above it, then the specials, as a Java module returning that value with
    /// `bounds` (a `Constant` node), put under `build` and sampled over seven seeds (both `int` extremes) and six
    /// points (NaN, infinity, -0). Canonical NaN bits; a sample that panics (where Java throws) folds as `0xffffffff`.
    /// Returns the number of values and the fold.
    fn seeded_edge_fold(
        table: &[f32],
        specials: &[f32],
        bounds: (f32, f32),
        build: impl Fn(Module) -> Module,
    ) -> (usize, u64) {
        let seeds = [0, 1, -1, 17, i32::MAX, i32::MIN, -987654321];
        let points = [
            (0.0, 0.0),
            (12.34, 56.78),
            (-1234.5, 987.25),
            (-0.0, 3.5),
            (f32::NAN, 1.0),
            (f32::INFINITY, 2.0),
        ];
        let values: Vec<f32> = table
            .iter()
            .flat_map(|&e| [e, e.next_down(), e.next_up()])
            .chain(specials.iter().copied())
            .collect();
        let mut hash = 0xcbf29ce484222325u64;
        for &v in &values {
            let graph = build(Module::node(ModuleKind::Constant(v), bounds.0, bounds.1));
            for seed in seeds {
                for (x, y) in points {
                    let bits =
                        match std::panic::catch_unwind(|| graph.try_value_seed_first(seed, x, y)) {
                            Ok(value) => {
                                let value = value.unwrap();
                                if value.is_nan() {
                                    0x7fc0_0000
                                } else {
                                    value.to_bits()
                                }
                            }
                            Err(_) => 0xffff_ffff,
                        };
                    hash = (hash ^ u64::from(bits)).wrapping_mul(0x100000001b3);
                }
            }
        }
        (values.len(), hash)
    }

    #[test]
    fn seeded_terrace_matches_reseeding_and_evaluates_children_lazily() {
        // Over non-Perlin sources with finite, nonzero ranges, reseeding and seed-first sampling seed the same
        // sources and use the same steps (both from the constructor's bounds), so they give the same bits.
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let ramp = src(SourceKind::Cubic, -91, 3);
        let cliff = src(SourceKind::Simplex2, i32::MIN, 2);
        let height = src(SourceKind::Ridge, 17, 2);
        for steps in [1, 2, 7, 64] {
            let graph =
                source
                    .clone()
                    .terrace(ramp.clone(), cliff.clone(), height.clone(), steps, 0.35);
            for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                let reseeded = graph.with_seed(i64::from(seed));
                for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, 3.5), (1e30, 3.0)] {
                    assert_eq!(
                        canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                        canonical_bits([reseeded.value(x, y)]),
                        "steps {steps}, seed {seed}, ({x}, {y})"
                    );
                }
            }
        }
        // Five steps over 0..1 (windows ±0.09375 around 0, 0.25, ...; ramp 0.8, cliff 0.65): flat samples read no
        // child, the window reads ramp and cliff, and only alpha above the ramp reads the ramp height.
        let unsupported = unsupported_seeded_node(0.5);
        let c = Module::constant;
        let terrace = |value, ramp: &Module, cliff: &Module, height: &Module| {
            Module::node(ModuleKind::Constant(value), 0.0, 1.0)
                .terrace(ramp.clone(), cliff.clone(), height.clone(), 5, 0.25)
                .try_value_seed_first(1, 0.0, 0.0)
        };
        for value in [0.12, 0.999] {
            assert!(terrace(value, &unsupported, &unsupported, &unsupported).is_ok());
        }
        for value in [0.05, 0.25, 0.34] {
            assert!(terrace(value, &unsupported, &c(0.7), &c(0.8)).is_err());
            assert!(terrace(value, &c(0.4), &unsupported, &c(0.8)).is_err());
        }
        assert!(terrace(0.25, &c(0.4), &c(0.7), &unsupported).is_ok());
        assert!(terrace(0.34, &c(0.4), &c(0.7), &unsupported).is_err());
    }

    #[test]
    fn runtime_seeded_legacy_terrace_matches_current_noise2d() {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: i32::MAX,
                octaves: 3,
                gain: 0.65,
                frequency: 0.125,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let lower = Module::source(Source::new(
            SourceKind::Perlin2,
            SourceConfig {
                seed: -91,
                octaves: 2,
                gain: 0.5,
                frequency: 0.25,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let upper = Module::source(Source::new(
            SourceKind::Simplex2,
            SourceConfig {
                seed: 17,
                octaves: 2,
                gain: 0.5,
                frequency: 0.0625,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let terrace = source.legacy_terrace(lower, upper, 7, 0.35);
        for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip([
            0x4f7a00177771432e,
            0x89d50db38b44a5ba,
            0xa98eef2a5f901194,
            0x2e30f88545dd8f60,
        ]) {
            let mut hash = 0xcbf29ce484222325u64;
            for (x, y) in [
                (0.0, 0.0),
                (0.125, -0.75),
                (12.34, 56.78),
                (-1234.5, 987.25),
            ] {
                hash ^= u64::from(terrace.try_value_seed_first(seed, x, y).unwrap().to_bits());
                hash = hash.wrapping_mul(0x100000001b3);
            }
            assert_eq!(hash, expected, "seed {seed}");
        }
    }

    #[test]
    fn seeded_legacy_terrace_matches_reseeding_and_evaluates_curves_lazily() {
        // Over non-Perlin sources with finite, nonzero ranges, reseeding and seed-first sampling seed the same
        // sources and use the same steps (both from the constructor's bounds), so they give the same bits.
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let lower = src(SourceKind::Cubic, -91, 3);
        let upper = src(SourceKind::Simplex2, i32::MIN, 2);
        for steps in [1, 2, 7, 64] {
            for blend in [0.35, 3.0] {
                let graph =
                    source
                        .clone()
                        .legacy_terrace(lower.clone(), upper.clone(), steps, blend);
                for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                    let reseeded = graph.with_seed(i64::from(seed));
                    for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, 3.5), (1e30, 3.0)] {
                        assert_eq!(
                            canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                            canonical_bits([reseeded.value(x, y)]),
                            "steps {steps}, blend {blend}, seed {seed}, ({x}, {y})"
                        );
                    }
                }
            }
        }
        // Five steps over 0..1 (windows ±0.09375 around 0, 0.25, ...): flat samples read no curve, below a window
        // reads only the upper curve, above it only the lower curve.
        let unsupported = unsupported_seeded_node(0.5);
        let c = Module::constant;
        let terrace = |value, blend, lower: &Module, upper: &Module| {
            Module::node(ModuleKind::Constant(value), 0.0, 1.0)
                .legacy_terrace(lower.clone(), upper.clone(), 5, blend)
                .try_value_seed_first(1, 0.0, 0.0)
        };
        for value in [0.0, 0.2, 0.5, 1.0, f32::NAN] {
            assert!(terrace(value, 0.25, &unsupported, &unsupported).is_ok());
        }
        assert!(terrace(0.14, 0.25, &unsupported, &c(0.9)).is_ok());
        assert!(terrace(0.14, 0.25, &c(0.6), &unsupported).is_err());
        assert!(terrace(0.36, 0.25, &c(0.6), &unsupported).is_ok());
        assert!(terrace(0.36, 0.25, &unsupported, &c(0.9)).is_err());
        // A blend range of 3 inverts the windows (first step: 0.25..-0.25): 0.1 lies below the first step's window
        // and above its upper bound, and Java returns the step without reading the lower curve.
        assert!(terrace(0.1, 3.0, &unsupported, &unsupported).is_ok());
    }

    /// `SeededLegacyTerraceEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): LegacyTerrace with a
    /// source fixed at, and one ulp beside, every step's value and window bounds (the constructor's table) and every
    /// rounding edge `(k + 0.5) / maxIndex` (plus NaN, ±inf, ±0, subnormals and values outside 0..1); 0, 1, 2, 5, 7, 64
    /// and 1000 steps; zero, one, inverted (above 1), negative and NaN blend ranges; zero, NaN and infinite curves;
    /// zero, wide, narrow and infinite source ranges; seeded curves; nested legacy terraces; and legacy terraces of
    /// seeded sources. Samples where Java throws (no steps) fold as `0xffffffff`, where the port panics.
    #[test]
    fn runtime_seeded_legacy_terrace_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let lower = src(SourceKind::Perlin2, -91, 2, 0.5, 0.25);
        let upper = src(SourceKind::Simplex2, 17, 2, 0.5, 0.0625);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let cubic = src(SourceKind::Cubic, -91, 2, 0.5, 0.25);
        let simplex = src(SourceKind::Simplex, i32::MAX, 3, 0.65, 0.125);
        let perlin = src(SourceKind::Perlin, 53, 2, 0.5, 0.5);
        let c = Module::constant;
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        type Build = Box<dyn Fn(Module) -> Module>;
        let plain = |steps, blend: f32, lower: Module, upper: Module| {
            Box::new(move |k: Module| k.legacy_terrace(lower.clone(), upper.clone(), steps, blend))
                as Build
        };
        let consts = |steps, blend| plain(steps, blend, c(0.6), c(0.9));
        let seeded = |steps, blend| plain(steps, blend, lower.clone(), upper.clone());
        let fixed = |source: &Module, steps, blend, lower: &Module, upper: &Module| {
            let (source, lower, upper) = (source.clone(), lower.clone(), upper.clone());
            Box::new(move |_: Module| {
                source
                    .clone()
                    .legacy_terrace(lower.clone(), upper.clone(), steps, blend)
            }) as Build
        };
        let (nested_cubic, nested_upper) = (cubic.clone(), upper.clone());
        // (steps of the edge table or None for no edges, blend, source bounds, legacy terrace)
        let families: Vec<(Option<usize>, f32, (f32, f32), Build)> = vec![
            (Some(0), 0.25, (0.0, 1.0), consts(0, 0.25)),
            (Some(1), 0.25, (0.0, 1.0), consts(1, 0.25)),
            (Some(1), 0.25, (0.5, 0.5), consts(1, 0.25)),
            (Some(2), 0.25, (0.0, 1.0), consts(2, 0.25)),
            (Some(5), 0.25, (0.0, 1.0), consts(5, 0.25)),
            (Some(7), 0.35, (0.0, 1.0), seeded(7, 0.35)),
            (Some(64), 0.5, (0.0, 1.0), seeded(64, 0.5)),
            (Some(1000), 0.1, (0.0, 1.0), consts(1000, 0.1)),
            (Some(5), 0.0, (0.0, 1.0), consts(5, 0.0)),
            (Some(5), 1.0, (0.0, 1.0), consts(5, 1.0)),
            (Some(5), 1.5, (0.0, 1.0), consts(5, 1.5)),
            (Some(5), 3.0, (0.0, 1.0), seeded(5, 3.0)),
            (Some(5), -0.5, (0.0, 1.0), consts(5, -0.5)),
            (Some(5), nan, (0.0, 1.0), consts(5, nan)),
            (Some(5), 0.25, (0.0, 1.0), plain(5, 0.25, c(0.0), c(2.0))),
            (Some(5), 0.25, (0.0, 1.0), plain(5, 0.25, c(nan), c(inf))),
            (Some(4), 0.25, (-1.0, 1.0), consts(4, 0.25)),
            (Some(4), 0.25, (0.0, 0.5), consts(4, 0.25)),
            (Some(3), 0.25, (0.0, inf), consts(3, 0.25)),
            (
                Some(6),
                0.3,
                (0.0, 1.0),
                plain(6, 0.3, zero.clone(), cubic.clone()),
            ),
            (
                Some(5),
                0.25,
                (0.0, 1.0),
                Box::new(move |k: Module| {
                    let inner = k.legacy_terrace(c(0.6), c(0.9), 3, 0.2);
                    let lower = nested_cubic.clone().legacy_terrace(c(0.6), c(0.9), 4, 0.5);
                    inner.legacy_terrace(lower, nested_upper.clone(), 5, 0.25)
                }),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&simplex, 7, 0.35, &lower, &upper),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&simplex, 1, 0.35, &lower, &upper),
            ),
            (
                None,
                0.0,
                (0.0, 1.0),
                fixed(&perlin, 64, 0.5, &cubic, &zero),
            ),
        ];
        let expected: [(usize, u64); 24] = [
            (15, 0xac00b784dc89d43f),
            (24, 0x89325b1f85d181e5),
            (24, 0x89325b1f85d181e5),
            (36, 0x9a346d92cce70d61),
            (72, 0x9f7ca0645a1f581d),
            (96, 0x8760a98042bd03c3),
            (780, 0xad022569110921a5),
            (12012, 0xf1e84c5fdcc5d113),
            (72, 0xa85a6e0612372f65),
            (72, 0x7b6627a8291ec347),
            (72, 0x08c3fec71b409e1d),
            (72, 0x7126e77349edacf7),
            (72, 0x0cf9cee70e372f65),
            (72, 0xa715fb322e372f65),
            (72, 0xa9aa5c6fafada48d),
            (72, 0x44a682fcb7b72f65),
            (60, 0x13a55b77628248c5),
            (60, 0x655003d9a3ec2561),
            (48, 0x57df34647b5f30a5),
            (84, 0x5344558cb4ae6229),
            (72, 0x6378400bcd73dc8e),
            (12, 0x740d5d9134f0c285),
            (12, 0x463f236bcb8ac885),
            (12, 0x71892f18c71d644d),
        ];
        // The constructor's step table (value, lower, upper per step), the rounding edges `(k + 0.5) / maxIndex`
        // and 1.
        let edges = |steps: usize, blend_range: f32, (min, max): (f32, f32)| {
            let max_index = steps as i32 - 1;
            let spacing = (max - min) / max_index as f32;
            let mut out = Vec::new();
            for i in 0..steps {
                let value = i as f32 * spacing;
                let bound = (spacing - spacing * blend_range) / 2.0;
                out.extend([value, value - bound, value + bound]);
            }
            out.extend((0..max_index.max(0)).map(|k| (k as f32 + 0.5) / max_index as f32));
            out.push(1.0);
            out
        };
        let specials = [
            nan,
            inf,
            -inf,
            0.0,
            -0.0,
            0.5,
            1.0,
            -1.0,
            2.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            1.0f32.next_down(),
        ];
        for (index, ((steps, blend, bounds, build), expected)) in
            families.iter().zip(expected).enumerate()
        {
            let table = steps.map_or_else(Vec::new, |steps| edges(steps, *blend, *bounds));
            let fold = seeded_edge_fold(&table, &specials, *bounds, build);
            assert_eq!(fold, expected, "family {index}");
        }
    }

    /// `TerraForgedSeededAdvancedTerraceOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): the PR's
    /// advanced terrace over seeded Perlin/Simplex/Ridge/Perlin sources, four seeds, four points, one fold per seed.
    #[test]
    fn runtime_seeded_advanced_terrace_matches_current_noise2d() {
        let source = |kind, seed, frequency, octaves, gain| {
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
        };
        let terrace = source(SourceKind::Perlin, 801, 1.0 / 96.0, 4, 0.5).advanced_terrace(
            source(SourceKind::Simplex, 802, 1.0 / 80.0, 3, 0.5),
            source(SourceKind::Ridge, 803, 1.0 / 128.0, 2, 0.975),
            source(SourceKind::Perlin, 804, 1.0 / 64.0, 2, 0.5),
            0.2,
            0.8,
            8,
            3,
        );
        for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip([
            0x9da2a5c3366d4444,
            0x9f2c62d939ce57c2,
            0xcbc7c4d2781a530f,
            0x30888fb4340299f7,
        ]) {
            let mut hash = 0xcbf29ce484222325u64;
            for (x, y) in [
                (0.0, 0.0),
                (0.125, -0.75),
                (12.34, 56.78),
                (-1234.5, 987.25),
            ] {
                hash ^= u64::from(terrace.try_value_seed_first(seed, x, y).unwrap().to_bits());
                hash = hash.wrapping_mul(0x100000001b3);
            }
            assert_eq!(hash, expected, "seed {seed}");
        }
    }

    #[test]
    fn seeded_advanced_terrace_matches_reseeding_and_evaluates_children_lazily() {
        // Over non-Perlin sources with finite, nonzero ranges, reseeding and seed-first sampling seed the same
        // sources and fix the same `modRange`, so they give the same bits.
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let modulation = src(SourceKind::Cubic, -91, 3);
        let mask = src(SourceKind::Simplex2, i32::MIN, 2);
        let slope = src(SourceKind::Ridge, 17, 2);
        for (steps, octaves) in [(1, 1), (4, 2), (8, 3), (i32::MAX, 3)] {
            let graph = source.clone().advanced_terrace(
                modulation.clone(),
                mask.clone(),
                slope.clone(),
                0.2,
                0.8,
                steps,
                octaves,
            );
            for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                let reseeded = graph.with_seed(i64::from(seed));
                for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, 3.5), (1e30, 3.0)] {
                    assert_eq!(
                        canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                        canonical_bits([reseeded.value(x, y)]),
                        "steps {steps}, octaves {octaves}, seed {seed}, ({x}, {y})"
                    );
                }
            }
        }
        // A sample at or below blendMin reads no child; a zero (or -0) mask reads neither slope nor modulation;
        // otherwise all three are read. A NaN sample is not at or below blendMin.
        let unsupported = unsupported_seeded_node(0.5);
        let c = Module::constant;
        let terrace = |value, modulation: &Module, mask: &Module, slope: &Module| {
            Module::node(ModuleKind::Constant(value), 0.0, 1.0)
                .advanced_terrace(
                    modulation.clone(),
                    mask.clone(),
                    slope.clone(),
                    0.2,
                    0.8,
                    4,
                    2,
                )
                .try_value_seed_first(1, 0.0, 0.0)
        };
        for value in [0.2, -1.0, f32::NEG_INFINITY] {
            assert!(terrace(value, &unsupported, &unsupported, &unsupported).is_ok());
        }
        for value in [0.5, f32::NAN] {
            assert!(terrace(value, &c(0.1), &unsupported, &c(0.25)).is_err());
            assert!(terrace(value, &unsupported, &c(0.0), &unsupported).is_ok());
            assert!(terrace(value, &unsupported, &c(-0.0), &unsupported).is_ok());
            assert!(terrace(value, &c(0.1), &c(0.5), &unsupported).is_err());
            assert!(terrace(value, &unsupported, &c(0.5), &c(0.25)).is_err());
            assert!(terrace(value, &c(0.1), &c(0.5), &c(0.25)).is_ok());
        }
    }

    /// `SeededAdvancedTerraceEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): AdvancedTerrace with
    /// a source fixed at, and one ulp beside, blendMin, blendMax and (first two octaves, at most 64 steps) every step
    /// value `k / n` and rounding edge `(k + 0.5) / n` (plus NaN, ±inf, ±0, subnormals and values outside 0..1); masks
    /// 1, 0.5, ±0, -1, NaN and inf; slopes 0, 1, -0.5 and NaN; 0, 1, negative and wrapping (`i32::MAX * i`) steps; 0,
    /// -1, 1 and 3 octaves; equal, inverted, NaN and infinite blend bounds; zero, infinite and NaN `modRange`; seeded
    /// children; nested terraces; and advanced terraces of seeded sources.
    #[test]
    fn runtime_seeded_advanced_terrace_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let modulation = src(SourceKind::Simplex2, 802, 3, 0.5, 0.0125);
        let mask = src(SourceKind::Cubic, 803, 2, 0.975, 0.0078125);
        let slope = src(SourceKind::Perlin2, 804, 2, 0.5, 0.015625);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let simplex = src(SourceKind::Simplex, i32::MAX, 3, 0.65, 0.125);
        let perlin = src(SourceKind::Perlin, 801, 4, 0.5, 1.0 / 96.0);
        let c = Module::constant;
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        type Build = Box<dyn Fn(Module) -> Module>;
        // (blendMin, blendMax, steps, octaves) of the edge table, or None for no edges; source bounds; terrace.
        type Family = (Option<(f32, f32, i32, i32)>, (f32, f32), Build);
        fn plain(
            (bmin, bmax, steps, octaves): (f32, f32, i32, i32),
            bounds: (f32, f32),
            modulation: &Module,
            mask: &Module,
            slope: &Module,
        ) -> Family {
            let (modulation, mask, slope) = (modulation.clone(), mask.clone(), slope.clone());
            (
                Some((bmin, bmax, steps, octaves)),
                bounds,
                Box::new(move |k: Module| {
                    k.advanced_terrace(
                        modulation.clone(),
                        mask.clone(),
                        slope.clone(),
                        bmin,
                        bmax,
                        steps,
                        octaves,
                    )
                }),
            )
        }
        let consts =
            |args, mask: f32, slope: f32| plain(args, (0.0, 1.0), &c(0.1), &c(mask), &c(slope));
        let fixed =
            |source: &Module, modulation: &Module, mask: &Module, args: (f32, f32, i32, i32)| {
                let (source, modulation, mask, slope) = (
                    source.clone(),
                    modulation.clone(),
                    mask.clone(),
                    slope.clone(),
                );
                let (bmin, bmax, steps, octaves) = args;
                (
                    None,
                    (0.0, 1.0),
                    Box::new(move |_: Module| {
                        source.clone().advanced_terrace(
                            modulation.clone(),
                            mask.clone(),
                            slope.clone(),
                            bmin,
                            bmax,
                            steps,
                            octaves,
                        )
                    }) as Build,
                )
            };
        let (n_mod, n_mask, n_slope) = (modulation.clone(), mask.clone(), slope.clone());
        let families: Vec<Family> = vec![
            consts((0.2, 0.8, 8, 3), 1.0, 0.25),
            consts((0.2, 0.8, 8, 3), 0.5, 0.25),
            consts((0.2, 0.8, 8, 3), 0.0, 0.25),
            consts((0.2, 0.8, 8, 3), -0.0, 0.25),
            consts((0.2, 0.8, 4, 2), -1.0, -0.5),
            consts((0.2, 0.8, 4, 2), nan, 0.25),
            consts((0.2, 0.8, 4, 2), inf, 0.25),
            consts((0.2, 0.8, 5, 2), 1.0, 0.0),
            consts((0.2, 0.8, 5, 2), 0.5, 1.0),
            consts((0.2, 0.8, 5, 2), 1.0, nan),
            consts((0.2, 0.8, 0, 2), 1.0, 0.25),
            consts((0.2, 0.8, -4, 2), 0.5, 0.25),
            consts((0.2, 0.8, i32::MAX, 3), 1.0, 0.25),
            consts((0.2, 0.8, 6, 0), 0.5, 0.25),
            consts((0.2, 0.8, 6, -1), 0.5, 0.25),
            consts((0.2, 0.8, 1, 1), 0.5, 0.25),
            consts((0.5, 0.5, 4, 2), 1.0, 0.25),
            consts((0.8, 0.2, 4, 2), 0.5, 0.25),
            consts((nan, 0.8, 4, 2), 0.5, 0.25),
            consts((0.2, nan, 4, 2), 0.5, 0.25),
            consts((-inf, inf, 4, 2), 0.5, 0.25),
            plain((-0.5, 0.8, 4, 2), (-1.0, -0.1), &c(0.1), &c(0.5), &c(0.25)),
            plain((0.2, 0.8, 4, 2), (0.0, inf), &c(0.1), &c(0.5), &c(0.25)),
            plain((0.2, 0.8, 4, 2), (0.0, 1.0), &c(nan), &c(0.5), &c(0.25)),
            plain((0.2, 0.8, 8, 3), (0.0, 1.0), &modulation, &mask, &slope),
            plain((0.1, 0.6, 6, 2), (0.0, 1.0), &zero, &c(0.75), &zero),
            (
                Some((0.2, 0.8, 4, 2)),
                (0.0, 1.0),
                Box::new(move |k: Module| {
                    let inner = k.advanced_terrace(c(0.1), c(1.0), c(0.25), 0.3, 0.7, 3, 1);
                    let mask =
                        n_mask
                            .clone()
                            .advanced_terrace(c(0.2), c(0.5), c(0.5), -0.5, 0.5, 2, 2);
                    inner.advanced_terrace(n_mod.clone(), mask, n_slope.clone(), 0.2, 0.8, 4, 2)
                }),
            ),
            fixed(&perlin, &modulation, &mask, (0.2, 0.8, 8, 3)),
            fixed(&simplex, &zero, &c(0.5), (-0.25, 0.4, 5, 2)),
            fixed(&perlin, &modulation, &mask, (0.2, 0.8, i32::MAX, 3)),
        ];
        let expected: [(usize, u64); 30] = [
            (174, 0xfa422d308ce954a7),
            (174, 0x7b22940c065a2cef),
            (174, 0x72926e3d17b20a0b),
            (174, 0x72926e3d17b20a0b),
            (102, 0xb837e5a665ad42cd),
            (102, 0x6cda63365272a2e3),
            (102, 0x8c114d46a972a2e3),
            (120, 0xfd3edb55f0f74b33),
            (120, 0x4ca93714cf67c4f1),
            (120, 0x50c00f04aac5ccbd),
            (18, 0xda019ab27bf4255b),
            (18, 0x34d9d7966d9ce513),
            (18, 0xc8ededf1791a6bc5),
            (18, 0x79040cf03186942b),
            (18, 0x79040cf03186942b),
            (30, 0x65ace3f451975267),
            (102, 0xd64a3ec83aec2b57),
            (102, 0x68084fdfe965257d),
            (102, 0x67fe3584c962e515),
            (102, 0x6cda63365272a2e3),
            (102, 0xfacbfbc1557f1655),
            (102, 0x67d2e2bf018daa07),
            (102, 0x9778ce34929cbc4f),
            (102, 0x6cda63365272a2e3),
            (174, 0x4fbf33c70fe14629),
            (138, 0xddec5f3f5947c5a1),
            (102, 0x9df24ec662b0c9db),
            (12, 0x5994c32209cccc45),
            (12, 0xec4763a38352470d),
            (12, 0xdcc0b8891c095125),
        ];
        // blendMin, blendMax, and for the first two octaves with `n = steps * i` in 1..=64 every `k / n` and
        // `(k + 0.5) / n`.
        let edges = |(bmin, bmax, steps, octaves): (f32, f32, i32, i32)| {
            let mut out = vec![bmin, bmax];
            for i in 1..=octaves.min(2) {
                let n = steps.wrapping_mul(i);
                if !(1..=64).contains(&n) {
                    continue;
                }
                for k in 0..=n {
                    out.extend([k as f32 / n as f32, (k as f32 + 0.5) / n as f32]);
                }
            }
            out
        };
        let specials = [
            nan,
            inf,
            -inf,
            0.0,
            -0.0,
            0.5,
            1.0,
            -1.0,
            2.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            1.0f32.next_down(),
        ];
        for (index, ((args, bounds, build), expected)) in families.iter().zip(expected).enumerate()
        {
            let table = args.map_or_else(Vec::new, edges);
            let fold = seeded_edge_fold(&table, &specials, *bounds, build);
            assert_eq!(fold, expected, "family {index}");
        }
    }

    #[test]
    fn seeded_curves_match_reseeding_and_read_every_child() {
        // Over non-Perlin sources with finite, nonzero ranges, reseeding and seed-first sampling seed the same
        // sources and fix the same power-curve bounds, so they give the same bits. A MidPointCurve or SCurve cannot
        // be reseeded (the jar and Noise2D 2cb4843's Cereal both throw), but samples seed-first.
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let midpoint = src(SourceKind::Cubic, -91, 3);
        let gradient = src(SourceKind::Simplex2, i32::MIN, 2);
        let graphs = [
            source
                .clone()
                .curve(CurveFunction::Interpolation(Interpolation::Curve4)),
            source.clone().variable_curve(midpoint, gradient),
            source.clone().power_curve(0.8),
            source.clone().power_curve(2.5).power_curve(0.5),
        ];
        for (index, graph) in graphs.iter().enumerate() {
            for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                let reseeded = graph.with_seed(i64::from(seed));
                for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, 3.5), (1e30, 3.0)] {
                    assert_eq!(
                        canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                        canonical_bits([reseeded.value(x, y)]),
                        "graph {index}, seed {seed}, ({x}, {y})"
                    );
                }
            }
        }
        for function in [
            CurveFunction::Midpoint {
                midpoint: 0.4,
                steepness: 6.0,
            },
            CurveFunction::s_curve(0.5, -0.2),
        ] {
            let graph = source.clone().curve(function);
            assert!(graph.try_value_seed_first(i32::MIN, 1.0, 2.0).is_ok());
            assert!(std::panic::catch_unwind(|| graph.with_seed(5)).is_err());
        }
        // Curve and PowerCurve read only their source; VariableCurve reads all three children (Java has no early exit).
        let unsupported = unsupported_seeded_node(0.5);
        let c = Module::constant;
        let midpoint_curve = CurveFunction::Midpoint {
            midpoint: 0.5,
            steepness: 4.0,
        };
        assert!(
            unsupported
                .clone()
                .curve(midpoint_curve)
                .try_value_seed_first(1, 0.0, 0.0)
                .is_err()
        );
        assert!(
            unsupported
                .clone()
                .power_curve(0.8)
                .try_value_seed_first(1, 0.0, 0.0)
                .is_err()
        );
        for (s, m, g) in [
            (&unsupported, &c(0.5), &c(4.0)),
            (&c(0.5), &unsupported, &c(4.0)),
            (&c(0.5), &c(0.5), &unsupported),
        ] {
            let graph = s.clone().variable_curve(m.clone(), g.clone());
            assert!(graph.try_value_seed_first(1, 0.0, 0.0).is_err());
        }
        let graph = c(f32::NAN).variable_curve(c(f32::NAN), c(4.0));
        assert!(graph.try_value_seed_first(1, 0.0, 0.0).unwrap().is_nan());
    }

    #[test]
    fn runtime_seeded_curves_match_current_noise2d() {
        let source = |kind, seed, frequency, octaves, gain| {
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
        };
        let base = source(SourceKind::Perlin, 901, 1.0 / 96.0, 4, 0.5);
        let graphs = [
            base.clone().curve(CurveFunction::Midpoint {
                midpoint: 0.4,
                steepness: 6.0,
            }),
            base.clone().curve(CurveFunction::s_curve(0.5, -0.2)),
            base.clone().variable_curve(
                source(SourceKind::Simplex, 902, 1.0 / 80.0, 3, 0.5),
                source(SourceKind::Ridge, 903, 1.0 / 128.0, 2, 0.975),
            ),
            base.power_curve(0.8),
        ];
        let expected = [
            [0x01599d465de40a43; 4],
            [0xa6a665d0d63d0c48; 4],
            [
                0x5d4f1efc413f1011,
                0x46db897093016fff,
                0x2f4d070948218174,
                0x71b0b05f4e543d5c,
            ],
            [0xf87ba5b74029c866; 4],
        ];
        for (graph, hashes) in graphs.iter().zip(expected) {
            for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip(hashes) {
                let mut hash = 0xcbf29ce484222325u64;
                for (x, y) in [
                    (0.0, 0.0),
                    (0.125, -0.75),
                    (12.34, 56.78),
                    (-1234.5, 987.25),
                ] {
                    hash ^= u64::from(graph.try_value_seed_first(seed, x, y).unwrap().to_bits());
                    hash = hash.wrapping_mul(0x100000001b3);
                }
                assert_eq!(hash, expected, "seed {seed}");
            }
        }
    }

    /// `SeededCurveEdgeOracle` (`.local/terraforged/oracles/seed_first/run-graphs.sh`): Curve (MidPointCurve, SCurve,
    /// the three interpolations), VariableCurve and PowerCurve with a source fixed at, and one ulp beside, each
    /// midpoint, `NoiseUtil.exp`'s zero-base point, SCurve's zero-power point and PowerCurve's source bounds and
    /// midpoint (plus NaN, ±inf, ±0, subnormals and values outside 0..1); zero, negative, infinite and NaN curve
    /// parameters; NaN, infinite and seeded variable-curve controls; power curves over zero (`map_current`'s 1),
    /// infinite, inverted and NaN bounds; power curves of seeded sources; and nested curves.
    #[test]
    fn runtime_seeded_curve_edges_match_current_noise2d() {
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let simplex2 = src(SourceKind::Simplex2, 802, 3, 0.5, 0.0125);
        let cubic = src(SourceKind::Cubic, 803, 2, 0.975, 0.0078125);
        let perlin2 = src(SourceKind::Perlin2, 804, 2, 0.5, 0.015625);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let huge = src(SourceKind::Billow, -5, 3, 1e20, 1.0);
        let simplex = src(SourceKind::Simplex, i32::MAX, 3, 0.65, 0.125);
        let perlin = src(SourceKind::Perlin, 801, 4, 0.5, 1.0 / 96.0);
        let c = Module::constant;
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        type Family = (Vec<f32>, (f32, f32), Box<dyn Fn(Module) -> Module>);
        let unit = |table: &[f32], build: Box<dyn Fn(Module) -> Module>| -> Family {
            (table.to_vec(), (0.0, 1.0), build)
        };
        let mp = |midpoint: f32, steepness: f32, table: &[f32]| {
            unit(
                table,
                Box::new(move |k: Module| {
                    k.curve(CurveFunction::Midpoint {
                        midpoint,
                        steepness,
                    })
                }),
            )
        };
        let sc = |lower: f32, upper: f32, table: &[f32]| {
            unit(
                table,
                Box::new(move |k: Module| k.curve(CurveFunction::s_curve(lower, upper))),
            )
        };
        let it = |interpolation, table: &[f32]| {
            unit(
                table,
                Box::new(move |k: Module| k.curve(CurveFunction::Interpolation(interpolation))),
            )
        };
        let vc = |midpoint: &Module, gradient: &Module, table: &[f32]| {
            let (midpoint, gradient) = (midpoint.clone(), gradient.clone());
            unit(
                table,
                Box::new(move |k: Module| k.variable_curve(midpoint.clone(), gradient.clone())),
            )
        };
        let pc = |power: f32, min: f32, max: f32| -> Family {
            let table = vec![min, max, min + (max - min) / 2.0];
            (
                table,
                (min, max),
                Box::new(move |k: Module| k.power_curve(power)),
            )
        };
        let fixed = |module: Module| unit(&[], Box::new(move |_| module.clone()));
        let families: Vec<Family> = vec![
            mp(0.4, 6.0, &[0.4, 0.4 + 256.0 / 6.0]),
            mp(0.5, 0.0, &[0.5]),
            mp(0.5, -4.0, &[0.5, 0.5 - 64.0]),
            mp(nan, 4.0, &[]),
            mp(inf, 4.0, &[]),
            mp(0.5, inf, &[0.5]),
            mp(0.5, nan, &[0.5]),
            mp(-0.0, 1e30, &[0.0]),
            sc(0.5, -0.2, &[0.0, 1.0, 2.5]),
            sc(0.5, -0.8, &[0.0, 1.0]),
            sc(0.0, 1.0, &[0.0, 1.0]),
            sc(1.0, 0.0, &[0.0, 1.0]),
            sc(nan, 1.0, &[0.0, 1.0]),
            sc(2.0, nan, &[0.0, 1.0]),
            sc(-1.0, 0.5, &[0.0, 1.0, 2.0]),
            sc(inf, -1.0, &[0.0, 1.0]),
            sc(-0.0, -0.0, &[0.0, 1.0]),
            it(Interpolation::Linear, &[0.0, 1.0]),
            it(Interpolation::Curve3, &[0.0, 0.5, 1.0]),
            it(Interpolation::Curve4, &[0.0, 0.5, 1.0]),
            vc(&c(0.5), &c(4.0), &[0.5]),
            vc(&c(nan), &c(4.0), &[]),
            vc(&c(0.5), &c(inf), &[0.5]),
            vc(&c(-inf), &c(-0.0), &[]),
            vc(&simplex2, &cubic, &[0.5]),
            vc(&zero, &perlin2, &[]),
            pc(0.8, 0.0, 1.0),
            pc(2.0, -1.0, 1.0),
            pc(0.0, 0.0, 1.0),
            pc(-1.0, 0.0, 1.0),
            pc(nan, 0.0, 1.0),
            pc(inf, 0.0, 1.0),
            pc(0.5, 0.5, 0.5),
            pc(1.0, 0.0, inf),
            pc(1.0, -inf, inf),
            pc(3.0, 0.0, 1e30),
            pc(1.0, 1.0, 0.0),
            pc(0.8, 0.0, nan),
            fixed(perlin.power_curve(0.8)),
            fixed(zero.clone().power_curve(1.5)),
            fixed(huge.power_curve(0.8)),
            unit(
                &[0.0, 1.0, 0.5],
                Box::new(move |k: Module| {
                    k.power_curve(0.8)
                        .variable_curve(simplex2.clone(), c(4.0))
                        .curve(CurveFunction::s_curve(0.5, -0.2))
                }),
            ),
            unit(
                &[0.0, 1.0, 0.5],
                Box::new(|k: Module| k.power_curve(2.0).power_curve(0.5)),
            ),
            unit(
                &[0.5],
                Box::new(move |k: Module| simplex.clone().variable_curve(k, c(6.0))),
            ),
        ];
        let expected: [(usize, u64); 44] = [
            (20, 0xf6ae4e04c7d1cda9),
            (17, 0x60949791576c3e8d),
            (20, 0x67376dd144c70d01),
            (14, 0x2d2fae0b58f76195),
            (14, 0x87ea76ab2f776195),
            (17, 0x126239de28ec3e8d),
            (17, 0xfc713680a16c3e8d),
            (17, 0x729c84d8a8ec3e8d),
            (23, 0x23f342e836dd378d),
            (20, 0x0164fe84e4b107e9),
            (20, 0xfad57b4e0b004061),
            (20, 0x06cea9bc78314d3b),
            (20, 0x0b82b503e27bfac5),
            (20, 0x0b82b503e27bfac5),
            (23, 0x34af43cece66f40d),
            (20, 0x843382745dfbfac5),
            (20, 0x8889817d307bfac5),
            (20, 0xe9c4266b68b14d3b),
            (23, 0x7e20e5dcc47294bf),
            (23, 0xf1a5daa2ec02e26b),
            (17, 0x44cdcb3bf7d79aa1),
            (14, 0x2d2fae0b58f76195),
            (17, 0x126239de28ec3e8d),
            (14, 0x2d2fae0b58f76195),
            (17, 0x492fcb363f40cb9f),
            (14, 0x2e3c1049a39fadff),
            (23, 0xa0c475c94e4c1ca5),
            (23, 0x1264650d40e77d19),
            (23, 0x516d6ad16ff7443d),
            (23, 0xdc5f71aa214665b3),
            (23, 0xcba82590e577443d),
            (23, 0xb9a7ce2a42f7443d),
            (23, 0xb9a7ce2a42f7443d),
            (23, 0xcba82590e577443d),
            (23, 0xcba82590e577443d),
            (23, 0xcba82590e577443d),
            (23, 0xb9a7ce2a42f7443d),
            (23, 0xcba82590e577443d),
            (14, 0xc2063e51c2a007a1),
            (14, 0x2ee0893bd7f76195),
            (14, 0xcc8eb8ea84b2e58d),
            (23, 0x2f53f7d494a57b32),
            (23, 0xa7baeda75988a301),
            (17, 0xc1790e7b8af061c8),
        ];
        let specials = [
            nan,
            inf,
            -inf,
            0.0,
            -0.0,
            0.5,
            1.0,
            -1.0,
            2.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            1.0f32.next_down(),
            1e30,
            -1e30,
        ];
        for (index, ((table, bounds, build), expected)) in families.iter().zip(expected).enumerate()
        {
            let fold = seeded_edge_fold(table, &specials, *bounds, build);
            assert_eq!(fold, expected, "family {index}");
        }
    }

    #[test]
    fn runtime_seeded_transforms_match_current_noise2d() {
        let source = |kind, seed, frequency, octaves, gain| {
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
        };
        let base = source(SourceKind::Perlin, 1001, 1.0 / 96.0, 4, 0.5);
        let simplex = source(SourceKind::Simplex, 1002, 1.0 / 80.0, 3, 0.5);
        let ridge = source(SourceKind::Ridge, 1003, 1.0 / 128.0, 2, 0.975);
        let perlin2 = source(SourceKind::Perlin2, 1004, 1.0 / 64.0, 2, 0.5);
        let graphs = [
            base.clone().steps(
                simplex.clone(),
                Module::constant(0.2),
                ridge.clone(),
                CurveFunction::Interpolation(Interpolation::Linear),
            ),
            base.clone().frequency(simplex.clone(), ridge.clone()),
            base.clone()
                .gradient(simplex.clone(), ridge.clone(), perlin2),
            base.modulate(simplex, ridge),
        ];
        let expected = [
            [
                0x9f745b5614915957,
                0x6c683937178c790d,
                0x74a80d94932da15f,
                0xa18cd68140e13a4b,
            ],
            [
                0xd73e35f81bbfafdf,
                0xbb7d3fa76cc7677a,
                0xa9df7d00157a964b,
                0x808cb808658362f0,
            ],
            [
                0xc3d0d844aeb558ad,
                0x2277c180ca2f135f,
                0x03706f82c69daf2b,
                0x4f1d0987ae38d539,
            ],
            [
                0xbf826dcb75996b52,
                0x2639ab937c91bfef,
                0x14f548b29c1b99bb,
                0x961c25513e62a41f,
            ],
        ];
        for (graph, hashes) in graphs.iter().zip(expected) {
            for (seed, expected) in [0, 17, -91, i32::MAX].into_iter().zip(hashes) {
                let mut hash = 0xcbf29ce484222325u64;
                for (x, y) in [
                    (0.0, 0.0),
                    (0.125, -0.75),
                    (12.34, 56.78),
                    (-1234.5, 987.25),
                ] {
                    hash ^= u64::from(graph.try_value_seed_first(seed, x, y).unwrap().to_bits());
                    hash = hash.wrapping_mul(0x100000001b3);
                }
                assert_eq!(hash, expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn runtime_seeded_transform_edges_match_current_noise2d() {
        // `SeededTransformEdgeOracle.java` (`.local/terraforged/oracles/seed_first/`): Steps, Freq, Grad and Modulate
        // families around a fixed value at and one ulp beside every edge, plus the specials.
        let src = |kind, seed, octaves, gain, frequency| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        let simplex2 = src(SourceKind::Simplex2, 802, 3, 0.5, 0.0125);
        let cubic = src(SourceKind::Cubic, 803, 2, 0.975, 0.0078125);
        let perlin2 = src(SourceKind::Perlin2, 804, 2, 0.5, 0.015625);
        let zero = src(SourceKind::Simplex, 1337, 0, 0.5, 3.0);
        let simplex = src(SourceKind::Simplex, i32::MAX, 3, 0.65, 0.125);
        let c = Module::constant;
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        let lin = CurveFunction::Interpolation(Interpolation::Linear);
        let c3 = CurveFunction::Interpolation(Interpolation::Curve3);
        let c4 = CurveFunction::Interpolation(Interpolation::Curve4);
        type Family = (Vec<f32>, Box<dyn Fn(Module) -> Module>);
        let family = |table: &[f32], build: Box<dyn Fn(Module) -> Module>| -> Family {
            (table.to_vec(), build)
        };
        let st = |steps: Module, min: Module, max: Module, curve: CurveFunction, table: &[f32]| {
            family(
                table,
                Box::new(move |k: Module| k.steps(steps.clone(), min.clone(), max.clone(), curve)),
            )
        };
        let gr = |lower: Module, upper: Module, strength: Module, table: &[f32]| {
            family(
                table,
                Box::new(move |k: Module| {
                    k.gradient(lower.clone(), upper.clone(), strength.clone())
                }),
            )
        };
        let quarters = [0.0, 0.25, 0.5, 0.75, 1.0];
        let alphas = [
            0.9375, 0.8125, 0.6875, 0.5625, 0.4375, 0.3125, 0.1875, 0.0625,
        ];
        let families: Vec<Family> = vec![
            st(c(4.0), c(0.0), c(0.0), lin, &quarters),
            st(c(4.0), c(0.25), c(0.75), lin, &alphas),
            st(c(4.0), c(0.25), c(0.75), c3, &alphas),
            st(
                c(3.0),
                c(0.0),
                c(1.0),
                c4,
                &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            ),
            st(c(4.0), c(0.75), c(0.25), lin, &quarters),
            st(c(4.0), c(-inf), c(0.5), lin, &[0.5, 0.25]),
            st(c(4.0), c(0.0), c(inf), lin, &[0.25]),
            st(c(4.0), c(nan), c(0.5), lin, &[0.25]),
            st(c(4.0), c(0.0), c(nan), lin, &[]),
            st(c(0.0), c(0.0), c(0.0), lin, &[]),
            st(c(0.0), c(0.25), c(0.75), lin, &[]),
            st(c(nan), c(0.0), c(0.0), lin, &[]),
            st(c(inf), c(0.25), c(0.75), lin, &[]),
            st(c(-3.0), c(0.0), c(0.0), lin, &[1.0 / 3.0]),
            st(c(1e10), c(0.0), c(0.0), lin, &[2147483647.0 / 1e10]),
            st(c(2.5), c(0.25), c(0.75), lin, &[0.0, 0.4, 0.8]),
            st(perlin2.clone(), simplex2.clone(), cubic.clone(), c3, &[0.5]),
            family(&[], {
                let graph = simplex.clone().steps(c(5.0), c(0.1), c(0.9), c3);
                Box::new(move |_| graph.clone())
            }),
            family(&[], {
                let graph = zero.clone().steps(c(4.0), c(-inf), c(0.5), lin);
                Box::new(move |_| graph.clone())
            }),
            family(&[0.0, 1.0], {
                let source = simplex2.clone();
                Box::new(move |k| source.clone().frequency(k, c(1.0)))
            }),
            family(&[0.0], {
                let source = simplex2.clone();
                Box::new(move |k| source.clone().frequency(c(1.0), k))
            }),
            family(&[0.5], {
                let source = cubic.clone();
                Box::new(move |k: Module| source.clone().frequency(k.clone(), k))
            }),
            family(&[], {
                let graph = cubic.clone().frequency(simplex2.clone(), perlin2.clone());
                Box::new(move |_| graph.clone())
            }),
            family(&[1.0], {
                let source = simplex.clone().frequency(c(2.0), c(0.5));
                Box::new(move |k| source.clone().frequency(k, c(3.0)))
            }),
            gr(c(0.25), c(0.75), c(0.5), &[0.25, 0.75, 0.5]),
            gr(c(0.5), c(0.5), c(0.5), &[0.5]),
            gr(c(0.75), c(0.25), c(0.5), &[0.25, 0.75]),
            gr(c(0.0), c(1.0), c(1.0), &[0.0, 1.0]),
            gr(c(0.25), c(0.75), c(nan), &[0.25, 0.75]),
            gr(c(0.25), c(0.75), c(inf), &[0.25, 0.75]),
            gr(c(0.25), c(0.75), c(-inf), &[0.25, 0.75]),
            gr(c(nan), c(0.75), c(0.5), &[0.75]),
            gr(c(0.25), c(nan), c(0.5), &[0.25]),
            gr(c(-inf), c(inf), c(0.5), &[]),
            gr(c(0.0), c(1.0), c(2.0), &[0.0, 1.0]),
            gr(simplex2.clone(), cubic.clone(), perlin2.clone(), &[0.5]),
            family(&[], {
                let graph = simplex.clone().gradient(c(0.3), c(0.6), zero.clone());
                Box::new(move |_| graph.clone())
            }),
            family(&[0.0, 0.25, 0.5, 0.75, 1.0, -0.25], {
                let source = simplex2.clone();
                Box::new(move |k| source.clone().modulate(k, c(8.0)))
            }),
            family(&[0.0], {
                let source = simplex2.clone();
                Box::new(move |k| source.clone().modulate(c(0.125), k))
            }),
            family(&[0.0], {
                let source = simplex2.clone();
                Box::new(move |k| source.clone().modulate(c(0.0), k))
            }),
            family(&[0.5], {
                let source = cubic.clone();
                Box::new(move |k: Module| source.clone().modulate(k.clone(), k))
            }),
            family(&[], {
                let graph = cubic.clone().modulate(simplex2.clone(), perlin2.clone());
                Box::new(move |_| graph.clone())
            }),
            family(&[0.5], {
                let source = simplex.clone().frequency(c(2.0), c(0.5));
                Box::new(move |k: Module| {
                    source
                        .clone()
                        .modulate(k.clone(), c(16.0))
                        .steps(c(5.0), c(0.2), c(0.8), c3)
                        .gradient(c(0.2), c(0.8), k)
                })
            }),
        ];
        let expected: [(usize, u64); 43] = [
            (29, 0x799fc40fffee46ed),
            (38, 0xcbb54ff77ac56b47),
            (38, 0x7b0b51a240baa9dd),
            (26, 0x0a40ba6c4e0edfe1),
            (29, 0x799fc40fffee46ed),
            (20, 0x5d6d66b59c4d1805),
            (17, 0xdcc3b28a3a6c3e8d),
            (17, 0xfc713680a16c3e8d),
            (14, 0x2d2fae0b58f76195),
            (14, 0x2d2fae0b58f76195),
            (14, 0x2d2fae0b58f76195),
            (14, 0x2d2fae0b58f76195),
            (14, 0x0237d760cbf74efb),
            (17, 0x1232920fc761067d),
            (17, 0xcca7c5a31b1efd07),
            (23, 0x8bf0f6c0f6ac28c9),
            (17, 0x8127e985c99335e3),
            (14, 0xe142c10009dc8f19),
            (14, 0x2ee0893bd7f76195),
            (20, 0x2e024ba8020321e7),
            (17, 0x0968862af114f142),
            (17, 0x9122b9caf18b8b8d),
            (14, 0xe8b0142b368d2729),
            (17, 0xb4792bae9d7dc621),
            (23, 0xb1e3da127dc387a1),
            (17, 0x938fbc783b57c0d1),
            (20, 0x90d1d936eba1f249),
            (20, 0x57ad3c9b2ae3c319),
            (20, 0x423f2ee5e4dc314d),
            (20, 0xd5eaac050fdc314d),
            (20, 0x07a705c0a1dc314d),
            (17, 0xd33e593fdf891935),
            (17, 0x10831c111a65ccd1),
            (14, 0x2d2fae0b58f76195),
            (20, 0x2774c1094a77a5fb),
            (17, 0x515a53addc4370e9),
            (14, 0x7dca395465e4f1d5),
            (32, 0xa9c3b9d011b17667),
            (17, 0xc95bea9f29ae54f2),
            (17, 0x7ae8bd2763820402),
            (17, 0xf003984473628472),
            (14, 0xc49bf035e22bd601),
            (17, 0xe5b4ddb4f80e8720),
        ];
        let specials = [
            nan,
            inf,
            -inf,
            0.0,
            -0.0,
            0.5,
            1.0,
            -1.0,
            2.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            1.0f32.next_down(),
            1e30,
            -1e30,
        ];
        for (index, ((table, build), expected)) in families.iter().zip(expected).enumerate() {
            let fold = seeded_edge_fold(table, &specials, (0.0, 1.0), build);
            assert_eq!(fold, expected, "family {index}");
        }
    }

    #[test]
    fn seeded_transforms_match_reseeding_and_read_children_lazily() {
        // Over non-Perlin sources with finite, nonzero ranges, reseeding and seed-first sampling seed the same sources,
        // so they give the same bits.
        let src = |kind, seed, octaves| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            ))
        };
        let source = src(SourceKind::Simplex, i32::MAX, 2);
        let a = src(SourceKind::Cubic, -91, 3);
        let b = src(SourceKind::Simplex2, i32::MIN, 2);
        let c = Module::constant;
        let curve = CurveFunction::Interpolation(Interpolation::Curve3);
        let graphs = [
            source.clone().steps(c(4.0), c(0.25), c(0.75), curve),
            source
                .clone()
                .steps(a.clone().scale(c(8.0)), b.clone(), a.clone(), curve),
            source.clone().frequency(a.clone(), b.clone()),
            source.clone().gradient(a.clone(), b.clone(), a.clone()),
            source.clone().modulate(a.clone(), b.clone().scale(c(20.0))),
            source
                .clone()
                .modulate(b.clone(), c(4.0))
                .frequency(c(0.5), a.clone()),
        ];
        for (index, graph) in graphs.iter().enumerate() {
            for seed in [0, 1, -1, 17, i32::MAX, i32::MIN] {
                let reseeded = graph.with_seed(i64::from(seed));
                for (x, y) in [(0.0, 0.0), (12.34, -56.78), (-0.0, 3.5), (1e30, 3.0)] {
                    assert_eq!(
                        canonical_bits([graph.try_value_seed_first(seed, x, y).unwrap()]),
                        canonical_bits([reseeded.value(x, y)]),
                        "graph {index}, seed {seed}, ({x}, {y})"
                    );
                }
            }
        }
        // Steps, Freq and Modulate read every child; Grad reads strength and lower bound unless the source is above
        // its upper bound (Java returns the source sample there).
        let unsupported = unsupported_seeded_node(0.5);
        let u = || unsupported.clone();
        let failing = [
            u().steps(c(4.0), c(0.25), c(0.75), curve),
            c(0.5).steps(u(), c(0.25), c(0.75), curve),
            c(0.5).steps(c(4.0), u(), c(0.75), curve),
            c(0.5).steps(c(4.0), c(0.25), u(), curve),
            c(0.5)
                .steps(c(4.0), c(0.0), c(0.0), curve)
                .frequency(u(), c(1.0)),
            u().frequency(c(1.0), c(1.0)),
            c(0.5).frequency(c(1.0), u()),
            u().gradient(c(0.25), c(0.75), c(0.5)),
            c(0.5).gradient(c(0.25), u(), c(0.5)),
            c(0.5).gradient(u(), c(0.75), c(0.5)),
            c(0.5).gradient(c(0.25), c(0.75), u()),
            u().modulate(c(0.25), c(1.0)),
            c(0.5).modulate(u(), c(1.0)),
            c(0.5).modulate(c(0.25), u()),
        ];
        for (index, graph) in failing.iter().enumerate() {
            assert!(
                graph.try_value_seed_first(1, 0.0, 0.0).is_err(),
                "graph {index}"
            );
        }
        let above = c(0.9).gradient(u(), c(0.75), u());
        assert_eq!(above.try_value_seed_first(1, 0.0, 0.0), Ok(0.9));
        assert_eq!(above.value(0.0, 0.0), 0.9);
        // Freq reads its controls at the original point and samples the source at the scaled one; Modulate moves the
        // point by `sin`/`cos` of the direction turn times the strength.
        let probe = a.clone().frequency(c(3.0), c(-2.0));
        assert_eq!(
            probe.try_value_seed_first(7, 1.5, 2.5).unwrap().to_bits(),
            a.try_value_seed_first(7, 4.5, -5.0).unwrap().to_bits()
        );
        let probe = a.clone().modulate(c(0.25), c(10.0));
        let (dx, dy) = (sin(0.25 * PI2) * 10.0, cos(0.25 * PI2) * 10.0);
        assert_eq!(
            probe.try_value_seed_first(7, 1.5, 2.5).unwrap().to_bits(),
            a.try_value_seed_first(7, 1.5 + dx, 2.5 + dy)
                .unwrap()
                .to_bits()
        );
    }

    #[test]
    fn module_graph_matches_engine_0_3_0() {
        let base = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 8128,
                octaves: 4,
                gain: 0.5,
                frequency: 1.0 / 128.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let shaped = base
            .clone()
            .bias(Module::constant(0.125))
            .scale(Module::constant(0.75))
            .clamp(Module::constant(0.2), Module::constant(0.8))
            .map(Module::constant(-1.0), Module::constant(2.0))
            .abs()
            .invert();
        let boosted = base
            .clone()
            .alpha(Module::constant(0.4))
            .boost(2)
            .power(Module::constant(1.75));
        let gated = boosted.clone().threshold(Module::constant(0.55));
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                shaped.clone(),
                [0xbf800000, 0x40000000],
                [0x3fd40000, 0x3fa11d66, 0x3f5b6d74, 0x3e8b2530],
            ),
            (
                boosted,
                [0, 0x3f800000],
                [0x3f7f20fa, 0x3f7fad65, 0x3f7fe9b7, 0x3f7fff19],
            ),
            (gated, [0, 0x3f800000], [0x3f800000; 4]),
            (
                Module::add(vec![base.clone(), shaped.clone()]),
                [0xbf800000, 0x40400000],
                [0x400a0000, 0x3feeaf28, 0x3fc8fd77, 0x3f920b16],
            ),
            (
                Module::multiply(vec![base.clone(), shaped.clone()]),
                [0x80000000, 0x40000000],
                [0x3f540000, 0x3f43464f, 0x3f1c7914, 0x3e71e388],
            ),
            (
                Module::max(vec![base, shaped]),
                [0x40000000, 0x40000000],
                [0x3fd40000, 0x3fa11d66, 0x3f5b6d74, 0x3f5e8394],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
        let max_bounds = Module::max(vec![
            Module::source(Source::new(SourceKind::Simplex, SourceConfig::default())),
            Module::constant(0.5),
        ]);
        assert_eq!(
            [
                max_bounds.min_value().to_bits(),
                max_bounds.max_value().to_bits()
            ],
            [0x3f000000, 0x3f800000]
        );
    }

    #[test]
    fn module_graph_reads_cubic_bounds_like_engine_0_3_0() {
        // CubicNoise is the only source whose bounds are not Noise's 0/1, so the modifiers that read their source's
        // bounds (Map, Boost's map, Invert, Clamp's and Map's shortcuts, Scale, the combiners) see its raw octave bound.
        // Also covers Module.map's and Module.pow's shortcuts and nested abs/invert. Expected values come from Engine 0.3.0.
        let cubic = Source::new(
            SourceKind::Cubic,
            SourceConfig {
                seed: 31,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        );
        let simplex = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 8128,
                octaves: 4,
                gain: 0.5,
                frequency: 1.0 / 128.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let (low, high) = (cubic.min_value(), cubic.max_value());
        let cubic = Module::source(cubic);
        let c = Module::constant;
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (-3.5, 2.5),
        ];
        const RAW: [u32; 5] = [0x3f2dacf3, 0x3eb80f89, 0x3ec8031e, 0x3eefe935, 0x3f2a2ed2];
        const CUBIC: [u32; 2] = [0xbfa80000, 0x3fa80000];
        let cases = [
            (cubic.clone(), CUBIC, RAW),
            (
                cubic.clone().map(c(-1.0), c(2.0)),
                [0xbf800000, 0x40000000],
                [0x3fa33e44, 0x3f692d72, 0x3f724aee, 0x3f848bc6, 0x3fa13f54],
            ),
            (cubic.clone().map(c(low), c(high)), CUBIC, RAW),
            (
                cubic.clone().boost(2),
                [0, 0x3f800000],
                [0x3f7ee5d8, 0x3f79bec4, 0x3f7a93a0, 0x3f7c48eb, 0x3f7ecb49],
            ),
            (
                cubic.clone().invert(),
                CUBIC,
                [0x3f22530d, 0x3f73f83c, 0x3f6bfe71, 0x3f580b66, 0x3f25d12e],
            ),
            (
                cubic.clone().clamp(c(0.1), c(0.4)),
                [0x3dcccccd, 0x3ecccccd],
                [0x3ecccccd, 0x3eb80f89, 0x3ec8031e, 0x3ecccccd, 0x3ecccccd],
            ),
            (cubic.clone().clamp(c(low), c(high)), CUBIC, RAW),
            (
                cubic.clone().power(c(0.0)),
                [0x3f800000, 0x3f800000],
                [0x3f800000; 5],
            ),
            (cubic.clone().power(c(1.0)), CUBIC, RAW),
            (
                cubic.clone().power(c(1.5)),
                CUBIC,
                [0x3f0f0cd3, 0x3e5cb7a7, 0x3e7a05d8, 0x3ea439b0, 0x3f0ac1b5],
            ),
            (
                cubic
                    .clone()
                    .scale(c(0.5))
                    .bias(c(0.25))
                    .abs()
                    .threshold(c(0.3)),
                [0xbed00000, 0x3f680000],
                [0x3f800000; 5],
            ),
            (
                cubic.clone().alpha(c(0.4)),
                CUBIC,
                [0x3f5f11fc, 0x3f3e6982, 0x3f419a3a, 0x3f49950b, 0x3f5dac54],
            ),
            // Engine's `invert()` has no shortcut, so inverting twice does not give the source back.
            (
                cubic.clone().invert().invert(),
                CUBIC,
                [0x3f2dacf3, 0x3eb80f88, 0x3ec8031e, 0x3eefe934, 0x3f2a2ed2],
            ),
            (
                cubic.clone().bias(c(-1.0)).abs().abs(),
                [0xc0140000, 0x3ea00000],
                [0x3ea4a61a, 0x3f23f83c, 0x3f1bfe71, 0x3f080b66, 0x3eaba25c],
            ),
            (
                Module::add(vec![cubic.clone(), simplex.clone()]),
                [0xbfa80000, 0x40140000],
                [0x3f96d67a, 0x3f772b4a, 0x3f8d4784, 0x3fab3c17, 0x3f885a72],
            ),
            (
                Module::multiply(vec![cubic.clone(), simplex.clone()]),
                [0x80000000, 0x3fa80000],
                [0x3eadacf3, 0x3e5f15e3, 0x3e8ea0c0, 0x3ed0878b, 0x3e884f90],
            ),
            (
                Module::max(vec![cubic.clone(), simplex.clone()]),
                [0x3f800000, 0x3fa80000],
                [0x3f2dacf3, 0x3f1b2385, 0x3f368d7a, 0x3f5e8394, 0x3f2a2ed2],
            ),
            (
                Module::max(vec![simplex.clone(), cubic]),
                [0x3fa80000, 0x3fa80000],
                [0x3f2dacf3, 0x3f1b2385, 0x3f368d7a, 0x3f5e8394, 0x3f2a2ed2],
            ),
            (
                simplex.boost(1),
                [0, 0x3f800000],
                [0x3f3504f3, 0x3f5227a8, 0x3f6854cd, 0x3f7b5934, 0x3f13e7a9],
            ),
        ];
        for (index, (module, expected_bounds, expected_values)) in cases.into_iter().enumerate() {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds,
                "case {index}"
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values,
                "case {index}"
            );
        }
    }

    #[test]
    fn domain_warps_match_engine_0_3_0() {
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let warp = Domain::domain_warp(
            source(SourceKind::Perlin, 110, 64.0, 3, 0.5),
            source(SourceKind::Simplex, 220, 96.0, 2, 0.5),
            Module::constant(37.5),
        );
        let direction = Domain::direction(
            source(SourceKind::Simplex, 330, 72.0, 3, 0.5),
            Module::constant(11.25),
        );
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                Domain::direct(),
                [
                    [0, 0, 0, 0],
                    [0, 0, 0x41440000, 0xc2c58000],
                    [0, 0, 0xc5800400, 0x46000100],
                    [0, 0, 0x47f12060, 0xbe000000],
                ],
            ),
            (
                warp.clone(),
                [
                    [0, 0, 0, 0],
                    [0xc0bec34d, 0x4156e18b, 0x40c93cb3, 0xc2aaa3cf],
                    [0x3df4d364, 0xbf334ce1, 0xc580030b, 0x45fffc66],
                    [0xbd01b190, 0x41257e96, 0x47f1205c, 0x41237e96],
                ],
            ),
            (
                direction.clone(),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0xc12463b5, 0xc092a4cb, 0x3ffce258, 0xc2ceaa4d],
                    [0xc04b2f4b, 0xc12caf18, 0xc5801d66, 0x45ffaba8],
                    [0xc132b251, 0xbfacfb79, 0x47f11aca, 0xbfbcfb79],
                ],
            ),
            (
                warp.clone().add(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0xc181e2ae, 0x410d8f26, 0xc07f1570, 0xc2b3ce1b],
                    [0xc04388b0, 0xc137e3e6, 0xc5801c71, 0x45ffa60e],
                    [0xc1333403, 0x410fdf27, 0x47f11ac6, 0x410ddf27],
                ],
            ),
            (
                warp.clone().compound(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0x3f212ad7, 0xc133b7ca, 0x414e12ad, 0xc2dbf6f9],
                    [0xc07f83a3, 0xc12848b9, 0xc58023f0, 0x45ffaddc],
                    [0xc10cf689, 0xc0dfdeb2, 0x47f11bf8, 0xc0e3deb2],
                ],
            ),
            (
                warp.clone().then(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0x40dd620e, 0xc2c11ac8, 0x41995884, 0xc3434d64],
                    [0xc58022fb, 0x45ffa842, 0xc600137e, 0x467fd521],
                    [0x47f11bf4, 0x404e3cf4, 0x48711e2a, 0x40463cf4],
                ],
            ),
            (
                // Axes already reporting -0.5..0.5 are not mapped again; a constant axis maps to NaN, as in Engine.
                Domain::domain_warp(
                    source(SourceKind::Perlin, 110, 64.0, 3, 0.5)
                        .map(Module::constant(-0.5), Module::constant(0.5)),
                    Module::constant(0.5).map(Module::constant(-0.5), Module::constant(0.5)),
                    Module::constant(4.0),
                ),
                [
                    [0, 0xffc00000, 0, 0xffc00000],
                    [0xbf22c8ca, 0xffc00000, 0x4139d373, 0xffc00000],
                    [0x3c50eb00, 0xffc00000, 0xc58003e6, 0xffc00000],
                    [0xbb5d5800, 0xffc00000, 0x47f12060, 0xffc00000],
                ],
            ),
            (
                warp.clone()
                    .then(direction.clone())
                    .add(warp.clone().compound(direction.clone()))
                    .then(warp.clone()),
                [
                    [0xc1037ef0, 0xc1336565, 0xc1037ef0, 0xc1336565],
                    [0x41d1b9ba, 0xc34504d1, 0x4219dcdd, 0xc393e268],
                    [0xc6002703, 0x467fb917, 0xc6402903, 0x46bfdd0c],
                    [0x48711d62, 0xc149cc10, 0x48b4d6c9, 0xc14bcc10],
                ],
            ),
        ];
        for (domain, expected) in cases {
            assert_eq!(
                points.map(|(x, y)| [
                    domain.offset_x(x, y).to_bits(),
                    domain.offset_y(x, y).to_bits(),
                    domain.x(x, y).to_bits(),
                    domain.y(x, y).to_bits(),
                ]),
                expected
            );
        }
        let warped = source(SourceKind::Ridge, 440, 128.0, 4, 0.975).warp(warp.add(direction));
        assert_eq!(
            [warped.min_value().to_bits(), warped.max_value().to_bits()],
            [0, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| warped.value(x, y))),
            [0x3f4fea9c, 0x3f2d9330, 0x3f4b5329, 0x3f53bd2e]
        );
    }

    #[test]
    fn sin_lookup_matches_engine_0_3_0() {
        // Java: `hash = hash * 31 + floatToRawIntBits(SIN[i])` over `NoiseUtil.SIN`.
        let hash = sin_table().iter().fold(0_i64, |hash, value| {
            hash.wrapping_mul(31)
                .wrapping_add(i64::from(value.to_bits() as i32))
        });
        assert_eq!(hash, 4517474909631308587);
        let angles = [
            0.0,
            1e-7,
            -1e-7,
            f32::from_bits(0x3fc9_0ff9),
            -f32::from_bits(0x3fc9_0ff9),
            std::f32::consts::PI,
            -std::f32::consts::PI,
            PI2,
            -PI2,
            100.25,
            -100.25,
            1e9,
            -1e9,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            std::f32::consts::FRAC_PI_4,
            -2.3561945,
        ];
        assert_eq!(
            angles.map(|angle| [sin(angle).to_bits(), cos(angle).to_bits()]),
            [
                [0, 0x3f800000],
                [0, 0x3f800000],
                [0, 0x3f800000],
                [0x3f800000, 0x250d3132],
                [0xbf800000, 0],
                [0x250d3132, 0xbf800000],
                [0x250d3132, 0xbf7fffd4],
                [0, 0x3f800000],
                [0, 0x3f7fffd4],
                [0xbe8e398f, 0x3f75ecc2],
                [0x3e8efaad, 0x3f75d0c2],
                [0xba48f443, 0xba48f443],
                [0, 0],
                [0, 0],
                [0xba48f443, 0xba48f443],
                [0, 0],
                [0x3f35287b, 0x3f34e165],
                [0xbf35287d, 0xbf349a32],
            ]
        );
    }

    #[test]
    fn selectors_match_engine_0_3_0() {
        // Java `SelectorSweep` oracle (`.local/terraforged/oracles/selectors/`).
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        let lower = source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
            .scale(Module::constant(0.5))
            .bias(Module::constant(-0.25));
        let upper = source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
            .scale(Module::constant(1.5))
            .bias(Module::constant(0.1));
        let variable = source(SourceKind::Simplex, 504, 64.0, 2, 0.5);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cases = [
            (
                lower
                    .clone()
                    .base(upper.clone(), 0.3, Interpolation::Curve3),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3fcccccd, 0x3fa8e9ab, 0x3f964679, 0x3fc8a719, 0x3f46bead, 0x3f773e11,
                    0x3fafdf2f, 0x3f73d5fa,
                ],
            ),
            (
                control.clone().blend(
                    lower.clone(),
                    upper.clone(),
                    0.45,
                    0.35,
                    Interpolation::Curve4,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3f9a5848, 0x3f2114d2, 0x3f6a1af9, 0x3fc709c5, 0x3e058e80, 0x3f772288,
                    0x3fa8c9bc, 0x3c912b30,
                ],
            ),
            (
                control.clone().variable_blend(
                    variable,
                    lower.clone(),
                    upper.clone(),
                    0.55,
                    0.05,
                    0.2,
                    Interpolation::Curve3,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3ed46297, 0xbdd54186, 0x3e5881e1, 0x3f876c7b, 0x3c197ec0, 0x3f464b90,
                    0x3f4c3d46, 0xbdf59262,
                ],
            ),
            (
                control.clone().select(
                    lower.clone(),
                    upper.clone(),
                    0.3,
                    0.7,
                    0.12,
                    Interpolation::Curve4,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3fcccccd, 0x3fa8e9ab, 0x3f964679, 0x3fc868e9, 0x3f3f186f, 0x3f728147,
                    0x3fafdf2f, 0x3f626264,
                ],
            ),
            (
                control
                    .clone()
                    .select(lower, upper, 0.3, 0.7, 0.0, Interpolation::Linear),
                [0xbe800000, 0x3fcccccd],
                [
                    0, 0xbe04fbbe, 0xbda942b6, 0xbe166d16, 0x3c197ec0, 0x3cf473f0, 0x3dcc8d18,
                    0xbdf59262,
                ],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
    }

    #[test]
    fn selector_edges_match_engine_0_3_0() {
        // Java `Selectors` oracle (`.local/terraforged/oracles/selectors/`): constant controls on and between the
        // edges, reversed bounds, zero and negative falloffs. NaN prints as `floatToIntBits` (0x7fc00000).
        const EXPECTED: &str = "\
sel0 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3dcccccd 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
base 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3e99999a 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
selinv 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3e99999a 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
base 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3eb851ec 0xbe800000 0x3fcccccd | 0x3faccccd 0x3f8bec3e 0x3f7a489d 0x3f2810d1\n\
selinv 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3eb851ec 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3eb851ec 0xbe800000 0x3fcccccd | 0x3e75c292 0xbdd3fd70 0xbda942b6 0x3c197ec0\n\
base 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
selinv 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blend 0x3f000000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f000000 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
base 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f457eaf\n\
base0 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f333333 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f333333 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a33 0x3f0bb24e 0x3ec924a8\n\
selinv 0x3f333333 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
selneg 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blend 0x3f333333 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f365a6b\n\
base0 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0inv 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3f51eb85 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f51eb85\n\
base0 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f51eb85\n\
baseneg 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0inv 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3f666666 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f958832 0x3f666666\n\
base0 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f666666\n\
baseneg 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x7fc00000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x7fc00000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
base 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blendwide 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f0760c8 0x3f0ea1c7 0x3e9978a0\n\
blendzero 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3c197ec0\n\
basectl 0xbe800000 0x3f800000 | 0x3eb98c02 0x3e32940c 0x3ebc62d2 0x3da4e1f8";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let mut print = |name: String, module: Module| {
            let mut line = format!(
                "{name} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for (x, y) in [
                (0.0, 0.0),
                (12.25, -98.75),
                (-4096.5, 8192.25),
                (64.0, 64.0),
            ] {
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        };
        let lower = source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
            .scale(Module::constant(0.5))
            .bias(Module::constant(-0.25));
        let upper = source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
            .scale(Module::constant(1.5))
            .bias(Module::constant(0.1));
        let variable = source(SourceKind::Simplex, 504, 64.0, 2, 0.5);
        let (l, u) = (|| lower.clone(), || upper.clone());
        for c in [0.1, 0.3, 0.36, 0.5, 0.7, 0.82, 0.9, f32::NAN] {
            let k = || Module::constant(c);
            let t = hex(c);
            print(
                format!("sel0 {t}"),
                k().select(l(), u(), 0.3, 0.7, 0.0, Interpolation::Linear),
            );
            print(
                format!("sel0inv {t}"),
                k().select(l(), u(), 0.7, 0.3, 0.0, Interpolation::Linear),
            );
            print(
                format!("sel {t}"),
                k().select(l(), u(), 0.3, 0.7, 0.12, Interpolation::Curve3),
            );
            print(
                format!("selinv {t}"),
                k().select(l(), u(), 0.7, 0.3, 0.12, Interpolation::Curve4),
            );
            print(
                format!("selneg {t}"),
                k().select(l(), u(), 0.3, 0.7, -0.12, Interpolation::Linear),
            );
            print(
                format!("blend {t}"),
                k().blend(l(), u(), 0.5, 0.2, Interpolation::Curve3),
            );
            let var = k().variable_blend(
                variable.clone(),
                l(),
                u(),
                0.5,
                0.05,
                0.3,
                Interpolation::Linear,
            );
            print(format!("var {t}"), var);
            print(
                format!("base {t}"),
                k().base(u(), 0.3, Interpolation::Curve3),
            );
            print(
                format!("base0 {t}"),
                k().base(u(), 0.0, Interpolation::Curve3),
            );
            print(
                format!("baseneg {t}"),
                k().base(u(), -0.2, Interpolation::Linear),
            );
        }
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        print(
            "blendwide".into(),
            control
                .clone()
                .blend(l(), u(), 0.45, 5.0, Interpolation::Linear),
        );
        print(
            "blendzero".into(),
            control
                .clone()
                .blend(l(), u(), 0.45, 0.0, Interpolation::Linear),
        );
        print(
            "basectl".into(),
            l().base(control, 0.4, Interpolation::Curve4),
        );
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    #[test]
    fn multi_blend_matches_engine_0_3_0() {
        let sources = || {
            vec![
                Module::constant(-1.0),
                Module::constant(0.25),
                Module::constant(2.0),
            ]
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let control = Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed: 501,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 80.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let module = control.multi_blend(0.4, Interpolation::Curve3, sources());
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0xbf800000, 0x40000000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3e800000, 0x3e800000, 0x3e800000, 0x3fb9d3fa, 0x3e800000, 0x3ffd47bb, 0x3ece61c4,
                0x3e800000,
            ]
        );

        let selectors = [
            0.0, 0.21666665, 0.21666667, 0.25, 0.2833333, 0.28333336, 0.5, 0.55, 0.5833333,
            0.6166666, 0.6166667, 0.75, 1.0,
        ];
        assert_eq!(
            selectors.map(|selector| {
                Module::constant(selector)
                    .multi_blend(0.4, Interpolation::Curve3, sources())
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [
                0xbf800000, 0xbf800000, 0xbf800000, 0xbec00004, 0x3e800000, 0x3e800000, 0x3e800000,
                0x3e800000, 0x3f8ffff4, 0x40000000, 0x40000000, 0x40000000, 0x40000000,
            ]
        );
        assert_eq!(
            [0.49, 0.5, 0.51].map(|selector| {
                Module::constant(selector)
                    .multi_blend(0.0, Interpolation::Curve3, sources())
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [0x3e800000; 3]
        );
    }

    #[test]
    fn multi_blend_edges_match_engine_0_3_0() {
        // Java `MultiBlends` oracle (`.local/terraforged/oracles/multiblend/`): constant controls across 0..1, -0.0
        // and NaN, a blend above 1, a negative blend, a -0.0 source and noise controls. NaN prints as
        // `floatToIntBits` (0x7fc00000).
        const EXPECTED: &str = "\
n2 b0x3fc00000 0x00000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3d4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3e4ccccd 0xbe800000 0x3fcccccd | 0x3d5a7412 0xbda703ac 0xbd26f187 0x3d0f1788\n\
n2 b0x3fc00000 0x3e800000 0xbe800000 0x3fcccccd | 0x3e888889 0x3de4cb8c 0x3e01d780 0x3e0c7db8\n\
n2 b0x3fc00000 0x3e99999a 0xbe800000 0x3fcccccd | 0x3ef5c292 0x3e9c26b3 0x3e96b5b2 0x3e753591\n\
n2 b0x3fc00000 0x3eb33333 0xbe800000 0x3fcccccd | 0x3f317e4b 0x3eff1a7f 0x3eec7fa0 0x3eaef6b2\n\
n2 b0x3fc00000 0x3ecccccd 0xbe800000 0x3fcccccd | 0x3f681b4f 0x3f310728 0x3f2124c9 0x3ee3529d\n\
n2 b0x3fc00000 0x3ee66666 0xbe800000 0x3fcccccd | 0x3f8f5c29 0x3f62810e 0x3f4c09c2 0x3f0bd744\n\
n2 b0x3fc00000 0x3f000000 0xbe800000 0x3fcccccd | 0x3faaaaab 0x3f89fd7a 0x3f76eeb9 0x3f26053a\n\
n2 b0x3fc00000 0x3f0ccccd 0xbe800000 0x3fcccccd | 0x3fc5f92d 0x3fa2ba6e 0x3f90e9da 0x3f403330\n\
n2 b0x3fc00000 0x3f19999a 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f266666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f400000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f4ccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f733333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f800000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x80000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x00000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3d4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3e4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3e800000 0xbe800000 0x3fcccccd | 0x3f4cccca 0x3f184a30 0x3f0bb24a 0x3ec924a5\n\
n3 b0x3ecccccd 0x3e99999a 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3eb33333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3ecccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3ee66666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f0ccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f19999a 0xbe800000 0x3fcccccd | 0x3f2bfffe 0x3f2db6f1 0x3f1bef33 0x3f0b0dca\n\
n3 b0x3ecccccd 0x3f266666 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f333333 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f400000 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f4ccccd 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f666666 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f733333 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f800000 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x80000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x00000000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3d4ccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3dcccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3e4ccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3e800000 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3e99999a 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3eb33333 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3ecccccd 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3ee66666 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f000000 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f0ccccd 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f19999a 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f266666 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f333333 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f400000 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f4ccccd 0xbe800000 0x40600000 | 0x00000000 0x00000000 0x00000000 0x00000000\n\
n5 b0xbe99999a 0x3f666666 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x3f733333 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x3f800000 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x80000000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x7fc00000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
ctl3 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
ctl5 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f0a143e";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let mut print = |name: String, module: Module| {
            let mut line = format!(
                "{name} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for (x, y) in [
                (0.0, 0.0),
                (12.25, -98.75),
                (-4096.5, 8192.25),
                (64.0, 64.0),
            ] {
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        };
        let sources = [
            source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
                .scale(Module::constant(0.5))
                .bias(Module::constant(-0.25)),
            source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
                .scale(Module::constant(1.5))
                .bias(Module::constant(0.1)),
            source(SourceKind::Perlin, 504, 64.0, 2, 0.5),
            Module::new_constant(-0.0),
            Module::constant(3.5),
        ];
        let controls = [
            0.0,
            0.05,
            0.1,
            0.2,
            0.25,
            0.3,
            0.35,
            0.4,
            0.45,
            0.5,
            0.55,
            0.6,
            0.65,
            0.7,
            0.75,
            0.8,
            0.9,
            0.95,
            1.0,
            -0.0,
            f32::NAN,
        ];
        for (count, blend, interpolation) in [
            (2, 1.5, Interpolation::Linear),
            (3, 0.4, Interpolation::Curve3),
            (5, -0.3, Interpolation::Curve3),
        ] {
            for control in controls {
                print(
                    format!("n{count} b{} {}", hex(blend), hex(control)),
                    Module::new_constant(control).multi_blend(
                        blend,
                        interpolation,
                        sources[..count].to_vec(),
                    ),
                );
            }
        }
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        print(
            "ctl3".into(),
            control
                .clone()
                .multi_blend(0.4, Interpolation::Curve3, sources[..3].to_vec()),
        );
        print(
            "ctl5".into(),
            control.multi_blend(0.8, Interpolation::Linear, sources.to_vec()),
        );
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());

        // Engine's `Combiner` bounds without sources.
        let empty = Module::constant(0.5).multi_blend(0.4, Interpolation::Linear, Vec::new());
        assert_eq!([empty.min_value(), empty.max_value()], [0.0, 0.0]);
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn multi_blend_panics_where_engine_throws() {
        // Engine: `nodes[index - 1]` with index 0 throws `ArrayIndexOutOfBoundsException`.
        Module::constant(-0.05)
            .multi_blend(0.4, Interpolation::Linear, vec![Module::constant(1.0); 3])
            .value(0.0, 0.0);
    }

    #[test]
    fn terrace_matches_engine_0_3_0() {
        // Java `Terraces` oracle (`.local/terraforged/oracles/terrace/`): noise terraces (one over a source whose range
        // is not 0..1) and clamped constants across 0..1 and -0.0 for 1, 2 and 7 steps, zero, wide and negative blend
        // ranges and constant ramps, cliffs and ramp heights, then a zero-range source. NaN prints as
        // `floatToIntBits` (0x7fc00000).
        const EXPECTED: &str = "\
noise0 0x00000000 0x3f800000 | 0x3f29b38c 0x3f000000 0x3f2aaaab 0x3f17c0fe 0x3f2aaaab 0x3f555556 0x3eb1ef8a 0x3f2aaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f000000 0x3f000000 0x3f000000 0x3f1eeee3 0x3ee13779 0x3f4ac589 0x3f2aaaab 0x3f156778 0x3f2aaaab 0x3f000000 0x3f2aaaab 0x3f000000 0x3f0e71be\n\
noise1 0xbe4ccccd 0x3fa66666 | 0x3f900000 0x3f400000 0x3f900000 0x3f400000 0x3f900000 0x3fc00000 0x3ec00000 0x3f900000 0x3f400000 0x3f900000 0x3f900000 0x3ec00000 0x3ec55480 0x3f400000 0x3f900000 0x3ec00000 0x3fc00000 0x3f900000 0x3f900000 0x3f900000 0x3f400000 0x3f900000 0x3f400000 0x3f400000\n\
noise2 0x00000000 0x3f800000 | 0x3f0a8d53 0x3f000000 0x3f0d758c 0x3f000000 0x3f350581 0x3f800000 0x3f000000 0x3f368896 0x3f000000 0x3f497e4e 0x3f314054 0x3f000000 0x3f000000 0x3f000000 0x3f01df0c 0x3f000000 0x3f800000 0x3f128b39 0x3f000000 0x3f02d20c 0x3f000000 0x3f1a1cb7 0x3f000000 0x3f000000\n\
s1 b0x00000000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x00000000 r0x3f19999a c0x3e99999a h0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x00000000 r0x40000000 c0xbf800000 h0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 r0x3f800000 c0x3f800000 h0x3f000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 r0x3f19999a c0x3e99999a h0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 r0x40000000 c0xbf800000 h0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 r0x3f19999a c0x3e99999a h0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 r0x40000000 c0xbf800000 h0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a r0x3f800000 c0x3f800000 h0x3f000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a r0x3f19999a c0x3e99999a h0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a r0x40000000 c0xbf800000 h0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s2 b0x00000000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3e147ae4 0x3e8f5c2b 0x3ea9930c 0x3ecf5c27 0x3ee38e3a 0x3f051eb8 0x3f200000 0x3f3851ec 0x3f4e147c 0x3f6147ae 0x3f71eb85 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x00000000 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3eaaaaad 0x3f2aaaad 0x3f800002 0x3f9c71c7 0x3f9c71c8 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x00000000 r0x40000000 c0xbf800000 h0x3f800000 | 0x3f000000 0x3f000000 0x3f000000 0x3f0ccccd 0x3f19999a 0x3f1eb852 0x3f266666 0x3f2aaaab 0x3f333333 0x3f400000 0x3f4ccccd 0x3f59999a 0x3f666666 0x3f733333 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3eb33333 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3e603078 0x3ed4122d 0x3ef8ab1c 0x3f15f6eb 0x3f2342d3 0x3f3bd596 0x3f5ba51e 0x3f75657e 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3eb33333 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e520d3a 0x3ec08c15 0x3f37cb7c 0x3f96b8d0 0x3f9867ad 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3eb33333 r0x40000000 c0xbf800000 h0x3f800000 | 0x3f000000 0x3f000000 0x3f000000 0x3f13b13b 0x3f276276 0x3f2f42f4 0x3f3b13b2 0x3f41a41b 0x3f4ec4ec 0x3f627628 0x3f762763 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3fc00000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3fc00000 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0x3fc00000 r0x40000000 c0xbf800000 h0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0xbe99999a r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3de63faa 0x3e603078 0x3e850f35 0x3ea398fc 0x3eb41790 0x3ed4122d 0x3f00c1e6 0x3f15f6eb 0x3f29a825 0x3f3bd596 0x3f4c7f3c 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0xbe99999a r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e520d3a 0x3eec4eca 0x3f37cb7c 0x3f796f92 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s2 b0xbe99999a r0x40000000 c0xbf800000 h0x3f800000 | 0x3f000000 0x3f000000 0x3f000000 0x3f09d89e 0x3f13b13b 0x3f17a17a 0x3f1d89d8 0x3f20d20d 0x3f276276 0x3f313b14 0x3f3b13b2 0x3f44ec4f 0x3f4ec4ec 0x3f589d89 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x00000000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3df5c290 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaad 0x3e81b4e9 0x3eaaaaab 0x3eaaaaab 0x3ec28f5b 0x3ef5c28f 0x3f000000 0x3f000001 0x3f1eb853 0x3f2aaaab 0x3f40da74 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x00000000 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x3de38e3c 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaaf 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f097b42 0x3f000000 0x3f000000 0x3f1c71cb 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x00000000 r0x40000000 c0xbf800000 h0x3f800000 | 0x3daaaaab 0x3daaaaab 0x3daaaaab 0x3e088889 0x3e2aaaab 0x3e2aaaab 0x3e6eeef0 0x3e800000 0x3e911112 0x3eaaaaab 0x3ec44445 0x3eddddde 0x3ef77777 0x3f088889 0x3f155556 0x3f222223 0x3f2eeeef 0x3f488889 0x3f622222 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3eb33333 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3e2398ff 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaad 0x3e93f1de 0x3eaaaaab 0x3eaaaaab 0x3ece0305 0x3f000000 0x3f000000 0x3f000000 0x3f28e640 0x3f2aaaab 0x3f49f8ed 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3eb33333 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x3e4b34e7 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e92992c 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f000000 0x3f000000 0x3f32cd3a 0x3f2aaaab 0x3f494c90 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3eb33333 r0x40000000 c0xbf800000 h0x3f800000 | 0x3daaaaab 0x3daaaaab 0x3daaaaab 0x3e241a42 0x3e2aaaab 0x3e2aaaab 0x3e65be5d 0x3e800000 0x3e9a41a4 0x3eaaaaab 0x3ebb13b2 0x3ee27626 0x3f000000 0x3f01a41a 0x3f155556 0x3f290691 0x3f2aaaab 0x3f4d20d2 0x3f5d89d9 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3fc00000 r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f000000 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3fc00000 r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f000000 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0x3fc00000 r0x40000000 c0xbf800000 h0x3f800000 | 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f000000 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0xbe99999a r0x3f800000 c0x3f800000 h0x3f000000 | 0x00000000 0x00000000 0x00000000 0x3dc7f3e2 0x3e2398ff 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e715b65 0x3e9e8c5f 0x3eaaaaab 0x3ebd595f 0x3ee94732 0x3f000000 0x3f000000 0x3f18fe7d 0x3f2aaaab 0x3f3c56d9 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0xbe99999a r0x3f19999a c0x3e99999a h0x40000000 | 0x00000000 0x00000000 0x00000000 0x3d0c08c0 0x3e4b34e7 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eb9d08a 0x3eaaaaab 0x3eaaaaab 0x3ee7ee81 0x3f000000 0x3f000000 0x3f08c08d 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
s7 b0xbe99999a r0x40000000 c0xbf800000 h0x3f800000 | 0x3daaaaab 0x3daaaaab 0x3daaaaab 0x3df96f97 0x3e241a42 0x3e2aaaab 0x3e72df2e 0x3e800000 0x3e8d20d2 0x3ea0d20e 0x3ec83484 0x3edbe5be 0x3eef96fa 0x3f0b7cb8 0x3f155556 0x3f1f2df3 0x3f32df2e 0x3f469069 0x3f641a42 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000\n\
flat 0x00000000";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let terrain = source(SourceKind::Perlin, 601, 96.0, 4, 0.5);
        let ramp = source(SourceKind::Simplex, 602, 72.0, 3, 0.5);
        let cliff = source(SourceKind::Ridge, 603, 128.0, 3, 0.975);
        let height = source(SourceKind::Perlin, 604, 80.0, 2, 0.5);
        let terrace = |source: &Module, steps, blend| {
            source
                .clone()
                .terrace(ramp.clone(), cliff.clone(), height.clone(), steps, blend)
        };
        let shifted = terrain
            .clone()
            .scale(Module::constant(1.5))
            .bias(Module::constant(-0.2));
        for (index, module) in [
            terrace(&terrain, 7, 0.35),
            terrace(&shifted, 5, 0.6),
            terrace(&terrain, 3, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut line = format!(
                "noise{index} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for i in 0..24 {
                let (x, y) = (-1500.0 + i as f32 * 131.37, 900.0 - i as f32 * 77.21);
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        }
        let values = [
            -0.05, -0.0, 0.0, 0.05, 0.1, 0.12, 0.15, 0.16666667, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45,
            0.5, 0.55, 0.6, 0.7, 0.8, 0.9, 0.95, 0.999999, 1.0, 1.05,
        ];
        for steps in [1, 2, 7] {
            for blend in [0.0, 0.35, 1.5, -0.3] {
                for [ramp, cliff, height] in [[1.0, 1.0, 0.5], [0.6, 0.3, 2.0], [2.0, -1.0, 1.0]] {
                    let mut line = format!(
                        "s{steps} b{} r{} c{} h{} |",
                        hex(blend),
                        hex(ramp),
                        hex(cliff),
                        hex(height)
                    );
                    for value in values {
                        let module = Module::new_constant(value)
                            .clamp(Module::constant(0.0), Module::constant(1.0))
                            .terrace(
                                Module::constant(ramp),
                                Module::constant(cliff),
                                Module::constant(height),
                                steps,
                                blend,
                            );
                        line += &format!(" {}", hex(module.value(0.0, 0.0)));
                    }
                    lines.push(line);
                }
            }
        }
        let flat = Module::constant(0.5).terrace(
            Module::constant(1.0),
            Module::constant(1.0),
            Module::constant(0.5),
            7,
            0.35,
        );
        lines.push(format!("flat {}", hex(flat.value(0.0, 0.0))));
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn terrace_panics_on_nan_where_engine_throws() {
        // Engine: `NoiseUtil.floor(NaN)` is -1, and `steps[-1]` throws `ArrayIndexOutOfBoundsException`.
        Module::constant(f32::NAN)
            .clamp(Module::constant(0.0), Module::constant(1.0))
            .terrace(
                Module::constant(1.0),
                Module::constant(1.0),
                Module::constant(0.5),
                7,
                0.35,
            )
            .value(0.0, 0.0);
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn terrace_without_steps_panics_where_engine_throws() {
        // Engine builds a terrace with no steps but throws when it samples `steps[0]`.
        Module::constant(0.5)
            .clamp(Module::constant(0.0), Module::constant(1.0))
            .terrace(
                Module::constant(1.0),
                Module::constant(1.0),
                Module::constant(0.5),
                0,
                0.35,
            )
            .value(0.0, 0.0);
    }

    #[test]
    fn constant_shares_source_zero_for_negative_zero() {
        // Java `Constants` oracle (`.local/terraforged/oracles/constant/`): `Source.constant(-0.0)` returns the shared
        // `ZERO` (+0.0), so `Source.constant(-0.0).scale(2)` samples +0.0, and `new Constant(-0.0f)` scaled by
        // `Source.constant(-0.0)` samples -0.0.
        let zero = Module::constant(-0.0);
        assert_eq!(
            [zero.min_value(), zero.max_value(), zero.value(3.0, 4.0)].map(f32::to_bits),
            [0; 3]
        );
        assert_eq!(
            zero.scale(Module::constant(2.0)).value(0.0, 0.0).to_bits(),
            0x0000_0000
        );
        assert_eq!(
            Module::new_constant(-0.0)
                .scale(Module::constant(-0.0))
                .value(0.0, 0.0)
                .to_bits(),
            0x8000_0000
        );
        assert_eq!(
            [0.5, 1.0, -1.0, 0.25].map(|value| Module::constant(value).value(0.0, 0.0).to_bits()),
            [0x3f00_0000, 0x3f80_0000, 0xbf80_0000, 0x3e80_0000]
        );
        assert!(Module::constant(f32::NAN).value(0.0, 0.0).is_nan());
    }

    #[test]
    fn legacy_terrace_matches_engine_0_3_0() {
        // Java `LegacyTerraces` oracle (`.local/terraforged/oracles/legacyterrace/`): noise legacy terraces (one over a
        // source whose range does not start at 0, one with a blend range above 1), then constant samples swept over
        // step counts, blend ranges and curves (built with `legacyTerrace(double, double, ...)`), an unclamped
        // zero-range source and a NaN sample (which rounds to step 0 rather than throwing).
        const EXPECTED: &str = "\
noise0 0x00000000 0x3f800000 | 0x3f2aaaab 0x3f07908d 0x3f260e7a 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f000000 0x3f000000 0x3f2aaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f000000 0x3efa81c9 0x3eb0e272 0x3f2aaaab 0x3eaaaaab 0x3f000000 0x3eaaaaab 0x3f2aaaab 0x3f2aaaab 0x3f2aaaab 0x3f1d3ab0\n\
noise1 0xbe4ccccd 0x3fa66666 | 0x3f82a6ac 0x3fa94d0e 0x3f99a170 0x3f31facc 0x3f856972 0x3f614caa 0x3f2f6cd0 0x3f39a118 0x3f68632a 0x3ec00000 0x3ec00000 0x3f0de122 0x3f7bd91e 0x3f0c942a 0x3f18f446 0x3f1bc71e 0x3f810af7 0x3ec00000 0x3f0aaaaf 0x3ec00000 0x3f5377a5 0x3f7a265a 0x3f648384 0x3fac7f16\n\
noise2 0x00000000 0x3f800000 | 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000\n\
noise3 0x00000000 0x3f800000 | 0x3f223329 0x3f2093e2 0x3f25bd32 0x3f15cd86 0x3f22301c 0x3f2902f4 0x3f1a3893 0x3f233b3c 0x3f29f172 0x3ea9b6cc 0x3ea3d567 0x3eda09ae 0x3f28ddd4 0x3ed300ac 0x3ec2607a 0x3eaa5ea1 0x3f23c025 0x3ea6c86d 0x3ed67a5b 0x3ea96ca2 0x3f24c8e4 0x3f28e380 0x3f24a7a3 0x3f1a5ed1\n\
s1 b0x00000000 l0x3e800000 u0x3f400000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x00000000 l0x3f800000 u0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x00000000 l0x40000000 u0xbf800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 l0x3e800000 u0x3f400000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 l0x3f800000 u0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3eb33333 l0x40000000 u0xbf800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 l0x3e800000 u0x3f400000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 l0x3f800000 u0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0x3fc00000 l0x40000000 u0xbf800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a l0x3e800000 u0x3f400000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a l0x3f800000 u0x3f800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 b0xbe99999a l0x40000000 u0xbf800000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s2 b0x00000000 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x00000000 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x00000000 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x3eb33333 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3cf1d1bf 0x3f200000 0x3f695457 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x3eb33333 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3df1d1bf 0x3f000000 0x3f61c5c9 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x3eb33333 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e71d1bf 0x3fc00000 0x3f8f1d1c 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0x3fc00000 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3d84bda2 0x3d9c06fa 0x3dcd1a78 0x3f200000 0x3f331613 0x3f457d62 0x3f56872b 0x3f6584aa 0x3f6c0832 0x3f71c716 0x3f71c71c 0x3f71c71c 0x00000000\n\
s2 b0x3fc00000 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e84bda2 0x3e9c06fa 0x3ecd1a78 0x3f000000 0x3f1972c4 0x3f31fc82 0x3f48b439 0x3f5cb0e2 0x3f656042 0x3f6d0972 0x3f6d097b 0x3f6d097b 0x00000000\n\
s2 b0x3fc00000 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f04bda2 0x3f1c06fa 0x3f4d1a78 0x3fc00000 0x3fb3469e 0x3fa701bf 0x3f9ba5e4 0x3f91a78f 0x3f8d4fdf 0x3f897b47 0x3f897b42 0x3f897b42 0x00000000\n\
s2 b0xbe99999a l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0xbe99999a l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s2 b0xbe99999a l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x00000000 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x00000000 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x00000000 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x3eb33333 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x2612f052 0x3c8d8971 0x3e1b8d8f 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e8aaaab 0x3eaaaaab 0x3ead2f85 0x3f000000 0x3f26e365 0x3f2aaaab 0x3f555556 0x3f5697c2 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x3eb33333 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x2712f052 0x3d8d8971 0x3e1683db 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e800000 0x3eaaaaab 0x3eb4be12 0x3f000000 0x3f25a0f8 0x3f2aaaab 0x3f555556 0x3f5a5f07 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x3eb33333 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x2792f052 0x3e0d8971 0x3e3ed17b 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3ed55556 0x3eaaaaab 0x3ebed179 0x3f000000 0x3f2fb45e 0x3f2aaaab 0x3f555556 0x3f5f68b8 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0x3fc00000 l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x3c50094e 0x3c6008e4 0x3c6008e6 0x3ca3d774 0x3deec81a 0x3e00a900 0x3e19031c 0x3e212f69 0x3e2a3fde 0x3e38ab39 0x3e38ab3a 0x3e8aaaab 0x3e9cd7ba 0x3eb33670 0x3efb425e 0x3f1dd903 0x3f2a8ff8 0x3f4e6bdd 0x3f599b38 0x3f763f91 0x3f7da129 0x3f7da130 0x3f7da130 0x00000000\n\
s7 b0x3fc00000 l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x3d50094e 0x3d6008e4 0x3d6008e6 0x3da3d774 0x3dcc9906 0x3de550e3 0x3e132097 0x3e1e0653 0x3e2a1c44 0x3e62ace4 0x3e62ace5 0x3e800000 0x3e983c14 0x3eccd9be 0x3ef9add3 0x3f199321 0x3f2a8711 0x3f4c1e0a 0x3f666cdf 0x3f72ff6c 0x3f7cd6e1 0x3f7cd6ea 0x3f7cd6ea 0x00000000\n\
s7 b0x3fc00000 l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x3dd0094e 0x3de008e4 0x3de008e6 0x3e23d774 0x3e6f08d3 0x3e62ace4 0x3e4234bf 0x3e374f03 0x3e2b3912 0x3e8d578f 0x3e8d5790 0x3ed55556 0x3ebd1942 0x3eef08d2 0x3f032916 0x3f3bc235 0x3f2ace45 0x3f5e8ca2 0x3f778469 0x3f86804a 0x3f819490 0x3f81948b 0x3f81948b 0x00000000\n\
s7 b0xbe99999a l0x3e800000 u0x3f400000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0xbe99999a l0x3f800000 u0x3f800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
s7 b0xbe99999a l0x40000000 u0xbf800000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3eaaaaab 0x3f000000 0x3f2aaaab 0x3f2aaaab 0x3f555556 0x3f555556 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000\n\
flat 0x7fc00000\n\
nan 0x00000000";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let terrain = source(SourceKind::Perlin, 701, 96.0, 4, 0.5);
        let lower = source(SourceKind::Simplex, 702, 72.0, 3, 0.5);
        let upper = source(SourceKind::Ridge, 703, 128.0, 3, 0.975);
        let terrace = |source: &Module, steps, blend| {
            source
                .clone()
                .legacy_terrace(lower.clone(), upper.clone(), steps, blend)
        };
        let shifted = terrain
            .clone()
            .scale(Module::constant(1.5))
            .bias(Module::constant(-0.2));
        for (index, module) in [
            terrace(&terrain, 7, 0.35),
            terrace(&shifted, 5, 0.6),
            terrace(&terrain, 3, 0.0),
            terrace(&terrain, 4, 1.5),
        ]
        .into_iter()
        .enumerate()
        {
            let mut line = format!(
                "noise{index} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for i in 0..24 {
                let (x, y) = (-1500.0 + i as f32 * 131.37, 900.0 - i as f32 * 77.21);
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        }
        let values = [
            -0.05,
            -0.0,
            0.0,
            0.03,
            0.05,
            0.05416666,
            0.05416667,
            0.08,
            0.1,
            0.1125,
            0.15,
            0.16666667,
            0.2,
            0.22083333,
            0.22083335,
            0.25,
            0.3,
            0.4,
            0.5,
            0.6,
            0.7,
            0.8,
            0.9,
            0.95,
            0.999999,
            1.0,
            1.05,
            f32::NAN,
        ];
        for steps in [1, 2, 7] {
            for blend in [0.0, 0.35, 1.5, -0.3] {
                for [lower, upper] in [[0.25, 0.75], [1.0, 1.0], [2.0, -1.0]] {
                    let mut line =
                        format!("s{steps} b{} l{} u{} |", hex(blend), hex(lower), hex(upper));
                    for value in values {
                        let module = Module::new_constant(value)
                            .clamp(Module::constant(0.0), Module::constant(1.0))
                            .legacy_terrace(
                                Module::constant(lower),
                                Module::constant(upper),
                                steps,
                                blend,
                            );
                        line += &format!(" {}", hex(module.value(0.0, 0.0)));
                    }
                    lines.push(line);
                }
            }
        }
        let one = || Module::constant(1.0);
        let flat = Module::constant(0.5).legacy_terrace(one(), one(), 7, 0.35);
        lines.push(format!("flat {}", hex(flat.value(0.0, 0.0))));
        let nan = Module::constant(f32::NAN)
            .clamp(Module::constant(0.0), one())
            .legacy_terrace(one(), one(), 7, 0.35);
        lines.push(format!("nan {}", hex(nan.value(0.0, 0.0))));
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    #[test]
    fn legacy_terrace_without_steps_panics_where_engine_throws() {
        // Engine builds a legacy terrace with no steps but throws `ArrayIndexOutOfBoundsException` whenever it samples
        // one (`steps[round(value * -1)]`), for every sample in 0..1.
        for value in [0.0, 0.4, 0.6, 1.0] {
            let module = Module::constant(value)
                .clamp(Module::constant(0.0), Module::constant(1.0))
                .legacy_terrace(Module::constant(1.0), Module::constant(1.0), 0, 0.35);
            // Noise2D 2cb4843's seed-first `getValue` throws the same way.
            for panic in [
                std::panic::catch_unwind(|| module.value(0.0, 0.0)).map(|_| ()),
                std::panic::catch_unwind(|| module.try_value_seed_first(17, 0.0, 0.0)).map(|_| ()),
            ] {
                let panic = panic.expect_err("a legacy terrace without steps samples nothing");
                let message = panic.downcast_ref::<String>().map_or("", String::as_str);
                assert!(message.contains("Java throws"), "{message}");
            }
        }
    }

    #[test]
    fn advanced_terrace_matches_engine_0_3_0() {
        // Java `AdvancedTerraces` oracle (`.local/terraforged/oracles/advancedterrace/`): noise advanced terraces (one
        // with a LandForms-style clamped and mapped mask, one over a source whose range does not start at 0, one with
        // `blendMin` above `blendMax`), then constant samples swept over steps and octaves (including 0, negative and a
        // `steps * octave` that wraps), blend windows and masks (-0.0 and NaN), slopes, modulations (`modRange` 1.1, 1,
        // 0 and -1) and unclamped constants, whose own value sets `modRange`.
        const EXPECTED: &str = "\
noise0 0x00000000 0x3f800000 | 0x3ee7cf9c 0x3ef02242 0x3f3915fc 0x3f2ca1ef 0x3ed55258 0x3f2c8f54 0x3f06ba96 0x3eeddd49 0x3ee4c6f6 0x3f05cb16 0x3f1bf656 0x3ea76e69 0x3eecc3c2 0x3efee820 0x3e8278de 0x3ee319b3 0x3e9d1f7a 0x3f07b3c3 0x3f05fd5f 0x3ebf6b40 0x3ea253c5 0x3ec698fc 0x3ee01028 0x3f0e07be\n\
noise1 0x00000000 0x3f800000 | 0x3eea5f3b 0x3ef6b612 0x3f375b16 0x3f2f03d6 0x3ee4cb69 0x3f2a15b6 0x3ef7ba33 0x3eea0b33 0x3ee5e976 0x3f04e477 0x3f294ff4 0x3ea9e602 0x3ee9f017 0x3ef994ac 0x3e833fdf 0x3ee52539 0x3e9db688 0x3f0a16d8 0x3eff39d2 0x3ebc9be5 0x3ea738a8 0x3ec30a8c 0x3edad183 0x3f1878a1\n\
noise2 0xbe4ccccd 0x3fa66666 | 0x3ed8cff2 0x3efbea94 0x3f48ffad 0x3f374ab7 0x3ec1ee1a 0x3f4169f6 0x3f05d071 0x3ed27f68 0x3ec038be 0x3f0cd4db 0x3f20d60e 0x3e88622c 0x3ee601ba 0x3ef13fa4 0x3e432d52 0x3ed6aeb0 0x3e821636 0x3f116c36 0x3f0aef93 0x3eaba349 0x3e75c3da 0x3eb2021c 0x3eec99f6 0x3f033d4a\n\
noise3 0x00000000 0x3f800000 | 0x3eea5f3b 0x3ee831be 0x3f3a6119 0x3f2ae905 0x3ee4cb69 0x3f2c276d 0x3ef7ba33 0x3f00333f 0x3ef040f9 0x3f04e477 0x3f152594 0x3ea9e602 0x3ee76a4f 0x3ef994ac 0x3e819502 0x3ee52539 0x3e9f01cb 0x3f0a16d8 0x3efc02bd 0x3ebc9be5 0x3ea738a8 0x3ec30a8c 0x3ed67fbb 0x3f1878a1\n\
s8 o3 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9adc51 0x3f02e8ba 0x3f39bed6 0x3f4ac37e 0x3f4ac37e 0x3f78253c 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9c1f08 0x3f05d174 0x3f404a79 0x3f48ba2e 0x3f48ba2f 0x3f7d1745 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e81bed6 0x3ea129e4 0x3f05d174 0x3f3b0df6 0x3f4ac37e 0x3f4ac37e 0x3f78253c 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e9d1746 0x3ea8ba2e 0x3f0ba2e9 0x3f42e8ba 0x3f48ba2e 0x3f48ba2f 0x3f7d1745 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s8 o3 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s8 o3 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4ac37e 0x3f78253c 0x3f800000 0x7fc00000\n\
s8 o3 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f48ba2f 0x3f7d1745 0x3f800000 0x7fc00000\n\
s8 o3 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s8 o3 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e939680 0x3f18ba2e 0x3f4bed61 0x3f6094f2 0x3f6094f3 0x3f78253c 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e8d9365 0x3f31745d 0x3f64a790 0x3f745d16 0x3f745d18 0x3f7d1745 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e2c37dc 0x3e6b0df8 0x3f31745d 0x3f50df6a 0x3f6094f2 0x3f6094f3 0x3f78253c 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e0ba2e9 0x3e22e8bb 0x3f62e8ba 0x3f6e8ba2 0x3f745d16 0x3f745d18 0x3f7d1745 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s1 o1 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s1 o1 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f6094f3 0x3f78253c 0x3f800000 0x7fc00000\n\
s1 o1 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f745d18 0x3f7d1745 0x3f800000 0x7fc00000\n\
s1 o1 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s1 o1 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s0 o1 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s0 o1 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s0 o1 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9d48ec 0x3f0a2e8c 0x3f33af52 0x3f437dac 0x3f437dad 0x3f78253c 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3ea0f83e 0x3f145d18 0x3f342b72 0x3f3a2e8c 0x3f3a2e8c 0x3f7d1745 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e904a7a 0x3eafb587 0x3f145d18 0x3f33c826 0x3f437dac 0x3f437dad 0x3f78253c 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3eba2e8c 0x3ec5d174 0x3f28ba2f 0x3f345d18 0x3f3a2e8c 0x3f3a2e8c 0x3f7d1745 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s-3 o2 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f437dad 0x3f78253c 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f3a2e8c 0x3f7d1745 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s-3 o2 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9c50ae 0x3f02e8ba 0x3f361bed 0x3f4f2095 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9f07c2 0x3f05d174 0x3f3904a8 0x3f51745d 0x3f51745e 0x3f745d16 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e720950 0x3ea9e413 0x3f05d174 0x3f36b0e0 0x3f4f2095 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e8ba2e9 0x3eba2e8c 0x3f0ba2e9 0x3f3a2e8c 0x3f51745d 0x3f51745e 0x3f745d16 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s5 o0 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s5 o0 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s5 o0 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f51745e 0x3f745d16 0x3f800000 0x7fc00000\n\
s5 o0 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s5 o0 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3f4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9c50ae 0x3f02e8ba 0x3f361bed 0x3f4f2095 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3f4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9f07c2 0x3f05d174 0x3f3904a8 0x3f51745d 0x3f51745d 0x3f745d16 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3f4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3f4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e720950 0x3ea9e413 0x3f05d174 0x3f36b0e0 0x3f4f2095 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e8ba2e9 0x3eba2e8c 0x3f0ba2e9 0x3f3a2e8c 0x3f51745d 0x3f51745d 0x3f745d16 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3e4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
s65536 o32768 b0x3f4ccccd 0x3e4ccccd m0x3f000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4f2096 0x3f73c824 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3f4ccccd 0x3e4ccccd m0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f51745d 0x3f745d16 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3f4ccccd 0x3e4ccccd m0x80000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x3f4cccce 0x3f733333 0x3f800000 0x7fc00000\n\
s65536 o32768 b0x3f4ccccd 0x3e4ccccd m0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e99999a 0x3f000000 0x3f333333 0x3f4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
slope 0x00000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9b26ca 0x3f05d174 0x3f42b714 0x3f45d174 0x3f45d174 0x3f800000 0x3f800000 0x7fc00000\n\
slope 0x3f800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9f07c2 0x3f05d174 0x3f3904a8 0x3f51745d 0x3f51745e 0x3f745d16 0x3f800000 0x7fc00000\n\
slope 0xbf800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4ccccf 0x3e7ad40b 0x3f05d174 0x3f868780 0x3ed9364c 0x3ed9364a 0x3fac9b26 0x3f800000 0x7fc00000\n\
slope 0x40000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3ea62434 0x3f05d174 0x3f273d8b 0x3f66c9b3 0x3f66c9b5 0x3f5f07c2 0x3f800000 0x7fc00000\n\
slope 0x7fc00000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
mod 0x3dcccccd | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e9c1f08 0x3f05d174 0x3f404a79 0x3f48ba2e 0x3f48ba2f 0x3f7d1745 0x3f800000 0x7fc00000\n\
mod 0x00000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccce 0x3e966667 0x3f000000 0x3f3b3333 0x3f433333 0x3f433334 0x3f7ccccd 0x3f800000 0x7fc00000\n\
mod 0xbf800000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0xff800000 0xff800000 0xff800000 0xff800000 0xff800000 0xff800000 0xff800000 0x7fc00000 0x7fc00000\n\
mod 0xc0000000 | 0x00000000 0x3dcccccd 0x3e4ccccd 0x3e4cccd1 0x3f0a2222 0x3f800000 0x3f95999a 0x3f9e6666 0x3f9e6666 0x3f81999a 0x3f800000 0x7fc00000\n\
raw 0x3f000000 0x3f000000 0x3f000000 0x3f400000\n\
raw 0x3f666666 0x3f666666 0x3f666666 0x3f7b3334\n\
raw 0x40400000 0x40400000 0x40400000 0x3f800000";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let c = Module::new_constant;
        let mut lines = Vec::new();
        let terrain = source(SourceKind::Perlin, 801, 96.0, 4, 0.5);
        let modulation = source(SourceKind::Simplex, 802, 80.0, 3, 0.5);
        let mask = source(SourceKind::Ridge, 803, 128.0, 2, 0.975);
        let slope = source(SourceKind::Perlin, 804, 64.0, 2, 0.5);
        let land_mask = source(SourceKind::Perlin, 805, 100.0, 1, 0.5)
            .clamp(Module::constant(0.5), Module::constant(0.95))
            .map(Module::constant(0.0), Module::constant(1.0));
        let terrace = |source: &Module, min, max, steps, octaves| {
            source.clone().advanced_terrace(
                modulation.clone(),
                mask.clone(),
                slope.clone(),
                min,
                max,
                steps,
                octaves,
            )
        };
        for (index, module) in [
            terrace(&terrain, 0.2, 0.8, 8, 3),
            terrain.clone().advanced_terrace(
                modulation.clone().scale(Module::constant(0.5)),
                land_mask,
                Module::constant(0.45),
                0.2,
                0.45,
                24,
                1,
            ),
            terrace(
                &terrain
                    .clone()
                    .scale(Module::constant(1.5))
                    .bias(Module::constant(-0.2)),
                0.0,
                0.3,
                6,
                2,
            ),
            terrace(&terrain, 0.6, 0.3, 5, 2),
        ]
        .into_iter()
        .enumerate()
        {
            let mut line = format!(
                "noise{index} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for i in 0..24 {
                let (x, y) = (-1500.0 + i as f32 * 131.37, 900.0 - i as f32 * 77.21);
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        }
        let values = [
            -0.5,
            0.1,
            0.2,
            0.20000002,
            0.3,
            0.5,
            0.7,
            0.8,
            0.8000001,
            0.95,
            1.0,
            f32::NAN,
        ];
        let bounded = |value| c(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let sweep = |label: String, module: &dyn Fn(f32) -> Module| {
            values.iter().fold(label + " |", |line, &value| {
                line + " " + &hex(module(value).value(0.0, 0.0))
            })
        };
        for [steps, octaves] in [[8, 3], [1, 1], [0, 1], [-3, 2], [5, 0], [65536, 32768]] {
            for [min, max] in [[0.2, 0.8], [0.2, 0.2], [0.8, 0.2]] {
                for mask in [0.5, 1.0, -0.0, f32::NAN] {
                    let label = format!(
                        "s{steps} o{octaves} b{} {} m{}",
                        hex(min),
                        hex(max),
                        hex(mask)
                    );
                    lines.push(sweep(label, &|value| {
                        bounded(value).advanced_terrace(
                            c(0.1),
                            c(mask),
                            c(0.25),
                            min,
                            max,
                            steps,
                            octaves,
                        )
                    }));
                }
            }
        }
        for slope in [0.0, 1.0, -1.0, 2.0, f32::NAN] {
            lines.push(sweep(format!("slope {}", hex(slope)), &|value| {
                bounded(value).advanced_terrace(
                    c(0.1),
                    Module::constant(1.0),
                    c(slope),
                    0.2,
                    0.8,
                    8,
                    3,
                )
            }));
        }
        for modulation in [0.1, 0.0, -1.0, -2.0] {
            lines.push(sweep(format!("mod {}", hex(modulation)), &|value| {
                bounded(value).advanced_terrace(
                    c(modulation),
                    Module::constant(1.0),
                    c(0.25),
                    0.2,
                    0.8,
                    8,
                    3,
                )
            }));
        }
        for value in [0.5, 0.9, 3.0] {
            let module =
                c(value).advanced_terrace(c(0.1), Module::constant(1.0), c(0.25), 0.2, 0.8, 8, 3);
            lines.push(format!(
                "raw {} {} {} {}",
                hex(value),
                hex(module.min_value()),
                hex(module.max_value()),
                hex(module.value(0.0, 0.0))
            ));
        }
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    #[test]
    fn curve_modifiers_match_engine_0_3_0() {
        // The `points` lines of the Java `Curves` oracle (`.local/terraforged/oracles/curves/`).
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let base = source(SourceKind::Perlin, 901, 96.0, 4, 0.5);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cases = [
            (
                base.clone().curve(CurveFunction::Midpoint {
                    midpoint: 0.4,
                    steepness: 6.0,
                }),
                [
                    0x3f255467, 0x3f42e2dd, 0x3f0fcb78, 0x3f24d9c2, 0x3f4c0704, 0x3f606716,
                    0x3f121320, 0x3edb3db7,
                ],
            ),
            (
                base.clone().curve(CurveFunction::s_curve(0.5, -0.2)),
                [
                    0x3f420300, 0x3f51b756, 0x3f36cc95, 0x3f41c3b0, 0x3f56f9f6, 0x3f647114,
                    0x3f3800c8, 0x3f236a93,
                ],
            ),
            (
                base.clone().variable_curve(
                    source(SourceKind::Simplex, 902, 80.0, 3, 0.5),
                    source(SourceKind::Ridge, 903, 128.0, 2, 0.975),
                ),
                [
                    0x3f000000, 0x3f065e28, 0x3ef0ed3b, 0x3f0513bd, 0x3f03d748, 0x3f0aaa49,
                    0x3efe7b16, 0x3eea1d84,
                ],
            ),
            (
                base.clone().power_curve(0.8),
                [
                    0x3f000000, 0x3f2148a1, 0x3ed1e2d7, 0x3efdbc37, 0x3f2ad941, 0x3f43b07c,
                    0x3ed5b8cd, 0x3e9f3786,
                ],
            ),
            (
                base.clone().steps(
                    Module::constant(9.5),
                    Module::constant(0.2),
                    Module::constant(0.8),
                    CurveFunction::Interpolation(Interpolation::Curve4),
                ),
                [
                    0x3f001a52, 0x3f17c488, 0x3ef1d004, 0x3eff64d8, 0x3f260dbe, 0x3f3cd8b8,
                    0x3ef2751a, 0x3ebca1b0,
                ],
            ),
            (
                base.steps(
                    Module::constant(7.0),
                    Module::constant(0.5),
                    Module::constant(0.5),
                    CurveFunction::Interpolation(Interpolation::Curve3),
                ),
                [
                    0x3edb6db7, 0x3f124925, 0x3edb6db7, 0x3edb6db7, 0x3f124925, 0x3f36db6e,
                    0x3edb6db7, 0x3e924925,
                ],
            ),
        ];
        for (module, expected) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                [0x00000000, 0x3f800000]
            );
            assert_eq!(bits(points.map(|(x, y)| module.value(x, y))), expected);
        }
    }

    #[test]
    fn curve_sweep_matches_engine_0_3_0() {
        // Java `Curves` oracle (`.local/terraforged/oracles/curves/`): noise curves (including a power curve over a
        // source whose range is not 0..1 and steps whose count and slopes are noise), then constant samples (signed
        // zeros, values outside 0..1, NaN and infinities) through midpoint and variable curves (`NoiseUtil.exp`
        // overflowing and NaN controls), S-curves (a negative `upper` raised to `-lower`), Curve4, power curves over
        // -1..2 bounds and over a constant's empty range, and steps over counts (0, negative, huge, NaN) and slope
        // windows (empty, inverted, NaN).
        const EXPECTED: &str = "\
noise0 0x00000000 0x3f800000 | 0x3f5501b4 0x3f2555c4 0x3ecfea00 0x3f489afa 0x3f0e6243 0x3f456e3e 0x3f011bc9 0x3f0961d1 0x3f50c10e 0x3f2781ab 0x3ea77285 0x3e8e6eef 0x3f234baa 0x3f3a9796 0x3f3638d1 0x3ec95c4f 0x3f3518ff 0x3f095c35 0x3f247068 0x3f1caa61 0x3f122cb1 0x3f55b332 0x3efa1ea0 0x3f2c21ba
noise1 0x00000000 0x3f800000 | 0x3f5c8663 0x3f4203c5 0x3f1fd133 0x3f54f838 0x3f360cf6 0x3f532652 0x3f2ed77b 0x3f335f8b 0x3f59d6ed 0x3f432366 0x3f115b3c 0x3f06899a 0x3f40f5e7 0x3f4d2b09 0x3f4ad4da 0x3f1da91b 0x3f4a3c41 0x3f335c5d 0x3f418d3d 0x3f3d87b9 0x3f380e33 0x3f5cf966 0x3f2c9107 0x3f45896e
noise2 0x00000000 0x3f800000 | 0x3f091877 0x3efd721f 0x3f0154ca 0x3f192179 0x3f011407 0x3efe9412 0x3ee45fc0 0x3f0afbba 0x3ef993bd 0x3efb5c2d 0x3f0309e6 0x3ed1f9dc 0x3f07b7c5 0x3f0f700c 0x3f070c9a 0x3ee7c4e5 0x3efc1847 0x3efe9f0d 0x3f038e17 0x3effbc02 0x3efec708 0x3efc0156 0x3ef2b1e0 0x3efd811e
noise3 0x00000000 0x3f800000 | 0x3f34f363 0x3f00087f 0x3e975c7e 0x3f27350e 0x3ecf8f5e 0x3f23e621 0x3ebafe8e 0x3ec79073 0x3f300c9e 0x3f03d3dd 0x3e761ace 0x3e514d92 0x3ef8d037 0x3f18defb 0x3f146ce5 0x3e92d320 0x3f13451e 0x3ec7873b 0x3efc4832 0x3ee958d0 0x3ed5e448 0x3f35c631 0x3eb50fd7 0x3f099dd7
noise4 0x00000000 0x3f800000 | 0x3f2f286c 0x3f001ba2 0x3ebc03aa 0x3f1e8486 0x3ef0d732 0x3f1a21a6 0x3ed0bd12 0x3ee8770e 0x3f2e188c 0x3f026d60 0x3e8f23ca 0x3e86bc62 0x3efd0eec 0x3f1435e5 0x3f141c3a 0x3eb8e6ae 0x3f13e1ba 0x3ee8689c 0x3efebdc2 0x3ef3a514 0x3ef277ac 0x3f2f286c 0x3ec8ea4c 0x3f09b6e0
noise5 0x00000000 0x3f800000 | 0x3f124925 0x3edb6db7 0x3e924925 0x3f124925 0x3edb6db7 0x3f124925 0x3e924925 0x3e924925 0x3f124925 0x3edb6db7 0x3e124925 0x3e124925 0x3edb6db7 0x3edb6db7 0x3edb6db7 0x3e924925 0x3edb6db7 0x3e924925 0x3edb6db7 0x3edb6db7 0x3edb6db7 0x3f124925 0x3e924925 0x3edb6db7
noise6 0xbe4ccccd 0x3fa66666 | 0x3f0e16a8 0x3f000000 0x3ee4a7cb 0x3f06a589 0x3efc0352 0x3f0554fd 0x3ef657c2 0x3efa2892 0x3f0b0d10 0x3f000511 0x3ece3e93 0x3ebb3355 0x3efff759 0x3f022154 0x3f014d50 0x3ee19835 0x3f012028 0x3efa2630 0x3efffe55 0x3eff6759 0x3efd30e2 0x3f0ea485 0x3ef421a7 0x3f0032b2
noise7 0x00000000 0x3f800000 | 0x3f2c0e0d 0x3f0986c8 0x3ebd97ce 0x3ecdaca2 0x3ee0f226 0x3f1ab034 0x3ecf3080 0x00000000 0x3f259f7c 0x3f0d148b 0x00000000 0x3e784408 0x3e84704b 0x3f0b7301 0x3f0dd42a 0x3eb3c1ee 0x3f0cfb8a 0x3eea2448 0x3effc89a 0x3ef5b8fa 0x3eee3f48 0x3f2bb6b6 0x3edadbb2 0x3f0a800c
mid 0x3ecccccd 0x40c00000 0x00000000 0x00000000 | 0x3b9bcece 0x3dac17c9 0x3dac17c9 0x3e94290f 0x3f000000 0x3f255467 0x3f64493f 0x3f795bc9 0x3f7fae58 0x3f7ffc50 0x3f7ffffe 0x7fc00000 0x00000000 0x00000000
mid 0x3f000000 0xc47a0000 0x00000000 0x00000000 | 0x00000000 0x3f7fffb2 0x3f7fffb2 0x3f800000 0x3f800000 0x3f000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
mid 0x3f000000 0x447a0000 0x00000000 0x00000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x0294494b 0x3f000000 0x3f800000 0x3f7fffb2 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
mid 0x3f000000 0x00000000 0x00000000 0x00000000 | 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x7fc00000 0x7fc00000 0x7fc00000
mid 0x7fc00000 0x40800000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
mid 0x3f000000 0x7f800000 0x00000000 0x00000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
var 0x3ecccccd 0x40c00000 0x00000000 0x00000000 | 0x3b9bcece 0x3dac17c9 0x3dac17c9 0x3e94290f 0x3f000000 0x3f255467 0x3f64493f 0x3f795bc9 0x3f7fae58 0x3f7ffc50 0x3f7ffffe 0x7fc00000 0x00000000 0x00000000
var 0x3f000000 0xc47a0000 0x00000000 0x00000000 | 0x00000000 0x3f7fffb2 0x3f7fffb2 0x3f800000 0x3f800000 0x3f000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
var 0x3f000000 0x447a0000 0x00000000 0x00000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x0294494b 0x3f000000 0x3f800000 0x3f7fffb2 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
var 0x3f000000 0x00000000 0x00000000 0x00000000 | 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x3f000000 0x7fc00000 0x7fc00000 0x7fc00000
var 0x7fc00000 0x40800000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
var 0x3f000000 0x7f800000 0x00000000 0x00000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x7fc00000 0x00000000 0x00000000
s 0x3f000000 0xbe4ccccd 0x00000000 0x00000000 | 0x7fc00000 0x00000000 0x00000000 0x3f092fe0 0x3f2e390e 0x3f420300 0x3f677a99 0x3f800000 0x3f8acff9 0x3f892fdf 0x3f655d88 0x7fc00000 0x00000000 0x7f800000
s 0x3f000000 0xbf4ccccd 0x00000000 0x00000000 | 0x7fc00000 0x00000000 0x00000000 0x3f1837f0 0x3f4278ef 0x3f5744fd 0x3f76f52e 0x3f800000 0x3f675276 0x3f3504f3 0x3eaaaaab 0x7fc00000 0x00000000 0x7f800000
s 0xbf800000 0x40000000 0x00000000 0x00000000 | 0x40800000 0xff800000 0x7f800000 0x40000000 0x3f99be72 0x3f800000 0x3f5db3d7 0x3f800000 0x40100000 0x41000000 0x43730000 0x7fc00000 0x7f800000 0x00000000
s 0x00000000 0x00000000 0x00000000 0x00000000 | 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x7fc00000 0x7fc00000 0x7fc00000
s 0x40000000 0xc0400000 0x00000000 0x00000000 | 0xbe000000 0x00000000 0x00000000 0x3e000000 0x3eaa81c4 0x3f000000 0x3f5db3d7 0x3f800000 0x3f2aaaab 0x3e800000 0x3c4a4588 0x7fc00000 0x00000000 0x7f800000
s 0x3f000000 0x7fc00000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
s 0x7fc00000 0xbf800000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
curve4 0x00000000 0x00000000 | 0xc0180000 0x80000000 0x00000000 0x3dd40000 0x3ea2877e 0x3f000000 0x3f658000 0x3f800000 0x40580000 0x42000000 0x44004000 0x7fc00000 0x7f800000 0xff800000
pow 0x3f4ccccd 0xbf800000 0x40000000 | 0x3e0dd56a 0x3e95b298 0x3e95b298 0x3ec2f200 0x3ee2aa92 0x3f000000 0x3f1e8700 0x3f3526b4 0x3f5c8aa6 0x3f800000 0x3f800000 0x7fc00000 0x3f800000 0x00000000
pow 0x40000000 0xbf800000 0x40000000 | 0x3e8e38e4 0x3ee38e39 0x3ee38e39 0x3ef8e38e 0x3efedcbb 0x3f000000 0x3f038e39 0x3f0e38e4 0x3f38e38e 0x3f800000 0x3f800000 0x7fc00000 0x3f800000 0x00000000
pow 0x3f000000 0xbf800000 0x40000000 | 0x3dbbe854 0x3e58658d 0x3e58658d 0x3e977d0a 0x3ebde6ad 0x3f000000 0x3f34417b 0x3f49e69d 0x3f6882f5 0x3f800000 0x3f800000 0x7fc00000 0x3f800000 0x00000000
pow 0x3f800000 0xbf800000 0x40000000 | 0x3e2aaaab 0x3eaaaaab 0x3eaaaaab 0x3ed55555 0x3eeeeeef 0x3f000000 0x3f155555 0x3f2aaaab 0x3f555555 0x3f800000 0x3f800000 0x7fc00000 0x3f800000 0x00000000
pow 0x00000000 0xbf800000 0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x00000000 0x3f800000 0x00000000
pow 0xbf800000 0xbf800000 0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x7fc00000 0x3f800000 0x333fffff
pow 0x7fc00000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x3f4ccccd 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x40000000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x3f000000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x3f800000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x00000000 0x00000000 0x00000000 | 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x3f800000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0xbf800000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
powc 0x7fc00000 0x00000000 0x00000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x41180000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbef2ca9c 0x3cd79440 0x3cd79440 0x3e86bca2 0x3ecf9834 0x3f0065cd 0x3f48071c 0x3f800000 0x3fb5e50e 0x3ff9435e 0x3ff9435e 0x7fc00000 0x3ff9435e 0xbf800000
steps c0 0x41180000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0xbed79436 0x00000000 0x00000000 0x3e579436 0x3ea1af28 0x3ed79436 0x3f3ca1af 0x3f7286bd 0x3fbca1af 0x40000000 0x40000000 0x00000000 0x40000000 0xbf7286bd
steps c0 0x41180000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbed79436 0x00000000 0x00000000 0x3e579436 0x3ea1af28 0x3ed79436 0x3f3ca1af 0x3f7286bd 0x3fbca1af 0x40000000 0x40000000 0x00000000 0x40000000 0xbf7286bd
steps c0 0x41180000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0xbef4a1b0 0x3cd79440 0x3cd79440 0x3e867288 0x3ed4f2ea 0x3f03286c 0x3f46e86c 0x3f800000 0x3fb5e50e 0x3ff9435e 0x3ff9435e 0x7fc00000 0x3ff9435e 0xbf800000
steps c0 0x41180000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x40e00000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbeedb6dc 0x00000000 0x00000000 0x3e91ed0a 0x3edb6db6 0x3f092492 0x3f408a29 0x3f800000 0x3fb6db6e 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0x40e00000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0xbedb6db7 0x00000000 0x00000000 0x3e124925 0x3e924925 0x3edb6db7 0x3f36db6e 0x3f800000 0x3fb6db6e 0x40000000 0x40000000 0x00000000 0x40000000 0xbf800000
steps c0 0x40e00000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbedb6db7 0x00000000 0x00000000 0x3e124925 0x3e924925 0x3edb6db7 0x3f36db6e 0x3f800000 0x3fb6db6e 0x40000000 0x40000000 0x00000000 0x40000000 0xbf800000
steps c0 0x40e00000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0xbeedb6dc 0x00000000 0x00000000 0x3e8f6db6 0x3ed9e83e 0x3f092492 0x3f444924 0x3f800000 0x3fb6db6e 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0x40e00000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x3f800000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbe800000 0x00000000 0x00000000 0x3e878e38 0x3f0e38e3 0x3f400000 0x3f7ebda1 0x3f800000 0x3f800000 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0x3f800000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x40000000 0x40000000 0x00000000 0x40000000 0xbf800000
steps c0 0x3f800000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x00000000 0x3f800000 0x3f800000 0x40000000 0x40000000 0x00000000 0x40000000 0xbf800000
steps c0 0x3f800000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0xbe800000 0x00000000 0x00000000 0x3ebc0000 0x3f1c779a 0x3f400000 0x3f760000 0x3f800000 0x3f800000 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0x3f800000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x00000000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x00000000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x00000000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x00000000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x00000000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0xc0400000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbeaaaaac 0x00000000 0x00000000 0x3eaaaaaa 0x3f2aaaaa 0x3f2aaaaa 0x3f800000 0x3f800000 0x3fb55555 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0xc0400000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0xbeaaaaab 0x80000000 0x80000000 0x80000000 0x3eaaaaab 0x3eaaaaab 0x3f2aaaab 0x3f800000 0x3faaaaab 0x40000000 0x40000000 0x80000000 0x40000000 0xbf800000
steps c0 0xc0400000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbeaaaaab 0x80000000 0x80000000 0x80000000 0x3eaaaaab 0x3eaaaaab 0x3f2aaaab 0x3f800000 0x3faaaaab 0x40000000 0x40000000 0x80000000 0x40000000 0xbf800000
steps c0 0xc0400000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0xbeaaaaac 0x00000000 0x00000000 0x3eaaaaaa 0x3f2aaaaa 0x3f2aaaaa 0x3f800000 0x3f800000 0x3fb55555 0x40000000 0x40000000 0x7fc00000 0x40000000 0xbf800000
steps c0 0xc0400000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x501502f9 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbf000000 0x00000000 0x00000000 0x3e800000 0x3ecccccc 0x3f000000 0x3f400000 0x3f800000 0x3f9b7ce0 0x3f9b7ce0 0x3f9b7ce0 0x7fc00000 0x3f9b7ce0 0xbf800000
steps c0 0x501502f9 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0xbe5be6ff 0x00000000 0x00000000 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x00000000 0x3e5be6ff 0xbe5be6ff
steps c0 0x501502f9 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbe5be6ff 0x00000000 0x00000000 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x3e5be6ff 0x00000000 0x3e5be6ff 0xbe5be6ff
steps c0 0x501502f9 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0xbf000000 0x00000000 0x00000000 0x3e800000 0x3ecccccc 0x3f000000 0x3f400000 0x3f800000 0x3f9b7ce0 0x3f9b7ce0 0x3f9b7ce0 0x7fc00000 0x3f9b7ce0 0xbf800000
steps c0 0x501502f9 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x7fc00000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x7fc00000 0x3f000000 0x3f000000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x7fc00000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x7fc00000 0x00000000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c0 0x7fc00000 0x7fc00000 0x3f800000 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c1 0x41180000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbef3a62c 0x3cd79440 0x3cd79440 0x3e86bca2 0x3ed31674 0x3f01af28 0x3f47286c 0x3f800000 0x3fb5e50e 0x3ff9435e 0x3ff9435e 0x7fc00000 0x3ff9435e 0xbf800000
steps c1 0x41180000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbed79436 0x00000000 0x00000000 0x3e579436 0x3ea1af28 0x3ed79436 0x3f3ca1af 0x3f7286bd 0x3fbca1af 0x40000000 0x40000000 0x00000000 0x40000000 0xbf7286bd
steps c1 0x00000000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c1 0x00000000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c1 0x7fc00000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c1 0x7fc00000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c2 0x41180000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0xbef30008 0x3cd79440 0x3cd79440 0x3e869ca8 0x3ecf381a 0x3f00ae7d 0x3f487e16 0x3f800000 0x3fb61504 0x3ff96358 0x3ff96358 0x7fc00000 0x3ff96358 0xbf800000
steps c2 0x41180000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0xbed79436 0x00000000 0x00000000 0x3e579436 0x3ea1af28 0x3ed79436 0x3f3ca1af 0x3f7286bd 0x3fbca1af 0x40000000 0x40000000 0x00000000 0x40000000 0xbf7286bd
steps c2 0x00000000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c2 0x00000000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c2 0x7fc00000 0x3e4ccccd 0x3f4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
steps c2 0x7fc00000 0x3f4ccccd 0x3e4ccccd 0xbf800000 0x40000000 | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000
";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let c = Module::new_constant;
        let base = source(SourceKind::Perlin, 901, 96.0, 4, 0.5);
        let midpoint = source(SourceKind::Simplex, 902, 80.0, 3, 0.5);
        let gradient = source(SourceKind::Ridge, 903, 128.0, 2, 0.975);
        let mut lines = Vec::new();
        for (index, module) in [
            base.clone().curve(CurveFunction::Midpoint {
                midpoint: 0.4,
                steepness: 6.0,
            }),
            base.clone().curve(CurveFunction::s_curve(0.5, -0.2)),
            base.clone()
                .variable_curve(midpoint.clone(), gradient.clone()),
            base.clone().power_curve(0.8),
            base.clone().steps(
                Module::constant(9.5),
                Module::constant(0.2),
                Module::constant(0.8),
                CurveFunction::Interpolation(Interpolation::Curve4),
            ),
            base.clone().steps(
                Module::constant(7.0),
                Module::constant(0.5),
                Module::constant(0.5),
                CurveFunction::Interpolation(Interpolation::Curve3),
            ),
            base.clone()
                .scale(Module::constant(1.5))
                .bias(Module::constant(-0.2))
                .power_curve(2.0),
            base.steps(
                midpoint.clone().scale(Module::constant(12.0)),
                gradient.scale(Module::constant(0.5)),
                midpoint,
                CurveFunction::Midpoint {
                    midpoint: 0.5,
                    steepness: 8.0,
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut line = format!(
                "noise{index} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for i in 0..24 {
                let (x, y) = (-1500.0 + i as f32 * 131.37, 900.0 - i as f32 * 77.21);
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        }
        let values = [
            -0.5,
            -0.0,
            0.0,
            0.25,
            0.4,
            0.5,
            0.75,
            1.0,
            1.5,
            2.0,
            3.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        // A constant with bounds -1..2 (values outside are clamped to them).
        let wide = |value| c(value).clamp(c(-1.0), c(2.0));
        let mut sweep = |name: String, make: &dyn Fn(Module) -> Module, wide_bounds: bool| {
            let input = |value| if wide_bounds { wide(value) } else { c(value) };
            let probe = make(input(0.0));
            let mut line = format!(
                "{name} {} {} |",
                hex(probe.min_value()),
                hex(probe.max_value())
            );
            for value in values {
                line += &format!(" {}", hex(make(input(value)).value(0.0, 0.0)));
            }
            lines.push(line);
        };
        let midpoints = [
            (0.4, 6.0),
            (0.5, -1000.0),
            (0.5, 1000.0),
            (0.5, 0.0),
            (f32::NAN, 4.0),
            (0.5, f32::INFINITY),
        ];
        for (midpoint, steepness) in midpoints {
            let function = CurveFunction::Midpoint {
                midpoint,
                steepness,
            };
            sweep(
                format!("mid {} {}", hex(midpoint), hex(steepness)),
                &|source| source.curve(function),
                false,
            );
        }
        for (midpoint, steepness) in midpoints {
            sweep(
                format!("var {} {}", hex(midpoint), hex(steepness)),
                &|source| source.variable_curve(c(midpoint), c(steepness)),
                false,
            );
        }
        for (lower, upper) in [
            (0.5, -0.2),
            (0.5, -0.8),
            (-1.0, 2.0),
            (0.0, 0.0),
            (2.0, -3.0),
            (0.5, f32::NAN),
            (f32::NAN, -1.0),
        ] {
            let function = CurveFunction::s_curve(lower, upper);
            sweep(
                format!("s {} {}", hex(lower), hex(upper)),
                &|source| source.curve(function),
                false,
            );
        }
        sweep(
            "curve4".to_string(),
            &|source| source.curve(CurveFunction::Interpolation(Interpolation::Curve4)),
            false,
        );
        let powers = [0.8, 2.0, 0.5, 1.0, 0.0, -1.0, f32::NAN];
        for power in powers {
            sweep(
                format!("pow {}", hex(power)),
                &|source| source.power_curve(power),
                true,
            );
        }
        for power in powers {
            sweep(
                format!("powc {}", hex(power)),
                &|source| source.power_curve(power),
                false,
            );
        }
        let steps =
            |name: usize,
             counts: &[f32],
             slopes: &[(f32, f32)],
             curve: CurveFunction,
             sweep: &mut dyn FnMut(String, &dyn Fn(Module) -> Module, bool)| {
                for &count in counts {
                    for &(min, max) in slopes {
                        sweep(
                            format!("steps c{name} {} {} {}", hex(count), hex(min), hex(max)),
                            &|source| source.steps(c(count), c(min), c(max), curve),
                            true,
                        );
                    }
                }
            };
        let (few_counts, few_slopes) = ([9.5, 0.0, f32::NAN], [(0.2, 0.8), (0.8, 0.2)]);
        steps(
            0,
            &[9.5, 7.0, 1.0, 0.0, -3.0, 1e10, f32::NAN],
            &[
                (0.2, 0.8),
                (0.5, 0.5),
                (0.8, 0.2),
                (0.0, 1.0),
                (f32::NAN, 1.0),
            ],
            CurveFunction::Interpolation(Interpolation::Curve3),
            &mut sweep,
        );
        steps(
            1,
            &few_counts,
            &few_slopes,
            CurveFunction::Interpolation(Interpolation::Linear),
            &mut sweep,
        );
        steps(
            2,
            &few_counts,
            &few_slopes,
            CurveFunction::Midpoint {
                midpoint: 0.5,
                steepness: 8.0,
            },
            &mut sweep,
        );
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    /// `Freq`, `Grad`, `Modulate`, `Min` and `Sub` over noise sources, from Engine 0.3.0 (`Scalar.java` oracle).
    #[test]
    fn scalar_nodes_match_engine_0_3_0() {
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let base = source(SourceKind::Perlin, 1001, 96.0, 4, 0.5);
        let a = source(SourceKind::Simplex, 1002, 80.0, 3, 0.5)
            .scale(Module::constant(1.5))
            .bias(Module::constant(-0.25));
        let b = source(SourceKind::Ridge, 1003, 128.0, 2, 0.975)
            .scale(Module::constant(0.75))
            .bias(Module::constant(0.1));
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cases = [
            (
                base.clone().frequency(
                    source(SourceKind::Simplex, 1004, 256.0, 2, 0.5)
                        .map(Module::constant(0.5), Module::constant(2.0)),
                    source(SourceKind::Ridge, 1005, 192.0, 2, 0.975)
                        .map(Module::constant(0.25), Module::constant(1.5)),
                ),
                [0x00000000, 0x3f800000],
                [
                    0x3f000000, 0x3f1f4fc3, 0x3efe6792, 0x3f27efc3, 0x3ea1a4da, 0x3ec0bfaa,
                    0x3f3665b9, 0x3f11cfe3,
                ],
            ),
            (
                base.clone().gradient(
                    Module::constant(0.25),
                    Module::constant(0.75),
                    source(SourceKind::Simplex, 1006, 64.0, 2, 0.5),
                ),
                [0x00000000, 0x3f800000],
                [
                    0x3f1837f0, 0x3f1c2d3b, 0x3f28280d, 0x3f2101f4, 0x3ea5e9fb, 0x3f216958,
                    0x3f2220ae, 0x3f2daaf5,
                ],
            ),
            (
                base.clone().modulate(
                    source(SourceKind::Simplex, 1007, 128.0, 2, 0.5),
                    source(SourceKind::Ridge, 1008, 96.0, 2, 0.975).scale(Module::constant(12.0)),
                ),
                [0x00000000, 0x3f800000],
                [
                    0x3efbd555, 0x3f385619, 0x3f17ef60, 0x3ef02b87, 0x3eaf69cd, 0x3f03a960,
                    0x3f11eaba, 0x3f2fcdb9,
                ],
            ),
            (
                Module::min(vec![base.clone(), a.clone(), b.clone()]),
                [0xbe800000, 0xbe800000],
                [
                    0x3f000000, 0x3f0a6fd8, 0x3f1c821a, 0x3eb15314, 0x3e913646, 0x3f010131,
                    0x3f0d912a, 0x3e773ddc,
                ],
            ),
            (
                Module::subtract(vec![base, a, b]),
                [0xc0066666, 0x3f933333],
                [
                    0xbf59999a, 0xbf1f148d, 0xbf850877, 0xbf0c0260, 0xbf68f8d3, 0xbf0c63aa,
                    0xbf813c31, 0xbed93fa0,
                ],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
    }

    /// Edge cases of the scalar nodes, from the same oracle: negative samples raised to a fractional power (Java's
    /// -NaN), `lower == upper` in `Grad` (0/0 makes a -NaN power that `Math.pow` returns),
    /// inverted gradient bounds, NaN frequencies, large and infinite modulation angles, `Min`'s signed zeros and NaN,
    /// and empty combiners.
    #[test]
    fn scalar_node_edge_cases_match_engine_0_3_0() {
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let c = Module::constant;
        let base = source(SourceKind::Perlin, 1001, 96.0, 4, 0.5);
        let a = source(SourceKind::Simplex, 1002, 80.0, 3, 0.5)
            .scale(c(1.5))
            .bias(c(-0.25));
        let signed = base.clone().scale(c(2.0)).bias(c(-1.0));
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cases: [(Module, [u32; 2], [u32; 8]); 11] = [
            // grad_neg
            (
                signed.gradient(c(-0.5), c(0.5), c(0.3)),
                [0xbf800000, 0x3f800000],
                [
                    0x00000000, 0x3e290784, 0x3e8134f9, 0xffc00000, 0xffc00000, 0x3dabac4a,
                    0x3e0d82cb, 0x3eb4ba59,
                ],
            ),
            // grad_eq
            (
                base.clone().gradient(c(0.5), c(0.5), c(0.7)),
                [0x00000000, 0x3f800000],
                [
                    0xffc00000, 0x3f10e55f, 0x3f1c821a, 0x3f4fb88c, 0x3f2f6984, 0x3f075721,
                    0x3f0d912a, 0x3f2ac941,
                ],
            ),
            // grad_inv
            (
                base.clone().gradient(c(0.75), c(0.25), c(0.5)),
                [0x00000000, 0x3f800000],
                [
                    0x3f000000, 0x3f10e55f, 0x3f1c821a, 0x3eff1da5, 0x3e913646, 0x3f075721,
                    0x3f0d912a, 0x3f2ac941,
                ],
            ),
            // freq_nan
            (
                base.clone().frequency(c(f32::NAN), c(2.0)),
                [0x00000000, 0x3f800000],
                [
                    0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000,
                    0x7fc00000, 0x7fc00000,
                ],
            ),
            // mod_big
            (
                base.clone().modulate(c(12345.678), c(-50.0)),
                [0x00000000, 0x3f800000],
                [
                    0x3f14fb78, 0x3f0e30bb, 0x3f0e8849, 0x3ee92425, 0x3efa97df, 0x3ed76f3e,
                    0x3eb0ca7c, 0x3f231817,
                ],
            ),
            // mod_inf
            (
                base.clone().modulate(c(f32::INFINITY), c(3.0)),
                [0x00000000, 0x3f800000],
                [
                    0x3f0000a7, 0x3f10e64a, 0x3f1c81c6, 0x3eff1b07, 0x3e913645, 0x3f075899,
                    0x3f0d9271, 0x3f2ac90e,
                ],
            ),
            // min_zero
            (
                Module::min(vec![Module::new_constant(-0.0), c(0.0)]),
                [0x80000000, 0x80000000],
                [
                    0x80000000, 0x80000000, 0x80000000, 0x80000000, 0x80000000, 0x80000000,
                    0x80000000, 0x80000000,
                ],
            ),
            // min_nan
            (
                Module::min(vec![base.clone(), c(f32::NAN), a]),
                [0x7fc00000, 0x7fc00000],
                [
                    0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000,
                    0x7fc00000, 0x7fc00000,
                ],
            ),
            // min_empty
            (
                Module::min(vec![]),
                [0x00000000, 0x00000000],
                [
                    0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
                    0x00000000, 0x00000000,
                ],
            ),
            // sub_empty
            (
                Module::subtract(vec![]),
                [0x00000000, 0x00000000],
                [
                    0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
                    0x00000000, 0x00000000,
                ],
            ),
            // sub_nan
            (
                Module::subtract(vec![base.clone(), c(f32::NAN)]),
                [0x7fc00000, 0x7fc00000],
                [
                    0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000,
                    0x7fc00000, 0x7fc00000,
                ],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
    }

    /// `(float) Math.pow(x, y)` for every pair of bases and powers, from JDK 26 (`NanPow2.java` oracle; interpreted and
    /// compiled code agree). Covers the NaN bits Java returns.
    #[test]
    fn java_pow_matches_math_pow_nan_bits() {
        const BASES: [u32; 12] = [
            0x3f800000, 0xbf800000, 0x3f000000, 0xbf000000, 0x00000000, 0x80000000, 0x40000000,
            0x7f800000, 0xff800000, 0x7fc00000, 0xffc00000, 0x7fc05678,
        ];
        const POWERS: [u32; 11] = [
            0x7fc00000, 0xffc00000, 0xffc01234, 0x00000000, 0x80000000, 0x3f800000, 0x40000000,
            0x3f000000, 0xbf800000, 0x7f800000, 0xff800000,
        ];
        const EXPECTED: [u32; 132] = [
            0x7fc00000, 0x7fc00000, 0x7fc00000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000,
            0x3f800000, 0x3f800000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0xffc00000, 0xffc01234,
            0x3f800000, 0x3f800000, 0xbf800000, 0x3f800000, 0xffc00000, 0xbf800000, 0x7fc00000,
            0x7fc00000, 0x7fc00000, 0xffc00000, 0xffc01234, 0x3f800000, 0x3f800000, 0x3f000000,
            0x3e800000, 0x3f3504f3, 0x40000000, 0x00000000, 0x7f800000, 0x7fc00000, 0xffc00000,
            0xffc01234, 0x3f800000, 0x3f800000, 0xbf000000, 0x3e800000, 0xffc00000, 0xc0000000,
            0x00000000, 0x7f800000, 0x7fc00000, 0xffc00000, 0xffc01234, 0x3f800000, 0x3f800000,
            0x00000000, 0x00000000, 0x00000000, 0x7f800000, 0x00000000, 0x7f800000, 0x7fc00000,
            0xffc00000, 0xffc01234, 0x3f800000, 0x3f800000, 0x80000000, 0x00000000, 0x00000000,
            0xff800000, 0x00000000, 0x7f800000, 0x7fc00000, 0xffc00000, 0xffc01234, 0x3f800000,
            0x3f800000, 0x40000000, 0x40800000, 0x3fb504f3, 0x3f000000, 0x7f800000, 0x00000000,
            0x7fc00000, 0xffc00000, 0xffc01234, 0x3f800000, 0x3f800000, 0x7f800000, 0x7f800000,
            0x7f800000, 0x00000000, 0x7f800000, 0x00000000, 0x7fc00000, 0xffc00000, 0xffc01234,
            0x3f800000, 0x3f800000, 0xff800000, 0x7f800000, 0x7f800000, 0x80000000, 0x7f800000,
            0x00000000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x3f800000, 0x3f800000, 0x7fc00000,
            0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0x7fc00000, 0xffc00000, 0xffc00000,
            0xffc00000, 0x3f800000, 0x3f800000, 0xffc00000, 0xffc00000, 0xffc00000, 0xffc00000,
            0xffc00000, 0xffc00000, 0x7fc05678, 0x7fc05678, 0x7fc05678, 0x3f800000, 0x3f800000,
            0x7fc05678, 0x7fc05678, 0x7fc05678, 0x7fc05678, 0x7fc05678, 0x7fc05678,
        ];
        let actual: Vec<u32> = BASES
            .iter()
            .flat_map(|&x| POWERS.iter().map(move |&y| (x, y)))
            .map(|(x, y)| java_pow(f32::from_bits(x), f32::from_bits(y)).to_bits())
            .collect();
        assert_eq!(actual, EXPECTED);
    }

    /// Samples through `Cache`, checked against Engine 0.3.0 (repeats, ±0 and NaN coordinates), its bounds, its
    /// sharing between clones and `cache()` of a cache, and the entry it keeps.
    #[test]
    fn cache_matches_engine_0_3_0() {
        let module = Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed: 1101,
                octaves: 4,
                gain: 0.5,
                frequency: 1.0 / 96.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
        .cache();
        let clone = module.clone();
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (12.25, -98.75),
            (0.0, 0.0),
            (-0.0, 0.0),
            (777.0, -333.0),
            (777.0, -333.0),
            (f32::NAN, 1.0),
            (f32::NAN, 1.0),
        ];
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0x00000000, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3edc7fe7, 0x3edc7fe7, 0x3f000000, 0x3f000000, 0x3eea770a, 0x3eea770a,
                0x7fc00000, 0x7fc00000,
            ]
        );
        assert!(Arc::ptr_eq(&module.0, &clone.0));
        assert!(Arc::ptr_eq(&module.0, &module.clone().cache().0));
        let ModuleKind::Cache { value, .. } = &module.0.kind else {
            unreachable!()
        };
        let cached = value.lock().unwrap().expect("cache entry");
        assert_eq!(
            [
                cached.x.to_bits(),
                cached.y.to_bits(),
                cached.value.to_bits()
            ],
            [0x7fc00000, 0x3f800000, 0x7fc00000]
        );
        // Unlike Java's `==`, -0.0 does not hit +0.0's entry: it is sampled and replaces it.
        module.value(0.0, 0.0);
        module.value(-0.0, 0.0);
        let cached = value.lock().unwrap().expect("cache entry");
        assert_eq!([cached.x.to_bits(), cached.y.to_bits()], [0x80000000, 0]);
    }

    /// Many threads sampling one cache, each walking its own coordinates, see exactly the uncached samples.
    #[test]
    fn shared_cache_matches_uncached_samples_across_threads() {
        let uncached = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 1102,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let cached = uncached
            .clone()
            .cache()
            .scale(Module::constant(2.0))
            .cache();
        let expected = uncached.scale(Module::constant(2.0));
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let (cached, expected) = (&cached, &expected);
                scope.spawn(move || {
                    for i in 0..4000 {
                        // Repeat each coordinate so threads both hit and overwrite the shared entries.
                        let x = ((i / 2) % 37) as f32 * 3.5 - thread as f32;
                        let y = ((i / 2) % 11) as f32 * -7.25;
                        assert_eq!(cached.value(x, y).to_bits(), expected.value(x, y).to_bits());
                    }
                });
            }
        });
    }

    /// `ReseedOracle`'s lines (`.local/terraforged/oracles/reseed/`): per case, the graph's bounds and sample hash
    /// before reseeding, then one hash of the reseeded graphs' bounds and samples over every seed of
    /// `RESEED_SEEDS`, or `throws` where the jar throws.
    const RESEED_EXPECTED: &str = "\
0 00000000 3f800000 e91c09309db618a1 1c2c96fca6d96fa6
1 00000000 3f800000 29244a0a73be6605 e3bb32846d9ac14f
2 00000000 3f800000 900633b168c83705 1648c5e2aa011e99
3 bfa80000 3fa80000 6233a3168508b036 249875a299bfbfbc
4 00000000 3f800000 61ad1977a568b63d 0ebcbaf0a8a57059
5 00000000 3f800000 9ce5e2ee4e22ae80 0362293e9c402edc
6 00000000 3f800000 8b52bfdbf089282b ea3289cd411e6c0a
7 00000000 3f800000 7165bf4b5abe75a2 4a30f358f0c6a82c
8 00000000 3f800000 875b0ca695870208 a09529692ee43029
9 00000000 3f800000 8815170ee5abbedf e8e68fb43b069e3a
10 00000000 3f800000 fd56c73549b59c6b dc4b97dc61589bc2
11 00000000 3f800000 dd2e5e444884da84 9d7652595dd3a4d5
12 00000000 3f800000 dc38fc486403ca25 90e550ff2df2f841
13 00000000 3f800000 219e9a389a886fea a77e690cb8e69351
14 bf800000 3f800000 b5a8493a4ff67022 489b0337430898cf
15 00000000 3f800000 2a600d8539f3e7e8 6ad50fd5c2c461c5
16 00000000 40000000 b1b87a172498acc4 1c2c96fca6d96fa6
17 00000000 3f800000 b54532f467001127 02c96ccab8c70fc9
18 00000000 3f4ccccd 438032ec84e604ae d656b432ee61303f
19 00000000 3f800000 367054e2b9284e7f 859e016f62379a67
20 c0000000 3f800000 649eaf7cdcc2a553 8490fc7a3164a9f3
21 00000000 3f800000 2a9125e03f690a3f 714a31a0d60e5d03
22 00000000 3f800000 1a7bf72ebeb25cff 41b7b3e1f2928d3a
23 00000000 3f800000 5d1db620ddf86a28 e5aa37ef73f92b98
24 00000000 3f800000 824e2ab1da68bdc5 fd5b8a262ed8ed2b
25 00000000 3f800000 6f4286915210810b 78fb9e21f2e04c06
26 00000000 3f800000 d9a1a0b1324556b8 691b1ca18d463904
27 00000000 3f800000 01f52e573a084a51 fa1dab0b7c14e905
28 00000000 3f800000 419a5cd985a3e484 e81db46db046ff4e
29 00000000 3f800000 2f5a07ed99f03182 f4d1d9f5f37cae31
30 00000000 3f800000 2baefc102778bc68 3c4222e85ba318b3
31 00000000 3f800000 91535aac7b221abe 306289b762893752
32 00000000 3f800000 e510c1ef1178d382 6c9b385a18b08950
33 00000000 3f800000 2349c9d2bb83076e 20072c21aad760d5
34 00000000 3f800000 2349c9d2bb83076e 20072c21aad760d5
35 00000000 3f800000 bdefb9104ca582f8 ef3593a8205cf1ec
36 3f000000 3fc00000 97e18b090ecaa802 465514925f72674b
37 3e800000 3fa00000 431cd5f45b05fba3 3671bf638970a84c
38 3dcccccd 3f8ccccd 0d22f0583c133c3e 6a1bff8f116e6ea3
39 00000000 3f800000 1065a11fc2c6f45c cd65dff18babebad
40 00000000 3f800000 b67a5cb28a3e5172 251359c8af09df4a
41 3e4ccccd 3f99999a 4aa5679dc8b198e9 db039fe19e9011eb
42 00000000 3f800000 44d8c470047d552b 69b71d4e05469cfd
43 00000000 3f800000 962a97e6a7257f09 914093c19186539c
44 00000000 3f800000 c5b0d83ced8ccbe6 7f782a9c44956642
45 00000000 3f800000 b60b86d19dc833bc 2593d9fde08ab8a1
46 00000000 3f800000 e91c09309db618a1 1c2c96fca6d96fa6
47 00000000 3f800000 0405b24cd469bbfa 5a9a46daf3790a06
48 3e99999a 40133333 3796a89a8b965180 ae80f7a2c06fa155
49 00000000 3f800000 1a7bf72ebeb25cff df7aa1c6fa50680d
50 3f800000 3f800000 438032ec84e604ae c18c665f2c51e6f5
51 00000000 00000000 69a65836e98c4ac9 d88b3df3e91876a5
52 bf800000 3f800000 1a17bd3b3397938d 37ab65b3de1f32a5
53 00000000 3f800000 f039a999d83728ee 53bb026964116061
54 80000000 80000000 e436d91b6e3c82b5 1bf8dbae39ad9e45
55 7fc00000 7fc00000 00f480cb81cced78 7c5fb67400fdb7b5
56 3e99999a 3e99999a 96e761c8bd11158b ea007bf8c4e54645
57 3f800000 40133333 38253db8bc22813d 792041e65212bc05
58 00000000 3f800000 e0380ada1528406d a64f255730dbb7b0
59 00000000 3f800000 d3625364c331267c throws
60 00000000 3f800000 e91c09309db618a1 throws
61 00000000 3f800000 c93c7d221a6d805e throws
62 00000000 3f800000 a4330e62085c2c79 throws
63 00000000 3f800000 a8b90acf822d8771 throws
64 00000000 3f800000 52bc36393c579ae6 throws
65 00000000 3f800000 c3c4da91c67ab690 throws
66 00000000 40000000 569932c2ee31f81f d62349505f789e79
67 00000000 3f800000 ac61e43d43f597a4 5a403faf5eda4a5f
68 00000000 3f800000 14b5211f8e072f78 5efd7e7640d71ab5
69 00000000 3f800000 e54a5030cb10abee ea007bf8c4e54645
70 00000000 40000000 b1b87a172498acc4 1c2c96fca6d96fa6
";
    const RESEED_SEEDS: [i64; 6] = [99, 0x1_0000_0063, -1, i64::MIN + 5, i32::MAX as i64, 0];

    /// The oracle's graphs, by index, built as `ReseedOracle.build` builds them.
    fn reseed_case(index: usize) -> Module {
        use Interpolation::{Curve3, Curve4, Linear};
        let src = |kind, seed, frequency, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency,
                    lacunarity: 2.0,
                    interpolation: Curve3,
                },
            ))
        };
        let c = Module::constant;
        let p = || src(SourceKind::Perlin, 12, 1.0 / 80.0, 3, 0.5);
        let q = || src(SourceKind::Simplex, -7, 1.0 / 40.0, 2, 0.5);
        let r = || src(SourceKind::Simplex2, 31, 1.0 / 25.0, 2, 0.6);
        let dw = || Domain::domain_warp(q(), r(), c(20.0));
        let cell = |seed, frequency| SourceConfig {
            seed,
            frequency,
            ..SourceConfig::default()
        };
        let custom = || Module::custom(0.0, 1.0, |_, y| y);
        let interp = CurveFunction::Interpolation;
        let midpoint = |midpoint, steepness| CurveFunction::Midpoint {
            midpoint,
            steepness,
        };
        use DistanceFunction::{Euclidean, Manhattan, Natural};
        match index {
            0 => p(),
            1 => src(SourceKind::Perlin2, i32::MAX, 1.0 / 30.0, 2, 0.5),
            2 => r(),
            3 => src(SourceKind::Cubic, -100, 1.0 / 20.0, 3, 0.5),
            4 => src(SourceKind::Ridge, i32::MIN, 1.0 / 60.0, 4, 0.975),
            5 => src(SourceKind::Billow, 5, 1.0 / 45.0, 3, 0.5),
            6 => src(SourceKind::SimplexRidge, 77, 1.0 / 70.0, 2, 0.975),
            7 => q(),
            8 => Module::cell_source(
                cell(5, 1.0 / 30.0),
                0.3,
                CellFunction::CellValue,
                Euclidean,
                c(0.0),
            ),
            9 => Module::cell_source(
                cell(-9, 1.0 / 50.0),
                0.6,
                CellFunction::NoiseLookup,
                Manhattan,
                p(),
            ),
            10 => Module::cell_edge_source(
                cell(8, 1.0 / 35.0),
                0.3,
                EdgeFunction::Distance2Div,
                Natural,
            ),
            11 => Module::sine_source(0.1, p()),
            12 => Module::line(Line::new(
                1.0,
                2.0,
                300.0,
                400.0,
                q().bias(c(0.25)).scale(c(5000.0)),
                c(0.1),
                c(0.2),
                0.5,
            )),
            13 => Module::erosion_ridge(
                ErosionRidge::full(5, 2, 0.6, 50.0, 0.4, 2.1, 0.7, ErosionMode::InputLinear),
                p().bias(c(0.3)),
            ),
            14 => p().map(c(-1.0), c(1.0)).abs(),
            15 => p().alpha(q()),
            16 => p().bias(q()),
            17 => q().boost(2),
            18 => p().clamp(q(), c(0.8)),
            19 => q().invert(),
            20 => p().map(c(-2.0), q()),
            21 => p().power(q()),
            22 => p().scale(q()),
            23 => p().threshold(q()),
            24 => p().warp(dw()),
            25 => p().warp(Domain::direction(q(), c(15.0))),
            26 => p().warp(dw().then(Domain::direction(r(), c(15.0)))),
            27 => p().base(q(), 0.3, Curve3),
            28 => q().blend(p(), c(0.3), 0.4, 0.2, Curve4),
            29 => q().bias(c(0.5)).blend(p(), r(), 0.4, 0.2, Linear),
            30 => q().variable_blend(p(), c(0.3), r(), 0.4, 0.1, 0.3, Curve3),
            31 => q().multi_blend(2.7, Curve3, vec![p(), c(0.3), r()]),
            32 => q().multi_blend(0.7, Linear, vec![p(), c(0.3), r(), c(0.9)]),
            33 => q().multi_blend(-1.5, Curve3, vec![p(), c(0.3)]),
            34 => q().multi_blend(f32::NAN, Curve3, vec![p(), c(0.3)]),
            35 => q().multi_blend(3e9, Linear, vec![p(), c(0.3)]),
            36 => p().bias(c(0.5)).terrace(c(0.3), q(), c(0.5), 5, 0.3),
            37 => p().bias(c(0.25)).legacy_terrace(c(0.3), q(), 6, 0.4),
            38 => p()
                .bias(c(0.1))
                .advanced_terrace(q(), c(0.3), c(0.2), 0.1, 0.9, 4, 3),
            39 => p().curve(interp(Curve4)),
            40 => p().variable_curve(q(), c(0.3)),
            41 => p().bias(c(0.2)).power_curve(2.5),
            42 => p().steps(q().scale(c(8.0)), c(0.1), c(0.9), interp(Curve3)),
            43 => p().frequency(q(), c(0.3)),
            44 => p().gradient(q(), c(0.3), c(0.5)),
            45 => p().modulate(q(), c(0.3)),
            46 => p().cache(),
            47 => q().select(p(), c(0.3), 0.3, 0.6, 0.1, Curve3),
            48 => Module::add(vec![p(), q().bias(c(0.3))]),
            49 => Module::multiply(vec![p(), q()]),
            50 => Module::max(vec![p(), q()]),
            51 => Module::min(vec![p(), q()]),
            52 => Module::subtract(vec![p(), q()]),
            53 => Module::multiply(vec![p(), q()])
                .cache()
                .blend(
                    p().warp(dw()).boost(1),
                    Module::max(vec![q(), r()]).abs(),
                    0.5,
                    0.3,
                    Curve3,
                )
                .frequency(c(1.5), c(0.75)),
            54 => Module::new_constant(-0.0),
            55 => c(f32::NAN),
            56 => Module::new_constant(-0.0).bias(c(0.3)),
            57 => Module::add(vec![
                Module::max(vec![p().bias(c(0.3)), q()]),
                r().select(p(), q(), 0.4, 0.5, 0.0, Linear),
            ]),
            58 => Module::cell_source(
                cell(3, 1.0 / 40.0),
                1.0,
                CellFunction::NoiseLookup,
                Euclidean,
                Module::add(vec![p(), q()]),
            ),
            59 => Module::custom(0.0, 1.0, |x, _| x),
            60 => p().warp(Domain::direct()),
            61 => p().warp(dw().add(Domain::direction(q(), c(15.0)))),
            62 => p().warp(dw().compound(Domain::direction(q(), c(15.0)))),
            63 => p().curve(midpoint(0.4, 3.0)),
            64 => p().curve(CurveFunction::s_curve(0.2, 0.7)),
            65 => p().steps(q(), c(0.1), c(0.9), midpoint(0.5, 2.0)),
            66 => Module::add(vec![p(), custom()]),
            67 => q().multi_blend(0.5, Linear, vec![p(), custom(), r()]),
            68 => Module::cell_source(
                cell(3, 1.0 / 40.0),
                1.0,
                CellFunction::NoiseLookup,
                Euclidean,
                Module::custom(0.0, 1.0, |_, _| 0.5),
            ),
            69 => Module::multiply(vec![custom()]),
            70 => p().bias(q().warp(Domain::direct())),
            _ => unreachable!(),
        }
    }

    /// FNV-1a over a sample's bits (canonical NaN), continuing `hash`.
    fn reseed_fnv(hash: u64, value: f32) -> u64 {
        let bits = if value.is_nan() {
            0x7fc0_0000_u32
        } else {
            value.to_bits()
        };
        bits.to_le_bytes().iter().fold(hash, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// `ReseedOracle.hash`: `reseed_fnv` over its 261 points.
    fn reseed_hash(hash: u64, module: &Module) -> u64 {
        let points = (0..256_i32).map(|i| {
            (
                ((i * 7919) % 2001 - 1000) as f32 * 0.73,
                ((i * 104_729) % 1999 - 999) as f32 * 1.37,
            )
        });
        let special = [
            (0.0, 0.0),
            (-0.0, -0.0),
            (1e6, -1e6),
            (123_456.75, -0.125),
            (-3.5, 2.5),
        ];
        points
            .chain(special)
            .fold(hash, |hash, (x, y)| reseed_fnv(hash, module.value(x, y)))
    }

    #[test]
    fn reseeding_matches_terraforged_0_3_1() {
        use std::fmt::Write;
        const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        let mut out = String::new();
        for index in 0..71 {
            let module = reseed_case(index);
            let before = reseed_hash(BASIS, &module);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                RESEED_SEEDS.iter().fold(BASIS, |hash, &seed| {
                    let module = module.with_seed(seed);
                    let hash = reseed_fnv(reseed_fnv(hash, module.min_value()), module.max_value());
                    reseed_hash(hash, &module)
                })
            }));
            let reseeded = result.map_or("throws".to_owned(), |hash| format!("{hash:016x}"));
            let (min, max) = (module.min_value().to_bits(), module.max_value().to_bits());
            writeln!(out, "{index} {min:08x} {max:08x} {before:016x} {reseeded}").unwrap();
            assert_eq!(reseed_hash(BASIS, &module), before, "case {index} changed");
        }
        for (line, expected) in out.lines().zip(RESEED_EXPECTED.lines()) {
            assert_eq!(line, expected);
        }
        assert_eq!(out, RESEED_EXPECTED);
    }
}
