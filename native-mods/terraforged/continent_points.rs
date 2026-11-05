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
    terraforged::{lerp, map, map_current},
    terraforged_engine::{ControlPoints, Terrain, TerrainRegistry},
};

/// TerraForged 1.18.2-0.3.1's `ContinentPoints` constants, kept where the biome overrides (#112) defined them.
pub use super::terraforged_biome_sampler::{BEACH, COAST, DEEP_OCEAN, INLAND, SHALLOW_OCEAN};

/// The jar's `FalloffPoint` record: from `control_point` up to the previous point's control point, the falloff
/// maps continent noise onto `min..max`. No `PartialEq`: the record's `equals` compares floats with
/// `Float.compare` (NaN equals NaN, `-0` differs from `0`), which Rust's `==` would not match.
#[derive(Clone, Copy, Debug)]
pub struct FalloffPoint {
    pub control_point: f32,
    pub min: f32,
    pub max: f32,
}

impl FalloffPoint {
    pub const fn new(control_point: f32, min: f32, max: f32) -> Self {
        Self {
            control_point,
            min,
            max,
        }
    }
}

/// `ContinentPoints.getTerrainType`: below `SHALLOW_OCEAN` a deep ocean, below `BEACH` a shallow ocean, below `COAST`
/// a coast, else `none` (NaN included). The terrains are `terrains`' built-ins (ids 1, 2, 3 and 0), so they compare
/// `is_same` with that registry's `TerrainType` constants.
pub fn terrain_type(continent_noise: f32, terrains: &TerrainRegistry) -> Terrain {
    if continent_noise < SHALLOW_OCEAN {
        terrains.get_by_id(1)
    } else if continent_noise < BEACH {
        terrains.get_by_id(2)
    } else if continent_noise < COAST {
        terrains.get_by_id(3)
    } else {
        terrains.get_by_id(0)
    }
}

/// `ContinentPoints.getFalloff(ControlPoints)`: Engine's `ControlPoints` (the defaults when the settings fail
/// `validate`) from inland down to deep ocean, each mapped onto the band between two of the mod's fixed points.
pub fn falloff_curve(control_points: ControlPoints) -> [FalloffPoint; 5] {
    [
        FalloffPoint::new(control_points.inland, 1.0, 1.0),
        FalloffPoint::new(control_points.coast, COAST, 1.0),
        FalloffPoint::new(control_points.beach, BEACH, COAST),
        FalloffPoint::new(control_points.shallow_ocean, SHALLOW_OCEAN, BEACH),
        FalloffPoint::new(control_points.deep_ocean, DEEP_OCEAN, SHALLOW_OCEAN),
    ]
}

/// `ContinentPoints.getFalloff(float, FalloffPoint[])`: the first point whose control point is at most
/// `continent_noise` (NaN matches none) maps it from `control_point..previous` (1 for the first point) onto
/// `min..max`; past every point, `0..last` maps onto `0..DEEP_OCEAN`. A zero-width band gives NaN, as in the jar.
pub fn falloff(continent_noise: f32, curve: &[FalloffPoint]) -> f32 {
    falloff_with(continent_noise, curve, map)
}

/// [`falloff`] as the newer source tree runs it: the same bytecode, but its `MathUtil.map` reaches Noise2D 2cb4843's
/// `NoiseUtil.map` ([`map_current`]), so a zero-width band (an inland point of 1, or a degenerate custom curve)
/// gives `lerp(min, max, 1)` instead of NaN.
pub fn falloff_current(continent_noise: f32, curve: &[FalloffPoint]) -> f32 {
    falloff_with(continent_noise, curve, map_current)
}

