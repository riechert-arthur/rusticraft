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

#[derive(Clone, Copy)]
pub enum CurveFunction {
    Interpolation(Interpolation),
    Midpoint { midpoint: f32, steepness: f32 },
    SCurve { lower: f32, upper: f32 },
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
            Self::SCurve { lower, upper } => java_pow(value, lower + upper * value),
        }
    }

    pub fn s_curve(lower: f32, upper: f32) -> Self {
        let upper = if upper < 0.0 {
            java_max(-lower, upper)
        } else {
            upper
        };
        Self::SCurve { lower, upper }
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

pub fn floor(value: f32) -> i32 {
    if value >= 0.0 {
        value as i32
    } else {
        value as i32 - 1
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
    vector: [f32; 2],
    distance: f32,
}

pub fn cell_vector(seed: i32, x: i32, y: i32) -> [f32; 2] {
    CELL_2D[(hash_2d(seed, x, y) & 255) as usize]
}

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
    pub fn new(seed: i32, strength: f32, grid_size: f32, blend_mode: ErosionMode) -> Self {
        Self::with_octaves(seed, 1, strength, grid_size, blend_mode)
    }

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
            1.0 / (octaves + 1) as f32,
            2.25,
            0.75,
            blend_mode,
        )
    }

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

    pub fn wrap(self, source: Module) -> Module {
        Module::erosion_ridge(self, source)
    }

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
        vector: [0.0; 2],
        distance: f32::MAX,
    };
    let mut nearest = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cell_x = xi + dx;
            let cell_y = yi + dy;
            let vector = cell_vector(seed, cell_x, cell_y);
            let delta_x = cell_x as f32 + vector[0] * displacement - x;
            let delta_y = cell_y as f32 + vector[1] * displacement - y;
            let candidate = distance.apply(delta_x, delta_y);
            if candidate < nearest {
                nearest = candidate;
                result = CellResult {
                    x: cell_x,
                    y: cell_y,
                    vector,
                    distance: candidate,
                };
            }
        }
    }
    result
}

pub fn cell_value(x: f32, y: f32, seed: i32, displacement: f32, distance: DistanceFunction) -> f32 {
    let cell = nearest_cell(x, y, seed, displacement, distance);
    value_coord_2d(seed, cell.x, cell.y)
}

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
            let cell_x = xi + dx;
            let cell_y = yi + dy;
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

fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
    a + alpha * (b - a)
}

pub fn perlin(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, false),
        grad(seed, x1, y0, xd1, yd0, false),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, false),
        grad(seed, x1, y1, xd1, yd1, false),
        xs,
    );
    lerp(lower, upper, ys)
}

pub fn perlin2(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, true),
        grad(seed, x1, y0, xd1, yd0, true),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, true),
        grad(seed, x1, y1, xd1, yd1, true),
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
            + corner(seed, i + i1, j + j1, x1, y1)
            + corner(seed, i + 1, j + 1, x2, y2))
}

fn cubic_lerp(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let p = d - c - (a - b);
    t * t * t * p + t * t * (a - b - p) + t * (c - a) + b
}

pub fn cubic(x: f32, y: f32, seed: i32) -> f32 {
    let x1 = floor(x);
    let y1 = floor(y);
    let rows = [-1, 0, 1, 2].map(|dy| {
        let values = [-1, 0, 1, 2].map(|dx| value_coord_2d(seed, x1 + dx, y1 + dy));
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
        Self {
            config,
            kind,
            min,
            max,
            range: max - min,
        }
    }

    pub fn value(self, x: f32, y: f32) -> f32 {
        self.evaluate(x, y, self.config.seed, false)
    }

    pub fn value_seeded(self, x: f32, y: f32, seed: i32) -> f32 {
        self.evaluate(x, y, seed, true)
    }

    pub fn min_value(self) -> f32 {
        if matches!(self.kind, SourceKind::Cubic) {
            self.min
        } else {
            0.0
        }
    }

    pub fn max_value(self) -> f32 {
        if matches!(self.kind, SourceKind::Cubic) {
            self.max
        } else {
            1.0
        }
    }

    fn evaluate(self, mut x: f32, mut y: f32, seed: i32, explicit_seed: bool) -> f32 {
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
            return map(sum, self.min, self.max, self.range);
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
                value += signal * java_pow(spectral_frequency, -1.0);
                spectral_frequency *= self.config.lacunarity;
                x *= self.config.lacunarity;
                y *= self.config.lacunarity;
                amplitude *= self.config.gain;
            }
            let result = map(value, self.min, self.max, self.range);
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
        map(sum, self.min, self.max, self.range)
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
        value += noise * java_pow(spectral_frequency, -1.0);
        spectral_frequency *= config.lacunarity;
        amplitude *= config.gain;
    }
    value
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

pub fn noise_map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let difference = clamp(value, min, max) - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

pub fn noise_curve(value: f32, midpoint: f32, steepness: f32) -> f32 {
    1.0 / (1.0 + noise_exp(-steepness * (value - midpoint)))
}

fn noise_exp(mut value: f32) -> f32 {
    value = 1.0 + value / 256.0;
    for _ in 0..8 {
        value *= value;
    }
    value
}

fn map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let clamped = clamp(value, min, max);
    let difference = clamped - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

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

fn round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

fn hash(x: i32, y: i32) -> i32 {
    let mut hash = x ^ 31337_i32.wrapping_mul(y);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60493);
    (hash >> 13) ^ hash
}

#[derive(Clone)]
pub struct Module(Box<ModuleKind>);

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

#[derive(Clone)]
struct BlendNode {
    source: Module,
    min: f32,
    max: f32,
}

