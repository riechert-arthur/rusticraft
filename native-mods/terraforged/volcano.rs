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

// TerraForged's `worldgen/test` volcano: `Volcano`, `VolcanoConfig` and `VolcanoFeature`.
// The original never registers the feature, and its `place` and centre test return
// before their real work (`if (true) return ...`); both switches are kept, so the feature
// is ported but places nothing, as in TerraForged.

/// `VolcanoFeature.place`'s `if (true) return true;`.
pub const PLACES_BLOCKS: bool = false;
/// `VolcanoFeature.test`'s `if (true) return y < 180;`, before its biome check.
pub const CHECKS_BIOME: bool = false;

#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub min: i32,
    pub max: i32,
}

impl Range {
    /// `Mth.lerp(rand, min, max)`.
    pub fn get(self, rand: f64) -> f64 {
        f64::from(self.min) + rand * (f64::from(self.max) - f64::from(self.min))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VolcanoConfig {
    pub density: f64,
    pub jitter: f64,
    pub pool_radius: Range,
    pub mouth_radius: Range,
    pub base_radius: Range,
    pub pool_height: Range,
    pub mouth_height: Range,
    pub fluid_level: Range,
}

impl VolcanoConfig {
    pub const DEFAULT: Self = Self {
        density: 1.0,
        jitter: 0.8,
        pool_radius: Range { min: 5, max: 15 },
        mouth_radius: Range { min: 20, max: 30 },
        base_radius: Range { min: 100, max: 200 },
        pool_height: Range { min: 40, max: 60 },
        mouth_height: Range { min: 100, max: 200 },
        fluid_level: Range { min: 70, max: 80 },
    };

    pub fn scale(&self) -> f64 {
        f64::from(self.base_radius.max)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Value {
    pub hash: i64,
    pub height: i32,
    pub mouth: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub hash: i64,
    pub x: i32,
    pub z: i32,
}

pub fn to_height_value(height: f64) -> i32 {
    64 + (height * 0.5).floor() as i32
}

/// `Volcano.getHighest`: a mouth beats any slope; otherwise the higher wins.
pub fn highest(x: i32, z: i32, config: &VolcanoConfig, points: &[Point]) -> Value {
    let mut max = Value::default();
    for point in points {
        let mut value = Value::default();
        eval_point(x, z, point, &mut value, config);
        let higher = value.height > max.height;
        if if max.mouth {
            value.mouth && higher
        } else {
            value.mouth || higher
        } {
            max.height = value.height;
            max.mouth = value.mouth;
            max.hash = point.hash;
        }
    }
    max
}

fn eval_point(x: i32, z: i32, point: &Point, value: &mut Value, config: &VolcanoConfig) {
    let mut alpha = distance_noise(x, z, point, value, config);
    if alpha.is_nan() {
        return;
    }
    let mut low = 0.0;
    let high = config.mouth_height.get(rand(point.hash, HEIGHT_1));
    if value.mouth {
        low = config.pool_height.get(rand(point.hash, HEIGHT_0));
    } else {
        // Curve the outer slope.
        alpha *= alpha;
    }
    value.height = to_height_value(low + alpha * (high - low));
}

fn distance_noise(x: i32, z: i32, point: &Point, value: &mut Value, config: &VolcanoConfig) -> f64 {
    let (dx, dz) = (f64::from(point.x - x), f64::from(point.z - z));
    let distance2 = dx * dx + dz * dz;
    let mut outer = config.base_radius.get(rand(point.hash, RADIUS_2));
    if distance2 >= outer * outer {
        return f64::NAN;
    }
    let mut inner = config.mouth_radius.get(rand(point.hash, RADIUS_1));
    if distance2 < inner * inner {
        value.mouth = true;
        outer = inner;
        inner = config.pool_radius.get(rand(point.hash, RADIUS_0));
        if distance2 <= inner * inner {
            return 0.0;
        }
        return (distance2.sqrt() - inner) / (outer - inner);
    }
    1.0 - (distance2.sqrt() - inner) / (outer - inner)
}

/// `Volcano.collectPoints` for a chunk: one jittered, density-thinned centre per grid
/// cell around it that `accept` takes.
pub fn collect_points(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    config: &VolcanoConfig,
    mut accept: impl FnMut(i32, i32) -> bool,
) -> Vec<Point> {
    let (x0, z0) = (chunk_x << 4, chunk_z << 4);
    let (x1, z1) = (x0 + 15, z0 + 15);
    let frequency = 1.0 / config.scale();
    let min_x = (f64::from(x0) * frequency).floor() as i32 - 1;
    let min_z = (f64::from(z0) * frequency).floor() as i32 - 1;
    let max_x = (f64::from(x1) * frequency).floor() as i32 + 1;
    let max_z = (f64::from(z1) * frequency).floor() as i32 + 1;
    let mut points = Vec::new();
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let hash = mix_cell(seed, x, z);
            // Skip cells to reduce the density.
            if rand(hash, DENSITY) > config.density {
                continue;
            }
            let px = f64::from(x) + rand(hash, POINT_X) * config.jitter;
            let pz = f64::from(z) + rand(hash, POINT_Y) * config.jitter;
            let pos_x = (px / frequency).floor() as i32;
            let pos_z = (pz / frequency).floor() as i32;
            if accept(pos_x, pos_z) {
                points.push(Point {
                    hash,
                    x: pos_x,
                    z: pos_z,
                });
            }
        }
    }
    points
}

/// `VolcanoFeature.test`: a centre below y 180.
pub fn accepts_centre(ocean_floor: i32) -> bool {
    ocean_floor < 180 || CHECKS_BIOME
}

/// What `VolcanoFeature.place` writes into a chunk.
pub trait VolcanoChunk<S: Copy> {
    /// `getHeight(OCEAN_FLOOR_WG, dx, dz) - 1`.
    fn ocean_floor(&self, dx: i32, dz: i32) -> i32;
    fn set(&mut self, dx: i32, y: i32, dz: i32, state: S);
}

/// The blocks `VolcanoFeature.place` uses.
#[derive(Clone, Copy)]
pub struct VolcanoBlocks<S> {
    pub stone: S,
    pub air: S,
    pub water: S,
    pub lava: S,
}

/// `VolcanoFeature.place`; with [`PLACES_BLOCKS`] off, as in TerraForged, it returns at once.
pub fn place<S: Copy + PartialEq>(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    config: &VolcanoConfig,
    blocks: VolcanoBlocks<S>,
    chunk: &mut impl VolcanoChunk<S>,
    accept: impl FnMut(i32, i32) -> bool,
) -> bool {
    if !PLACES_BLOCKS {
        return true;
    }
    let points = collect_points(seed, chunk_x, chunk_z, config, accept);
    for dz in 0..16 {
        for dx in 0..16 {
            let value = highest((chunk_x << 4) + dx, (chunk_z << 4) + dz, config, &points);
            if value.height == 0 {
                continue;
            }
            let mut height = value.height;
            let mut surface = chunk.ocean_floor(dx, dz);
            let mut filler = blocks.stone;
            if value.mouth && height < surface {
                height = surface + 5;
                surface = value.height;
                filler = blocks.air;
            }
            let fluid = if rand(value.hash, FLUID_FILLER) < 0.5 {
                blocks.water
            } else {
                blocks.lava
            };
            let fluid_level = to_height_value(config.fluid_level.get(rand(value.hash, HEIGHT_2)));
            let mut y = height;
            while y > surface {
                let block = if filler == blocks.air && y <= fluid_level {
                    fluid
                } else {
                    filler
                };
                chunk.set(dx, y, dz, block);
                y -= 1;
            }
        }
    }
    true
}

const DENSITY: i32 = 6869;
const POINT_X: i32 = 12343;
const POINT_Y: i32 = 16477;
const RADIUS_0: i32 = 18899;
const RADIUS_1: i32 = 21701;
const RADIUS_2: i32 = 26921;
const HEIGHT_0: i32 = 30047;
const HEIGHT_1: i32 = 31643;
const HEIGHT_2: i32 = 33199;
const FLUID_FILLER: i32 = 39761;

fn mix_gamma(z: i64) -> i64 {
    let mut z = z as u64;
    z = (z ^ (z >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
    z = (z ^ (z >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    z = (z ^ (z >> 33)) | 1;
    let n = (z ^ (z >> 1)).count_ones();
    (if n < 24 { z ^ 0xaaaa_aaaa_aaaa_aaaa } else { z }) as i64
}

fn mix(z: i64) -> i64 {
    let mut z = z as u64;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) as i64
}

fn mix_with(seed: i64, offset: i64) -> i64 {
    mix(seed.wrapping_add(mix_gamma(offset)))
}

fn mix_cell(seed: i64, x: i32, z: i32) -> i64 {
    mix_with(mix_with(seed, i64::from(x)), i64::from(z))
}

fn rand(hash: i64, offset: i32) -> f64 {
    ((mix_with(hash, i64::from(offset)) as u64) >> 11) as f64
        * f64::from_bits(0x3ca0_0000_0000_0000)
}

#[cfg(test)]
mod tests {
    use super::*;

    // From tools/terraforged/TerraForgedVolcanoOracle.java (its mouth columns and every sixth
    // other one): seed, chunk x and z, block x and z, height, mouth and the winning hash.
    const COLUMNS: [(i64, i32, i32, i32, i32, i32, bool, i64); 39] = [
        (1234, 0, 0, 0, 0, 64, false, 8683236328467284493),
        (1234, 0, 0, 0, 3, 64, false, 8683236328467284493),
        (1234, 0, 0, 0, 6, 64, false, 8683236328467284493),
        (1234, 0, 0, 0, 9, 65, false, 8683236328467284493),
        (1234, 0, 0, 0, 12, 65, false, 8683236328467284493),
        (1234, 0, 0, 0, 15, 65, false, 8683236328467284493),
        (1234, 5, -3, 80, -48, 66, false, 1448916186279217493),
        (1234, 5, -3, 80, -45, 65, false, 1448916186279217493),
        (1234, 5, -3, 80, -42, 65, false, 1448916186279217493),
        (1234, 5, -3, 80, -39, 64, false, 1448916186279217493),
        (1234, 5, -3, 80, -36, 64, false, 1448916186279217493),
        (1234, 5, -3, 80, -33, 64, false, 1448916186279217493),
        (1234, -12, 7, -192, 112, 113, false, -7763188581607500539),
        (1234, -12, 7, -192, 115, 110, false, -7763188581607500539),
        (1234, -12, 7, -192, 118, 108, false, -7763188581607500539),
        (1234, -12, 7, -192, 121, 105, false, -7763188581607500539),
        (1234, -12, 7, -192, 124, 103, false, -7763188581607500539),
        (1234, -12, 7, -192, 127, 101, false, -7763188581607500539),
        (1234, 40, 40, 640, 640, 84, false, 7580164125718389058),
        (1234, 40, 40, 640, 643, 85, false, 7580164125718389058),
        (1234, 40, 40, 640, 646, 87, false, 7580164125718389058),
        (1234, 40, 40, 640, 649, 88, false, 7580164125718389058),
        (1234, 40, 40, 640, 652, 89, false, 7580164125718389058),
        (1234, 40, 40, 640, 655, 91, false, 7580164125718389058),
        (-987654321987, 0, 0, 12, 15, 64, false, -3440878337646031563),
        (
            -987654321987,
            5,
            -3,
            92,
            -48,
            64,
            false,
            -4387052528602779307,
        ),
        (
            -987654321987,
            5,
            -3,
            92,
            -45,
            64,
            false,
            -4387052528602779307,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            112,
            90,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            115,
            90,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            118,
            90,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            121,
            91,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            124,
            91,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            -12,
            7,
            -192,
            127,
            91,
            false,
            7518619269005532078,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            640,
            111,
            false,
            9047382995963561974,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            643,
            113,
            false,
            9047382995963561974,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            646,
            114,
            false,
            9047382995963561974,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            649,
            115,
            false,
            9047382995963561974,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            652,
            116,
            false,
            9047382995963561974,
        ),
        (
            -987654321987,
            40,
            40,
            640,
            655,
            117,
            false,
            9047382995963561974,
        ),
    ];

    #[test]
    fn volcano_columns_match_terraforged_source() {
        for (seed, cx, cz, x, z, height, mouth, hash) in COLUMNS {
            let points = collect_points(seed, cx, cz, &VolcanoConfig::DEFAULT, |_, _| true);
            let value = highest(x, z, &VolcanoConfig::DEFAULT, &points);
            assert_eq!(
                value,
                Value {
                    hash,
                    height,
                    mouth
                },
                "{seed} {x} {z}"
            );
        }
    }

    #[test]
    fn the_feature_stays_off_as_in_terraforged() {
        struct Never;
        impl VolcanoChunk<u8> for Never {
            fn ocean_floor(&self, _: i32, _: i32) -> i32 {
                unreachable!()
            }
            fn set(&mut self, _: i32, _: i32, _: i32, _: u8) {
                unreachable!()
            }
        }
        let blocks = VolcanoBlocks {
            stone: 1,
            air: 0,
            water: 2,
            lava: 3,
        };
        assert!(place(
            1,
            0,
            0,
            &VolcanoConfig::DEFAULT,
            blocks,
            &mut Never,
            |_, _| true
        ));
        assert!(accepts_centre(179) && !accepts_centre(180));
    }
}