/// `getFalloff` with `NoiseUtil.map` passed in (the jar's [`map`] or the newer source's [`map_current`]).
fn falloff_with(
    continent_noise: f32,
    curve: &[FalloffPoint],
    map: fn(f32, f32, f32, f32) -> f32,
) -> f32 {
    // TerraForged's `MathUtil.map(value, min, max, from, to)`: `NoiseUtil.lerp(from, to, NoiseUtil.map(value, min,
    // max, max - min))`.
    let math_map =
        |value, min: f32, max: f32, from, to| lerp(from, to, map(value, min, max, max - min));
    let mut previous = 1.0;
    for point in curve {
        if continent_noise >= point.control_point {
            return math_map(
                continent_noise,
                point.control_point,
                previous,
                point.min,
                point.max,
            );
        }
        previous = point.control_point;
    }
    math_map(continent_noise, 0.0, previous, 0.0, DEEP_OCEAN)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::WorldControlPoints;
    use super::*;

    // Expected values come from `ContinentPointsOracle.java` (`.local/terraforged/oracles/continent_points/`), which
    // runs the 1.18.2-0.3.1 jar's own `ContinentPoints`, `FalloffPoint`, `MathUtil` and Engine `ControlPoints`.
    const MIN: f32 = f32::from_bits(1);
    const INPUTS: [f32; 31] = [
        f32::NEG_INFINITY,
        -f32::MAX,
        -1.0,
        -0.25,
        -MIN,
        -0.0,
        0.0,
        MIN,
        f32::MIN_POSITIVE,
        0.05,
        0.1,
        f32::from_bits(0x3e7f_ffff),
        0.25,
        f32::from_bits(0x3e80_0001),
        0.3,
        f32::from_bits(0x3eff_ffff),
        0.5,
        f32::from_bits(0x3f00_0001),
        f32::from_bits(0x3f0c_cccc),
        0.55,
        f32::from_bits(0x3f0c_ccce),
        0.6,
        0.75,
        f32::from_bits(0x3f7f_ffff),
        1.0,
        f32::from_bits(0x3f80_0001),
        2.0,
        f32::MAX,
        f32::INFINITY,
        f32::NAN,
        f32::from_bits(0xffc0_0001),
    ];

    fn settings(
        deep: f32,
        shallow: f32,
        beach: f32,
        coast: f32,
        inland: f32,
    ) -> WorldControlPoints {
        WorldControlPoints {
            deep_ocean: deep,
            shallow_ocean: shallow,
            beach,
            coast,
            inland,
        }
    }

    /// The oracle's `curve` line (points' raw bits) and `falloff` line (FNV-1a over each result's canonical bits,
    /// at `INPUTS` then each point's control point with its neighbouring floats).
    /// `ContinentPointsOracle` rerun with Noise2D 2cb4843's `NoiseUtil.map` first on the class path
    /// (`out-current.txt`, the newer source's `getFalloff`): the curves whose fold changes (zero-width bands).
    const CURRENT: [(&str, u64); 3] = [
        ("subnormal", 0xdda8_dcd1_384c_ca15),
        ("negzero", 0xcf7d_ae8a_83b6_3f17),
        ("neginf", 0x810e_a3d7_6cf8_4b36),
    ];

    /// Checks the curve and its fold under [`falloff`], then under [`falloff_current`] (the jar's hash unless
    /// [`CURRENT`] lists the curve).
    fn check_curve(name: &str, curve: &[FalloffPoint], points: &str, count: usize, hash: u64) {
        check_curve_with(name, curve, points, count, hash, falloff);
        let current = CURRENT
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(hash, |&(_, h)| h);
        check_curve_with(name, curve, points, count, current, falloff_current);
    }

    fn check_curve_with(
        name: &str,
        curve: &[FalloffPoint],
        points: &str,
        count: usize,
        hash: u64,
        falloff: fn(f32, &[FalloffPoint]) -> f32,
    ) {
        let actual: Vec<String> = curve
            .iter()
            .map(|p| {
                let [a, b, c] = [p.control_point, p.min, p.max].map(f32::to_bits);
                format!("{a:x},{b:x},{c:x}")
            })
            .collect();
        assert_eq!(actual.join(" "), points, "{name} points");
        let mut inputs = INPUTS.to_vec();
        for point in curve {
            let c = point.control_point;
            inputs.extend([c.next_down(), c, c.next_up()]);
        }
        let mut fnv = 0xcbf2_9ce4_8422_2325_u64;
        for input in &inputs {
            let value = falloff(*input, curve);
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            for byte in bits.to_le_bytes() {
                fnv = (fnv ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        assert_eq!((inputs.len(), fnv), (count, hash), "{name} falloff");
    }

    #[test]
    fn continent_points_constants_match_jar() {
        let constants = [DEEP_OCEAN, SHALLOW_OCEAN, BEACH, COAST, INLAND].map(f32::to_bits);
        assert_eq!(
            constants,
            [
                0x3dcc_cccd,
                0x3e80_0000,
                0x3f00_0000,
                0x3f0c_cccd,
                0x3f19_999a
            ]
        );
    }

    #[test]
    fn continent_points_terrain_types_match_jar() {
        let terrains = TerrainRegistry::new();
        let names: Vec<String> = INPUTS
            .iter()
            .map(|input| {
                let terrain = terrain_type(*input, &terrains);
                assert!(terrain.is_same(&terrains.get_by_id(terrain.id())));
                terrain.name().to_owned()
            })
            .collect();
        let expected = "deep_ocean deep_ocean deep_ocean deep_ocean deep_ocean deep_ocean deep_ocean deep_ocean \
            deep_ocean deep_ocean deep_ocean deep_ocean ocean ocean ocean ocean coast coast coast none none none none \
            none none none none none none none none";
        assert_eq!(names.join(" "), expected);
    }

    #[test]
    fn continent_points_falloff_matches_jar_and_newer_source() {
        let default = "3f008312,3f800000,3f800000 3ee56042,3f0ccccd,3f800000 3ea76c8b,3f000000,3f0ccccd \
            3e800000,3e800000,3f000000 3dcccccd,3dcccccd,3e800000";
        let engine = [
            (
                WorldControlPoints::default(),
                "default",
                default,
                0xe4da_905d_f05d_75a4,
            ),
            (
                settings(0.08, 0.22, 0.34, 0.47, 0.63),
                "custom",
                "3f2147ae,3f800000,3f800000 3ef0a3d7,3f0ccccd,3f800000 3eae147b,3f000000,3f0ccccd \
                 3e6147ae,3e800000,3f000000 3da3d70a,3dcccccd,3e800000",
                0xf815_3f06_45be_39e4,
            ),
            (
                settings(0.1, 0.25, 0.5, 0.55, f32::from_bits(0x3f80_0001)),
                "invalid",
                default,
                0xe4da_905d_f05d_75a4,
            ),
            (
                settings(f32::NAN, 0.25, 0.5, 0.55, 0.6),
                "nan",
                default,
                0xe4da_905d_f05d_75a4,
            ),
            (
                settings(0.0, MIN, f32::from_bits(2), f32::from_bits(3), 1.0),
                "subnormal",
                "3f800000,3f800000,3f800000 3,3f0ccccd,3f800000 2,3f000000,3f0ccccd 1,3e800000,3f000000 \
                 0,3dcccccd,3e800000",
                0x62e8_96d1_f8a1_6e25,
            ),
            (
                settings(-0.0, MIN, 0.5, 0.55, 0.6),
                "negzero",
                "3f19999a,3f800000,3f800000 3f0ccccd,3f0ccccd,3f800000 3f000000,3f000000,3f0ccccd \
                 1,3e800000,3f000000 80000000,3dcccccd,3e800000",
                0x7475_9990_9348_cce3,
            ),
        ];
        for (points, name, expected, hash) in engine {
            check_curve(
                name,
                &falloff_curve(ControlPoints::new(points)),
                expected,
                46,
                hash,
            );
        }
        let point = FalloffPoint::new;
        check_curve("empty", &[], "", 31, 0xf11e_589d_0040_fc82);
        check_curve(
            "single",
            &[point(0.5, 0.2, 0.8)],
            "3f000000,3e4ccccd,3f4ccccd",
            34,
            0x594a_52fc_24ea_704c,
        );
        check_curve(
            "nanpoint",
            &[point(f32::NAN, 0.3, 0.7), point(0.2, 0.1, 0.3)],
            "7fc00000,3e99999a,3f333333 3e4ccccd,3dcccccd,3e99999a",
            37,
            0x3d31_223a_3c5b_2009,
        );
        check_curve(
            "neginf",
            &[point(0.5, 0.6, 0.9), point(f32::NEG_INFINITY, 0.0, 1.0)],
            "3f000000,3f19999a,3f666666 ff800000,0,3f800000",
            37,
            0x36d1_def3_554d_6fb6,
        );
        check_curve(
            "unordered",
            &[point(0.2, 0.1, 0.4), point(0.7, 0.5, 1.0)],
            "3e4ccccd,3dcccccd,3ecccccd 3f333333,3f000000,3f800000",
            37,
            0xa187_3a80_1f5a_41db,
        );
        // Spot values (the original fixtures): a zero-width band (inland = 1) gives NaN.
        let curve = falloff_curve(ControlPoints::new(settings(
            0.0,
            MIN,
            f32::from_bits(2),
            f32::from_bits(3),
            1.0,
        )));
        assert!(falloff(1.0, &curve).is_nan());
        // The newer source (Noise2D 2cb4843's map) gives the band's lerp at 1 there, and the deep-ocean tail of a
        // zero deep-ocean point (`0..0`) maps a negative input to DEEP_OCEAN instead of NaN.
        assert_eq!(falloff_current(1.0, &curve), 1.0);
        assert!(falloff(-0.25, &curve).is_nan());
        assert_eq!(falloff_current(-0.25, &curve), DEEP_OCEAN);
        let custom = falloff_curve(ControlPoints::new(settings(0.08, 0.22, 0.34, 0.47, 0.63)));
        assert_eq!(falloff(0.4, &custom).to_bits(), 0x3f05_e85f);
        assert_eq!(falloff(-0.25, &custom).to_bits(), 0);
    }
}