#[derive(Clone)]
struct TerraceStep {
    value: f32,
    lower_bound: f32,
    upper_bound: f32,
}

#[derive(Clone, Copy)]
struct CacheValue {
    x: f32,
    y: f32,
    value: f32,
    empty: bool,
}

impl Default for CacheValue {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            value: 0.0,
            empty: true,
        }
    }
}

#[derive(Clone)]
enum ModuleKind {
    Constant(f32),
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
    Map(Module, Module, Module),
    Power(Module, Module),
    Scale(Module, Module, f32, f32),
    Threshold(Module, Module),
    Warp(Module, Domain),
    Base {
        base: Module,
        source: Module,
        min: f32,
        max: f32,
        falloff: f32,
        max_value: f32,
        interpolation: Interpolation,
    },
    Blend {
        control: Module,
        lower: Module,
        upper: Module,
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
    MultiBlend {
        control: Module,
        nodes: Vec<BlendNode>,
        max_index: usize,
        blend_range: f32,
        interpolation: Interpolation,
    },
    Terrace {
        source: Module,
        ramp: Module,
        cliff: Module,
        ramp_height: Module,
        steps: Vec<TerraceStep>,
        max_index: usize,
    },
    LegacyTerrace {
        source: Module,
        lower_curve: Module,
        upper_curve: Module,
        steps: Vec<TerraceStep>,
        max_index: usize,
    },
    AdvancedTerrace {
        source: Module,
        modulation: Module,
        mask: Module,
        slope: Module,
        blend_min: f32,
        blend_max: f32,
        blend_range: f32,
        mod_range: f32,
        steps: usize,
        octaves: usize,
    },
    Curve {
        source: Module,
        function: CurveFunction,
    },
    VariableCurve {
        source: Module,
        midpoint: Module,
        gradient: Module,
    },
    PowerCurve {
        source: Module,
        min: f32,
        max: f32,
        midpoint: f32,
        range: f32,
        power: f32,
    },
    Steps {
        source: Module,
        steps: Module,
        slope_min: Module,
        slope_max: Module,
        curve: CurveFunction,
    },
    Frequency {
        source: Module,
        x: Module,
        y: Module,
    },
    Gradient {
        source: Module,
        lower: Module,
        upper: Module,
        strength: Module,
    },
    Modulate {
        source: Module,
        direction: Module,
        strength: Module,
    },
    Cache {
        source: Module,
        value: std::sync::Arc<std::sync::Mutex<CacheValue>>,
    },
    Select {
        control: Module,
        lower: Module,
        upper: Module,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
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

    #[allow(clippy::too_many_arguments)]
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

    #[allow(clippy::too_many_arguments)]
    pub fn distance_on_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let dx = bx - ax;
        let dy = by - ay;
        ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn distance2_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let dx = bx - ax;
        let dy = by - ay;
        let projection = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy);
        if projection < 0.0 || projection > 1.0 {
            return -1.0;
        }
        Self::dist2(x, y, ax + dx * projection, ay + dy * projection)
    }

    #[allow(clippy::too_many_arguments)]
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

    #[allow(clippy::too_many_arguments)]
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
    pub fn constant(value: f32) -> Self {
        Self(Box::new(ModuleKind::Constant(value)))
    }

    pub fn source(source: Source) -> Self {
        Self(Box::new(ModuleKind::Source(source)))
    }

    pub fn erosion_ridge(ridge: ErosionRidge, source: Self) -> Self {
        Self(Box::new(ModuleKind::ErosionRidge { ridge, source }))
    }

    pub fn cell_source(
        config: SourceConfig,
        displacement: f32,
        function: CellFunction,
        distance: DistanceFunction,
        lookup: Self,
    ) -> Self {
        Self(Box::new(ModuleKind::CellSource {
            config,
            displacement,
            function,
            distance,
            lookup,
        }))
    }

    pub fn cell_edge_source(
        config: SourceConfig,
        displacement: f32,
        edge: EdgeFunction,
        distance: DistanceFunction,
    ) -> Self {
        Self(Box::new(ModuleKind::CellEdgeSource {
            config,
            displacement,
            edge,
            distance,
        }))
    }

    pub fn sine_source(frequency: f32, alpha: Self) -> Self {
        Self(Box::new(ModuleKind::SineSource { frequency, alpha }))
    }

    pub fn line(line: Line) -> Self {
        Self(Box::new(ModuleKind::Line(line)))
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.value(x, y),
            ModuleKind::ErosionRidge { ridge, source } => ridge.value(x, y, source),
            ModuleKind::CellSource {
                config,
                displacement,
                function,
                distance,
                lookup,
            } => {
                let x = x * config.frequency;
                let y = y * config.frequency;
                let cell = nearest_cell(x, y, config.seed, *displacement, *distance);
                match function {
                    CellFunction::CellValue => {
                        noise_map(value_coord_2d(config.seed, cell.x, cell.y), -1.0, 1.0, 2.0)
                    }
                    CellFunction::NoiseLookup => lookup.value(
                        cell.x as f32 + cell.vector[0],
                        cell.y as f32 + cell.vector[1],
                    ),
                    CellFunction::Distance => {
                        let _ = cell.distance - 1.0;
                        0.0
                    }
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
                noise_map(value, min, max, range)
            }
            ModuleKind::SineSource { frequency, alpha } => {
                let alpha = alpha.value(x, y);
                let x = x * *frequency;
                let y = y * *frequency;
                let noise = if alpha == 0.0 {
                    sin_lookup(x)
                } else if alpha == 1.0 {
                    sin_lookup(y)
                } else {
                    lerp(sin_lookup(x), sin_lookup(y), alpha)
                };
                noise_map(noise, -1.0, 1.0, 2.0)
            }
            ModuleKind::Line(line) => line.value(x, y),
            ModuleKind::Abs(source) => source.value(x, y).abs(),
            ModuleKind::Alpha(source, alpha) => {
                let alpha = alpha.value(x, y);
                source.value(x, y) * alpha + (1.0 - alpha)
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
                clamp(source.value(x, y), min.value(x, y), max.value(x, y))
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
            ModuleKind::Map(source, min, max) => {
                let alpha = (source.value(x, y) - source.min_value())
                    / (source.max_value() - source.min_value());
                let min = min.value(x, y);
                let max = max.value(x, y);
                min + alpha * (max - min)
            }
            ModuleKind::Power(source, power) => java_pow(source.value(x, y), power.value(x, y)),
            ModuleKind::Scale(source, scale, ..) => source.value(x, y) * scale.value(x, y),
            ModuleKind::Threshold(source, threshold) => {
                if source.value(x, y) < threshold.value(x, y) {
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
                ..
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
            } => {
                let selector = control.value(x, y);
                let index = round(selector * *max_index as f32) as usize;
                let mut min = &nodes[index];
                let mut max = min;
                if *blend_range == 0.0 {
                    return min.source.value(x, y);
                }
                if selector > min.max {
                    max = &nodes[index + 1];
                } else if selector < min.min {
                    min = &nodes[index - 1];
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
            } => {
                let noise_value = clamp(source.value(x, y), 0.0, 0.999999);
                let index = floor(noise_value * steps.len() as f32) as usize;
                let step = &steps[index];
                if index == *max_index || noise_value < step.lower_bound {
                    return step.value;
                }
                if noise_value > step.upper_bound {
                    return steps[index + 1].value;
                }
                let ramp = 1.0 - ramp.value(x, y) * 0.5;
                let cliff = 1.0 - cliff.value(x, y) * 0.5;
                let alpha =
                    (noise_value - step.lower_bound) / (step.upper_bound - step.lower_bound);
                let mut value = step.value;
                if alpha > ramp {
                    let next = &steps[index + 1];
                    let ramp_size = 1.0 - ramp;
                    let ramp_alpha = (alpha - ramp) / ramp_size;
                    value += (next.value - value) * ramp_alpha * ramp_height.value(x, y);
                }
                if alpha > cliff {
                    let next = &steps[index + 1];
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
            } => {
                let noise_value = clamp(source.value(x, y), 0.0, 1.0);
                let index = round(noise_value * *max_index as f32) as usize;
                let step = &steps[index];
                if noise_value < step.lower_bound && index > 0 {
                    let lower = &steps[index - 1];
                    let alpha =
                        (noise_value - lower.upper_bound) / (step.lower_bound - lower.upper_bound);
                    let alpha = 1.0 - Interpolation::Curve3.apply(alpha);
                    let range = step.value - lower.value;
                    return step.value - alpha * range * upper_curve.value(x, y);
                }
                if noise_value > step.upper_bound && index < *max_index {
                    let upper = &steps[index + 1];
                    let alpha =
                        (noise_value - step.upper_bound) / (upper.lower_bound - step.upper_bound);
                    let alpha = Interpolation::Curve3.apply(alpha);
                    let range = upper.value - step.value;
                    return step.value + alpha * range * lower_curve.value(x, y);
                }
                step.value
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
                let value = source.value(x, y);
                if value <= *blend_min {
                    return value;
                }
                let mask = mask.value(x, y);
                if mask == 0.0 {
                    return value;
                }
                let slope = slope.value(x, y);
                let modulation = modulation.value(x, y);
                let mut result = value;
                for octave in 1..=*octaves {
                    let step_count = *steps * octave;
                    result = round(result * step_count as f32) as f32 / step_count as f32;
                    result += (value - result) * slope;
                }
                result = (result + modulation) / *mod_range;
                let mut alpha = if value > *blend_max {
                    1.0
                } else {
                    (value - *blend_min) / *blend_range
                };
                if mask != 1.0 {
                    alpha *= mask;
                }
                lerp(value, result, alpha)
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
            } => {
                let value = source.value(x, y);
                let value = if value >= *midpoint {
                    *midpoint + java_pow(value - *midpoint, *power)
                } else {
                    *midpoint - java_pow(*midpoint - value, *power)
                };
                noise_map(value, *min, *max, *range)
            }
            ModuleKind::Steps {
                source,
                steps,
                slope_min,
                slope_max,
                curve,
            } => {
                let noise_value = source.value(x, y);
                let min = slope_min.value(x, y);
                let max = slope_max.value(x, y);
                let step_count = steps.value(x, y);
                let range = max - min;
                if range <= 0.0 {
                    return (noise_value * step_count) as i32 as f32 / step_count;
                }
                let inverted = 1.0 - noise_value;
                let value = (inverted * step_count) as i32 as f32 / step_count;
                let delta = inverted - value;
                let alpha = noise_map(delta * step_count, min, max, range);
                1.0 - lerp(value, inverted, curve.apply(alpha))
            }
            ModuleKind::Frequency {
                source,
                x: x_frequency,
                y: y_frequency,
            } => {
                let x_frequency = x_frequency.value(x, y);
                let y_frequency = y_frequency.value(x, y);
                source.value(x * x_frequency, y * y_frequency)
            }
            ModuleKind::Gradient {
                source,
                lower,
                upper,
                strength,
            } => {
                let noise_value = source.value(x, y);
                let upper_bound = upper.value(x, y);
                if noise_value > upper_bound {
                    return noise_value;
                }
                let amount = strength.value(x, y);
                let lower_bound = lower.value(x, y);
                if noise_value < lower_bound {
                    return java_pow(noise_value, 1.0 - amount);
                }
                let alpha = 1.0 - (noise_value - lower_bound) / (upper_bound - lower_bound);
                java_pow(noise_value, 1.0 - amount * alpha)
            }
            ModuleKind::Modulate {
                source,
                direction,
                strength,
            } => {
                let angle = direction.value(x, y) * java_pi2();
                let strength = strength.value(x, y);
                let dx = sin_lookup(angle) * strength;
                let dy = sin_lookup(angle + COS_OFFSET) * strength;
                source.value(x + dx, y + dy)
            }
            ModuleKind::Cache { source, value } => {
                let mut cached = value.lock().expect("TerraForged cache lock poisoned");
                if !cached.empty && x == cached.x && y == cached.y {
                    return cached.value;
                }
                let result = source.value(x, y);
                *cached = CacheValue {
                    x,
                    y,
                    value: result,
                    empty: false,
                };
                result
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                lower_bound,
                upper_bound,
                falloff,
                interpolation,
            } => select_value(
                control.value(x, y),
                lower,
                upper,
                x,
                y,
                *lower_bound,
                *upper_bound,
                *falloff,
                *interpolation,
            ),
            ModuleKind::Add(sources) => combine(sources, x, y, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine(sources, x, y, |a, b| a * b),
            ModuleKind::Max(sources) => combine(sources, x, y, java_max),
            ModuleKind::Min(sources) => combine(sources, x, y, java_min),
            ModuleKind::Subtract(sources) => combine(sources, x, y, |a, b| a - b),
        }
    }

    pub fn min_value(&self) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.min_value(),
            ModuleKind::ErosionRidge { .. } => 0.0,
            ModuleKind::CellSource { .. }
            | ModuleKind::CellEdgeSource { .. }
            | ModuleKind::SineSource { .. }
            | ModuleKind::Line(_) => 0.0,
            ModuleKind::Bias(source, bias) => source.min_value() + bias.min_value(),
            ModuleKind::Boost(..) => 0.0,
            ModuleKind::Clamp(_, min, _) | ModuleKind::Map(_, min, _) => min.min_value(),
            ModuleKind::Scale(_, _, min, _) => *min,
            ModuleKind::Base { base, .. } => base.min_value(),
            ModuleKind::Blend { lower, upper, .. }
            | ModuleKind::VariableBlend { lower, upper, .. }
            | ModuleKind::Select { lower, upper, .. } => {
                java_min(lower.min_value(), upper.min_value())
            }
            ModuleKind::MultiBlend { nodes, .. } => nodes[1..]
                .iter()
                .fold(nodes[0].source.min_value(), |value, node| {
                    java_min(value, node.source.min_value())
                }),
            ModuleKind::Terrace { source, .. }
            | ModuleKind::LegacyTerrace { source, .. }
            | ModuleKind::AdvancedTerrace { source, .. }
            | ModuleKind::Curve { source, .. }
            | ModuleKind::VariableCurve { source, .. }
            | ModuleKind::PowerCurve { source, .. }
            | ModuleKind::Steps { source, .. }
            | ModuleKind::Frequency { source, .. }
            | ModuleKind::Gradient { source, .. }
            | ModuleKind::Modulate { source, .. }
            | ModuleKind::Cache { source, .. } => source.min_value(),
            ModuleKind::Add(sources) => combine_bounds(sources, true, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine_bounds(sources, true, |a, b| a * b),
            ModuleKind::Max(sources) => {
                let Some((first, rest)) = sources.split_first() else {
                    return 0.0;
                };
                rest.iter().fold(first.min_value(), |value, source| {
                    java_max(value, source.max_value())
                })
            }
            ModuleKind::Min(sources) => combine_bounds(sources, true, java_min),
            ModuleKind::Subtract(sources) => {
                let Some((first, rest)) = sources.split_first() else {
                    return 0.0;
                };
                rest.iter().fold(first.min_value(), |value, source| {
                    value - source.max_value()
                })
            }
            ModuleKind::Abs(source)
            | ModuleKind::Alpha(source, _)
            | ModuleKind::Invert(source)
            | ModuleKind::Power(source, _)
            | ModuleKind::Threshold(source, _)
            | ModuleKind::Warp(source, _) => source.min_value(),
        }
    }

    pub fn max_value(&self) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.max_value(),
            ModuleKind::ErosionRidge { .. } => 1.0,
            ModuleKind::CellSource { .. }
            | ModuleKind::CellEdgeSource { .. }
            | ModuleKind::SineSource { .. }
            | ModuleKind::Line(_) => 1.0,
            ModuleKind::Bias(source, bias) => source.max_value() + bias.max_value(),
            ModuleKind::Boost(..) => 1.0,
            ModuleKind::Clamp(_, _, max) | ModuleKind::Map(_, _, max) => max.max_value(),
            ModuleKind::Scale(_, _, _, max) => *max,
            ModuleKind::Base { max_value, .. } => *max_value,
            ModuleKind::Blend { lower, upper, .. }
            | ModuleKind::VariableBlend { lower, upper, .. }
            | ModuleKind::Select { lower, upper, .. } => {
                java_max(lower.max_value(), upper.max_value())
            }
            ModuleKind::MultiBlend { nodes, .. } => nodes[1..]
                .iter()
                .fold(nodes[0].source.max_value(), |value, node| {
                    java_max(value, node.source.max_value())
                }),
            ModuleKind::Terrace { source, .. }
            | ModuleKind::LegacyTerrace { source, .. }
            | ModuleKind::AdvancedTerrace { source, .. }
            | ModuleKind::Curve { source, .. }
            | ModuleKind::VariableCurve { source, .. }
            | ModuleKind::PowerCurve { source, .. }
            | ModuleKind::Steps { source, .. }
            | ModuleKind::Frequency { source, .. }
            | ModuleKind::Gradient { source, .. }
            | ModuleKind::Modulate { source, .. }
            | ModuleKind::Cache { source, .. } => source.max_value(),
            ModuleKind::Add(sources) => combine_bounds(sources, false, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine_bounds(sources, false, |a, b| a * b),
            ModuleKind::Max(sources) => combine_bounds(sources, false, java_max),
            ModuleKind::Min(sources) => {
                let Some((first, rest)) = sources.split_first() else {
                    return 0.0;
                };
                rest.iter().fold(first.max_value(), |value, source| {
                    java_min(value, source.min_value())
                })
            }
            ModuleKind::Subtract(sources) => {
                let Some((first, rest)) = sources.split_first() else {
                    return 0.0;
                };
                rest.iter().fold(first.max_value(), |value, source| {
                    value - source.min_value()
                })
            }
            ModuleKind::Abs(source)
            | ModuleKind::Alpha(source, _)
            | ModuleKind::Invert(source)
            | ModuleKind::Power(source, _)
            | ModuleKind::Threshold(source, _)
            | ModuleKind::Warp(source, _) => source.max_value(),
        }
    }

    pub fn abs(self) -> Self {
        if matches!(self.0.as_ref(), ModuleKind::Abs(_)) {
            self
        } else {
            Self(Box::new(ModuleKind::Abs(self)))
        }
    }

    pub fn alpha(self, alpha: Self) -> Self {
        if alpha.min_value() < 0.0 || alpha.max_value() > 1.0 {
            self
        } else {
            Self(Box::new(ModuleKind::Alpha(self, alpha)))
        }
    }

    pub fn bias(self, bias: Self) -> Self {
        if bias.min_value() == 0.0 && bias.max_value() == 0.0 {
            self
        } else {
            Self(Box::new(ModuleKind::Bias(self, bias)))
        }
    }

    pub fn boost(self, iterations: usize) -> Self {
        if iterations == 0 {
            self
        } else {
            let mapped = self.map(Self::constant(0.0), Self::constant(1.0));
            Self(Box::new(ModuleKind::Boost(mapped, iterations)))
        }
    }

    pub fn clamp(self, min: Self, max: Self) -> Self {
        if min.min_value() == min.max_value()
            && min.min_value() == self.min_value()
            && max.min_value() == max.max_value()
            && max.max_value() == self.max_value()
        {
            self
        } else {
            Self(Box::new(ModuleKind::Clamp(self, min, max)))
        }
    }

    pub fn invert(self) -> Self {
        if matches!(self.0.as_ref(), ModuleKind::Invert(_)) {
            self
        } else {
            Self(Box::new(ModuleKind::Invert(self)))
        }
    }

    pub fn map(self, min: Self, max: Self) -> Self {
        Self(Box::new(ModuleKind::Map(self, min, max)))
    }

    pub fn power(self, power: Self) -> Self {
        Self(Box::new(ModuleKind::Power(self, power)))
    }

    pub fn scale(self, scale: Self) -> Self {
        if scale.min_value() == 1.0 && scale.max_value() == 1.0 {
            return self;
        }
        let min = self.min_value() * scale.min_value();
        let max = self.max_value() * scale.max_value();
        Self(Box::new(ModuleKind::Scale(self, scale, min, max)))
    }

    pub fn threshold(self, threshold: Self) -> Self {
        Self(Box::new(ModuleKind::Threshold(self, threshold)))
    }

    pub fn warp(self, domain: Domain) -> Self {
        Self(Box::new(ModuleKind::Warp(self, domain)))
    }

    pub fn base(self, source: Self, falloff: f32, interpolation: Interpolation) -> Self {
        let min = self.max_value();
        let max = min + falloff;
        let max_value = java_max(self.max_value(), source.max_value());
        Self(Box::new(ModuleKind::Base {
            base: self,
            source,
            min,
            max,
            falloff,
            max_value,
            interpolation,
        }))
    }

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
        Self(Box::new(ModuleKind::Blend {
            control: self,
            lower,
            upper,
            blend_lower,
            blend_upper,
            blend_range: blend_upper - blend_lower,
            interpolation,
        }))
    }

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
        Self(Box::new(ModuleKind::VariableBlend {
            control: self,
            variable,
            lower,
            upper,
            midpoint,
            min_blend,
            max_blend,
            interpolation,
        }))
    }

    pub fn multi_blend(self, blend: f32, interpolation: Interpolation, sources: Vec<Self>) -> Self {
        let spacing = 1.0 / sources.len() as f32;
        let radius = spacing / 2.0;
        let blend_range = radius * blend;
        let cell_radius = (radius - blend_range) / 2.0;
        let max_index = sources.len() - 1;
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
                let max = if index == max_index {
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
        Self(Box::new(ModuleKind::MultiBlend {
            control: self,
            nodes,
            max_index,
            blend_range,
            interpolation,
        }))
    }

    pub fn terrace(
        self,
        ramp: Self,
        cliff: Self,
        ramp_height: Self,
        step_count: usize,
        blend_range: f32,
    ) -> Self {
        let max_index = step_count - 1;
        let range = self.max_value() - self.min_value();
        let spacing = range / max_index as f32;
        let steps = (0..step_count)
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
        Self(Box::new(ModuleKind::Terrace {
            source: self,
            ramp,
            cliff,
            ramp_height,
            steps,
            max_index,
        }))
    }

    pub fn legacy_terrace(
        self,
        lower_curve: Self,
        upper_curve: Self,
        step_count: usize,
        blend_range: f32,
    ) -> Self {
        let max_index = step_count - 1;
        let spacing = (self.max_value() - self.min_value()) / max_index as f32;
        let steps = (0..step_count)
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
        Self(Box::new(ModuleKind::LegacyTerrace {
            source: self,
            lower_curve,
            upper_curve,
            steps,
            max_index,
        }))
    }

    pub fn advanced_terrace(
        self,
        modulation: Self,
        mask: Self,
        slope: Self,
        blend_min: f32,
        blend_max: f32,
        steps: usize,
        octaves: usize,
    ) -> Self {
        let mod_range = self.max_value() + modulation.max_value();
        Self(Box::new(ModuleKind::AdvancedTerrace {
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
        }))
    }

    pub fn curve(self, function: CurveFunction) -> Self {
        Self(Box::new(ModuleKind::Curve {
            source: self,
            function,
        }))
    }

    pub fn variable_curve(self, midpoint: Self, gradient: Self) -> Self {
        Self(Box::new(ModuleKind::VariableCurve {
            source: self,
            midpoint,
            gradient,
        }))
    }

    pub fn power_curve(self, power: f32) -> Self {
        let source_min = self.min_value();
        let source_max = self.max_value();
        let source_midpoint = source_min + (source_max - source_min) / 2.0;
        let min = source_midpoint - java_pow(source_midpoint - source_min, power);
        let max = source_midpoint + java_pow(source_max - source_midpoint, power);
        let range = max - min;
        Self(Box::new(ModuleKind::PowerCurve {
            source: self,
            min,
            max,
            midpoint: min + range / 2.0,
            range,
            power,
        }))
    }

    pub fn steps(
        self,
        steps: Self,
        slope_min: Self,
        slope_max: Self,
        curve: CurveFunction,
    ) -> Self {
        Self(Box::new(ModuleKind::Steps {
            source: self,
            steps,
            slope_min,
            slope_max,
            curve,
        }))
    }

    pub fn frequency(self, x: Self, y: Self) -> Self {
        Self(Box::new(ModuleKind::Frequency { source: self, x, y }))
    }

    pub fn gradient(self, lower: Self, upper: Self, strength: Self) -> Self {
        Self(Box::new(ModuleKind::Gradient {
            source: self,
            lower,
            upper,
            strength,
        }))
    }

    pub fn modulate(self, direction: Self, strength: Self) -> Self {
        Self(Box::new(ModuleKind::Modulate {
            source: self,
            direction,
            strength,
        }))
    }

    pub fn cache(self) -> Self {
        Self(Box::new(ModuleKind::Cache {
            source: self,
            value: std::sync::Arc::new(std::sync::Mutex::new(CacheValue::default())),
        }))
    }

    pub fn select(
        self,
        lower: Self,
        upper: Self,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        interpolation: Interpolation,
    ) -> Self {
        Self(Box::new(ModuleKind::Select {
            control: self,
            lower,
            upper,
            lower_bound,
            upper_bound,
            falloff,
            interpolation,
        }))
    }

    pub fn add(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Add(sources)))
    }

    pub fn multiply(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Multiply(sources)))
    }

    pub fn max(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Max(sources)))
    }

    pub fn min(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Min(sources)))
    }

    pub fn subtract(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Subtract(sources)))
    }
}

fn select_value(
    value: f32,
    lower: &Module,
    upper: &Module,
    x: f32,
    y: f32,
    lower_bound: f32,
    upper_bound: f32,
    falloff: f32,
    interpolation: Interpolation,
) -> f32 {
    let lower_min = lower_bound - falloff;
    let lower_max = lower_bound + falloff;
    let upper_min = upper_bound - falloff;
    let upper_max = upper_bound + falloff;
    if falloff == 0.0 {
        return if value > upper_min {
            upper.value(x, y)
        } else {
            lower.value(x, y)
        };
    }
    if value < lower_min {
        lower.value(x, y)
    } else if value < lower_max {
        let alpha = (value - lower_min) / (lower_max - lower_min);
        lerp(
            lower.value(x, y),
            upper.value(x, y),
            interpolation.apply(alpha),
        )
    } else if value < upper_min {
        upper.value(x, y)
    } else if value < upper_max {
        let alpha = (value - upper_min) / (upper_max - upper_min);
        lerp(
            upper.value(x, y),
            lower.value(x, y),
            interpolation.apply(alpha),
        )
    } else {
        lower.value(x, y)
    }
}

#[derive(Clone)]
pub struct Domain(Box<DomainKind>);

#[derive(Clone)]
enum DomainKind {
    Direct,
    DomainWarp {
        x: Module,
        y: Module,
        distance: Module,
    },
    Direction {
        direction: Module,
        strength: Module,
    },
    Add(Domain, Domain),
    Compound(Domain, Domain),
    Cumulative(Domain, Domain),
}

impl Domain {
    pub fn direct() -> Self {
        Self(Box::new(DomainKind::Direct))
    }

    pub fn domain_warp(x: Module, y: Module, distance: Module) -> Self {
        let map_axis = |axis: Module| {
            if axis.min_value() == -0.5 && axis.max_value() == 0.5 {
                axis
            } else {
                axis.map(Module::constant(-0.5), Module::constant(0.5))
            }
        };
        Self(Box::new(DomainKind::DomainWarp {
            x: map_axis(x),
            y: map_axis(y),
            distance,
        }))
    }

    pub fn direction(direction: Module, strength: Module) -> Self {
        Self(Box::new(DomainKind::Direction {
            direction,
            strength,
        }))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Add(self, next)))
    }

    pub fn compound(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Compound(self, next)))
    }

    pub fn then(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Cumulative(self, next)))
    }

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
                let angle = direction.value(x, y) * java_pi2();
                sin_lookup(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_x(x, y) + second.offset_x(x, y),
            DomainKind::Compound(first, second) => second.offset_x(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.x(first.x(x, y), first.y(x, y)),
        }
    }

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
                let angle = direction.value(x, y) * java_pi2();
                sin_lookup(angle + COS_OFFSET) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_y(x, y) + second.offset_y(x, y),
            DomainKind::Compound(first, second) => second.offset_y(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.y(first.x(x, y), first.y(x, y)),
        }
    }

    pub fn x(&self, x: f32, y: f32) -> f32 {
        x + self.offset_x(x, y)
    }

    pub fn y(&self, x: f32, y: f32) -> f32 {
        y + self.offset_y(x, y)
    }
}

fn combine(sources: &[Module], x: f32, y: f32, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first.value(x, y), |value, source| {
        operation(value, source.value(x, y))
    })
}

fn combine_bounds(sources: &[Module], minimum: bool, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    let initial = if minimum {
        first.min_value()
    } else {
        first.max_value()
    };
    rest.iter().fold(initial, |value, source| {
        operation(
            value,
            if minimum {
                source.min_value()
            } else {
                source.max_value()
            },
        )
    })
}

fn java_pow(value: f32, power: f32) -> f32 {
    (f64::from(value).powf(f64::from(power))) as f32
}

fn java_max(first: f32, second: f32) -> f32 {
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

fn java_min(first: f32, second: f32) -> f32 {
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

fn java_pi2() -> f32 {
    (std::f64::consts::PI * 2.0) as f32
}

const COS_OFFSET: f32 = f32::from_bits(0x3fc9_0ff9);

pub fn sin_lookup(radians: f32) -> f32 {
    const SIN_COUNT: f32 = 4096.0;
    const SIN_MASK: i32 = 4095;
    let radians_full = java_pi2();
    let index = ((radians * (SIN_COUNT / radians_full)) as i32 & SIN_MASK) as usize;
    if index & 1023 == 0 {
        let degrees = (index / 1024 * 90) as f64;
        (degrees.to_radians().sin()) as f32
    } else {
        let angle = (index as f32 + 0.5) / SIN_COUNT * radians_full;
        f64::from(angle).sin() as f32
    }
}

pub fn cos_lookup(radians: f32) -> f32 {
    sin_lookup(radians + COS_OFFSET)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
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
        let cubic = Source::new(SourceKind::Cubic, config);
        assert_eq!(cubic.min_value().to_bits(), 0xbfb0fa58);
        assert_eq!(cubic.max_value().to_bits(), 0x3fb0fa58);
        let perlin = Source::new(SourceKind::Perlin, config);
        assert_eq!(perlin.min_value().to_bits(), 0);
        assert_eq!(perlin.max_value().to_bits(), 0x3f800000);
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
    fn line_source_matches_engine_0_3_0() {
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
        let line = Line::new(
            -10.0,
            5.0,
            30.0,
            25.0,
            radius,
            Module::constant(0.2),
            Module::constant(0.3),
            0.4,
        );
        let module = Module::line(line.clone());
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0, 0x3f800000]
        );
        let points = [
            (-10.0, 5.0),
            (30.0, 25.0),
            (10.0, 15.0),
            (10.0, 20.0),
            (-15.0, 2.0),
            (35.0, 28.0),
            (0.0, 0.0),
            (22.0, 14.0),
        ];
        let expected = [
            [0xffc00000, 0, 0x3f570a3e, 0x3f570a3e, 0, 0],
            [0xffc00000, 0, 0x3f570a3e, 0x3f570a3e, 0, 0],
            [0x3f6eeef0, 0x3f555555, 0x3f570a3e, 0x3f570a3e, 0, 0],
            [0x3f096fb8, 0x3f355555, 0x3ee6ee27, 0x3ef7cf76, 0, 0],
            [0, 0x3dae147b, 0, 0x3c34235f, 1, 0],
            [0, 0x3d681b4f, 0, 0, 0, 1],
            [0, 0x3ea00000, 0, 0, 0, 0],
            [0, 0x3e9dddde, 0x3d01b31a, 0x3dd4df5f, 0, 0],
        ];
        assert_eq!(
            points.map(|(x, y)| [
                line.value(x, y).to_bits(),
                line.width_modifier(x, y).to_bits(),
                line.value_with_width(x, y, 0.6).to_bits(),
                line.value_with_min_width(x, y, 4.0, 0.6).to_bits(),
                line.clip_start(x, y) as u32,
                line.clip_end(x, y) as u32,
            ]),
            expected
        );
        assert_eq!(Line::dist2(1.0, 2.0, 4.0, 6.0).to_bits(), 0x41c80000);
        assert_eq!(Line::sign(2.0, 3.0, 0.0, 0.0, 4.0, 4.0), -1);
        assert!(Line::intersect(0.0, 0.0, 4.0, 4.0, 0.0, 4.0, 4.0, 0.0));
        assert_eq!(
            Line::distance_on_line(2.0, 3.0, 0.0, 0.0, 4.0, 4.0).to_bits(),
            0x3f200000
        );
        assert_eq!(
            Line::distance2_line(2.0, 3.0, 0.0, 0.0, 4.0, 4.0).to_bits(),
            0x3f000000
        );
        assert_eq!(
            Line::distance2_line_including_ends(8.0, 9.0, 0.0, 0.0, 4.0, 4.0).to_bits(),
            0x42240000
        );
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
            bits(points.map(|(x, y)| warped.value(x, y))),
            [0x3f4fea9c, 0x3f2d9330, 0x3f4b5329, 0x3f53bd2e]
        );
    }

    #[test]
    fn selectors_match_engine_0_3_0() {
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
    fn terrace_matches_engine_0_3_0() {
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
        let module = source(SourceKind::Perlin, 601, 96.0, 4, 0.5).terrace(
            source(SourceKind::Simplex, 602, 72.0, 3, 0.5),
            source(SourceKind::Ridge, 603, 128.0, 3, 0.975),
            source(SourceKind::Perlin, 604, 80.0, 2, 0.5),
            7,
            0.35,
        );
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
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0x00000000, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f017a74,
                0x3e2aaaab,
            ]
        );

        let bounded =
            |value| Module::constant(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let values = [
            -1.0, 0.0, 0.05416666, 0.05416667, 0.1, 0.11249999, 0.1125, 0.15, 0.16666667, 0.2,
            0.22083333, 0.22083335, 0.999999, 1.0, 2.0,
        ];
        assert_eq!(
            values.map(|value| {
                bounded(value)
                    .terrace(
                        Module::constant(1.0),
                        Module::constant(1.0),
                        Module::constant(0.5),
                        7,
                        0.35,
                    )
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [
                0x00000000, 0x00000000, 0x3e2aaaaa, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab,
                0x3e2aaaab, 0x3e2aaaad, 0x3e93f1de, 0x3eaaaaab, 0x3eaaaaab, 0x3f800000, 0x3f800000,
                0x3f800000,
            ]
        );
    }

    #[test]
    fn legacy_terrace_matches_engine_0_3_0() {
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
        let module = source(SourceKind::Perlin, 701, 96.0, 4, 0.5).legacy_terrace(
            source(SourceKind::Simplex, 702, 72.0, 3, 0.5),
            source(SourceKind::Ridge, 703, 128.0, 3, 0.975),
            7,
            0.35,
        );
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
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3ebe6cf3, 0x3f296077, 0x3f000000, 0x3e91b23e, 0x3f03e9a3, 0x3f2aaaab,
                0x3f000000,
            ]
        );
        let bounded =
            |value| Module::constant(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let values = [
            -1.0, 0.0, 0.05416666, 0.05416667, 0.1, 0.1125, 0.15, 0.16666667, 0.2, 0.22083333,
            0.22083335, 0.25, 0.999999, 1.0, 2.0,
        ];
        assert_eq!(
            values.map(|value| bounded(value)
                .legacy_terrace(Module::constant(0.25), Module::constant(0.75), 7, 0.35)
                .value(0.0, 0.0)
                .to_bits()),
            [
                0, 0, 0, 0x2612f052, 0x3e1b8d8f, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab,
                0x3e2aaaab, 0x3e2aaaab, 0x3e8aaaab, 0x3f800000, 0x3f800000, 0x3f800000,
            ]
        );
    }

    #[test]
    fn advanced_terrace_matches_engine_0_3_0() {
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
        let module = source(SourceKind::Perlin, 801, 96.0, 4, 0.5).advanced_terrace(
            source(SourceKind::Simplex, 802, 80.0, 3, 0.5),
            source(SourceKind::Ridge, 803, 128.0, 2, 0.975),
            source(SourceKind::Perlin, 804, 64.0, 2, 0.5),
            0.2,
            0.8,
            8,
            3,
        );
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
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3ec5e4a8, 0x3f110a8b, 0x3eec7491, 0x3ecdb230, 0x3f1fed43, 0x3f210ae1,
                0x3f2f009a,
            ]
        );
        let bounded =
            |value| Module::constant(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let values = [0.1, 0.2, 0.20000002, 0.5, 0.8, 0.8000001, 1.0];
        assert_eq!(
            values.map(|value| bounded(value)
                .advanced_terrace(
                    Module::constant(0.1),
                    Module::constant(0.5),
                    Module::constant(0.25),
                    0.2,
                    0.8,
                    8,
                    3,
                )
                .value(0.0, 0.0)
                .to_bits()),
            [
                0x3dcccccd, 0x3e4ccccd, 0x3e4cccce, 0x3f02e8ba, 0x3f4ac37e, 0x3f4ac37e, 0x3f800000,
            ]
        );
        assert_eq!(
            bounded(0.7)
                .advanced_terrace(
                    Module::constant(0.1),
                    Module::constant(0.0),
                    Module::constant(0.25),
                    0.2,
                    0.8,
                    8,
                    3
                )
                .value(0.0, 0.0)
                .to_bits(),
            0x3f333333
        );
        assert_eq!(
            bounded(0.7)
                .advanced_terrace(
                    Module::constant(0.1),
                    Module::constant(1.0),
                    Module::constant(0.25),
                    0.2,
                    0.8,
                    8,
                    3
                )
                .value(0.0, 0.0)
                .to_bits(),
            0x3f404a79
        );
    }

    #[test]
    fn curve_modifiers_match_engine_0_3_0() {
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
        ];
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0x00000000, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3edc7fe7, 0x3edc7fe7, 0x3f000000, 0x3f000000, 0x3eea770a, 0x3eea770a,
            ]
        );
        let ModuleKind::Cache { value, .. } = module.0.as_ref() else {
            unreachable!()
        };
        let ModuleKind::Cache {
            value: clone_value, ..
        } = clone.0.as_ref()
        else {
            unreachable!()
        };
        assert!(std::sync::Arc::ptr_eq(value, clone_value));
        let cached = value.lock().unwrap();
        assert_eq!(
            [cached.x.to_bits(), cached.y.to_bits()],
            [0x44424000, 0xc3a68000]
        );
        assert_eq!(cached.value.to_bits(), 0x3eea770a);
    }
}
