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

use super::terraforged::{
    Module, Source, SourceConfig, SourceKind, clamp, copy_sign, map, pow_int, sin,
};

#[derive(Clone, Copy)]
pub struct ClimateRange {
    pub seed_offset: i32,
    pub scale: i32,
    pub falloff: i32,
    pub min: f32,
    pub max: f32,
    pub bias: f32,
}

impl ClimateRange {
    pub const fn new(scale: i32, falloff: i32, min: f32, max: f32, bias: f32) -> Self {
        Self {
            seed_offset: 0,
            scale,
            falloff,
            min,
            max,
            bias,
        }
    }

    pub fn min(self) -> f32 {
        self.min.min(self.max).clamp(0.0, 1.0)
    }

    pub fn max(self) -> f32 {
        self.min.max(self.max).clamp(self.min(), 1.0)
    }

    pub fn bias(self) -> f32 {
        self.bias.clamp(-1.0, 1.0)
    }

    pub fn apply(self, module: Module) -> Module {
        module
            .bias(Module::constant(self.bias() / 2.0))
            .clamp(Module::constant(self.min()), Module::constant(self.max()))
    }
}

impl Default for ClimateRange {
    fn default() -> Self {
        Self::new(7, 1, 0.0, 1.0, 0.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BiomeShape {
    pub biome_size: i32,
    pub macro_noise_size: i32,
    pub biome_warp_scale: i32,
    pub biome_warp_strength: i32,
}

impl BiomeShape {
    pub const DEFAULT_BIOME_SIZE: i32 = 225;
}

impl Default for BiomeShape {
    fn default() -> Self {
        Self {
            biome_size: Self::DEFAULT_BIOME_SIZE,
            macro_noise_size: 8,
            biome_warp_scale: 150,
            biome_warp_strength: 80,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BiomeNoise {
    pub kind: SourceKind,
    pub scale: i32,
    pub octaves: i32,
    pub gain: f32,
    pub lacunarity: f32,
    pub strength: i32,
}

impl BiomeNoise {
    pub fn build(self, seed: i32) -> Module {
        Module::source(Source::new(
            self.kind,
            SourceConfig {
                seed,
                octaves: self.octaves as usize,
                gain: self.gain,
                frequency: 1.0 / self.scale as f32,
                lacunarity: self.lacunarity,
                ..SourceConfig::default()
            },
        ))
        .bias(Module::constant(-0.5))
    }
}

impl Default for BiomeNoise {
    fn default() -> Self {
        Self {
            kind: SourceKind::Simplex,
            scale: 24,
            octaves: 2,
            gain: 0.5,
            lacunarity: 2.65,
            strength: 14,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClimateSettings {
    pub temperature: ClimateRange,
    pub moisture: ClimateRange,
    pub biome_shape: BiomeShape,
    pub biome_edge_shape: BiomeNoise,
}

impl Default for ClimateSettings {
    fn default() -> Self {
        Self {
            temperature: ClimateRange::new(6, 2, 0.0, 0.98, 0.05),
            moisture: ClimateRange::new(6, 1, 0.0, 1.0, 0.0),
            biome_shape: BiomeShape::default(),
            biome_edge_shape: BiomeNoise::default(),
        }
    }
}

/// Engine's `Compressor` (`world.climate`): squeezes `module`'s middle band `lower_start..upper_end` into
/// `lower_end..upper_start` and stretches the two ends to fill the rest of 0..1. All float math in Engine's order.
#[derive(Clone)]
pub struct Compressor {
    lower_start: f32,
    lower_end: f32,
    lower_range: f32,
    lower_expand_range: f32,
    upper_start: f32,
    upper_end: f32,
    upper_range: f32,
    upper_expanded_range: f32,
    compression: f32,
    compression_range: f32,
    module: Module,
}

impl Compressor {
    /// `new Compressor(module, inset, amount)`.
    pub fn new(module: Module, inset: f32, amount: f32) -> Self {
        Self::with_ranges(
            module,
            inset,
            inset + amount,
            1.0 - inset - amount,
            1.0 - inset,
        )
    }

    /// `new Compressor(module, lowerStart, lowerEnd, upperStart, upperEnd)`.
    pub fn with_ranges(
        module: Module,
        lower_start: f32,
        lower_end: f32,
        upper_start: f32,
        upper_end: f32,
    ) -> Self {
        Self {
            module,
            lower_start,
            lower_end,
            lower_range: lower_start,
            lower_expand_range: lower_end,
            upper_start,
            upper_end,
            upper_range: 1.0 - upper_end,
            upper_expanded_range: 1.0 - upper_start,
            compression: upper_start - lower_end,
            compression_range: upper_end - lower_start,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let value = self.module.value(x, y);
        if value <= self.lower_start {
            let alpha = value / self.lower_range;
            return alpha * self.lower_expand_range;
        }
        if value >= self.upper_end {
            let delta = value - self.upper_end;
            let alpha = delta / self.upper_range;
            return self.upper_start + alpha * self.upper_expanded_range;
        }
        let delta = value - self.lower_start;
        let alpha = delta / self.compression_range;
        self.lower_end + alpha * self.compression
    }

    /// As a graph node, with the `Noise` default bounds 0..1 (Engine doesn't override them).
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

/// Engine's `Temperature`: a latitude band, `sin(y * frequency)` from `NoiseUtil`'s lookup table raised to `power`
/// (sign kept) and mapped from -1..1 to 0..1. `x` is unused.
#[derive(Clone, Copy)]
pub struct Temperature {
    power: i32,
    frequency: f32,
}

impl Temperature {
    /// `new Temperature(frequency, power)`.
    pub const fn new(frequency: f32, power: i32) -> Self {
        Self { power, frequency }
    }

    pub fn value(self, _x: f32, y: f32) -> f32 {
        let sin = clamp(sin(y * self.frequency), -1.0, 1.0);
        let value = copy_sign(pow_int(sin, self.power), sin);
        map(value, -1.0, 1.0, 2.0)
    }

    /// As a graph node, with the `Noise` default bounds 0..1.
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

/// Engine's `Moisture`: `source` sampled at half the x frequency; from `power` 2 up, recentred to -1..1, raised to
/// `power` (sign kept) and mapped back to 0..1.
#[derive(Clone)]
pub struct Moisture {
    source: Module,
    power: i32,
}

impl Moisture {
    /// `new Moisture(seed, scale, power)`: `Source.simplex(seed, scale, 1).clamp(0.125, 0.875).map(0, 1)`.
    pub fn new(seed: i32, scale: i32, power: i32) -> Self {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed,
                frequency: 1.0 / scale as f32,
                ..SourceConfig::default()
            },
        ))
        .clamp(Module::constant(0.125), Module::constant(0.875))
        .map(Module::constant(0.0), Module::constant(1.0));
        Self::from_module(source, power)
    }

    /// `new Moisture(source, power)`: wraps `source.freq(0.5, 1.0)`.
    pub fn from_module(source: Module, power: i32) -> Self {
        Self {
            source: source.frequency(Module::constant(0.5), Module::constant(1.0)),
            power,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let mut noise = self.source.value(x, y);
        if self.power < 2 {
            return noise;
        }
        noise = (noise - 0.5) * 2.0;
        let value = copy_sign(pow_int(noise, self.power), noise);
        map(value, -1.0, 1.0, 2.0)
    }

    /// As a graph node, with the `Noise` default bounds 0..1.
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Output of `.local/terraforged/oracles/climate/ClimateOracle.java` (run.sh) against Engine 0.3.0.
    const ORACLE: &str = r"
c 0 9e47820e6da50ed4
c 1 ca58e6dfac6956b6
c 2 b15cfb34f55662
c 3 5273a64c0ec741d1
c 4 9273261680a7d895
c 5 80030bc32344ce08
c 6 24b24dfdd719d007
c 7 d24edd85e3c0c4f
c 8 3f03b346e6bc0c4f
r 0 60ac147a37ddbfc9
r 1 5273a64c0ec741d1
r 2 7c8a7d78d89ab295
r 3 76cec387f5923ed
r 4 b15cfb34f55662
r 5 d24edd85e3c0c4f
t 0 a71d34adf7c0c4f 3b3565dd64f9381b 5a4af3056543c845 c31c8c02fbada914 eb41d1a1622f5639 f8aced0e8b906fb4 493273d6c6dd743a ac489289d28aaf4a a71d34adf7c0c4f a71d34adf7c0c4f a71d34adf7c0c4f
t 1 279b6a37af7c0c4f 99544e5f1ef1fb66 241c83784760e085 6529371a0ccf8e08 27421174319b0903 8de91d30e6622e28 7265c8edc838df7b b2c7557d40531fd5 279b6a37af7c0c4f 279b6a37af7c0c4f 279b6a37af7c0c4f
t 2 4835613d807c0c4f a4bc1ed3373ab69e 3cae512e5ecc94df 38525e1a85cf6e40 5f8ba0e0a7a8c15 d29a0e5f96a548a 157f8faa3a9818b6 dd97759e85120f48 4835613d807c0c4f 4835613d807c0c4f 4835613d807c0c4f
t 3 dc24d028bdfc0c4f a9a8c383b0ca03a8 9870f2533e993c4b 3805d0cabc605e0b f677f71000271fc6 c071d34661e70d9a f985fbb3b9c0fd2a b64092ce2db3523d dc24d028bdfc0c4f dc24d028bdfc0c4f dc24d028bdfc0c4f
t 4 1c15fe725dfc0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f
t 5 c73992f7d07c0c4f 56e8dccdf21d6b1c 62953ce1cf5e8066 9c332e986afa469e c38290360668337e e43a32d5e30a54c9 8a5cfbb19251f34d f5df9c0065fb324c c73992f7d07c0c4f c73992f7d07c0c4f c73992f7d07c0c4f
t 6 d45065ec8efc0c4f b45cb61faa0894c7 321711133bfb9c8c 7a170b075a6584f 9964ffa097f900ac 89d54141dac484d3 5583e7254f4e0070 18839cbbd6283f74 d45065ec8efc0c4f d45065ec8efc0c4f d45065ec8efc0c4f
t 7 1c15fe725dfc0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f
t 8 6f41f12ae07c0c4f 75092d206c1ccb9e ff99de31f284091d c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 6f41f12ae07c0c4f 6f41f12ae07c0c4f 6f41f12ae07c0c4f
m 474ac1af09e680a2 474ac1af09e680a2 ecaecf1a11e7ffca 62f38e51983c5e4b 379e4519613f2205 1c5a0ebcf0e327fb 434562d962b96a3 66efb06b0ebf4632 474ac1af09e680a2 474ac1af09e680a2 474ac1af09e680a2
k 474ac1af09e680a2 474ac1af09e680a2 ecaecf1a11e7ffca 62f38e51983c5e4b 379e4519613f2205 1c5a0ebcf0e327fb 434562d962b96a3 66efb06b0ebf4632 474ac1af09e680a2 474ac1af09e680a2 474ac1af09e680a2
n 0 2d1077121f72a31e 2d1077121f72a31e d9ed3c1db03acb20 d04c1783cd1b995a 98b7297998dd6c15 74aa9106dc3dcc57 74aa9106dc3dcc57 e2d6bb4849a3731b 2e7a3485d05a0585 eaeefa3e50dbacac 44fc765383699a44 44fc765383699a44 ef5148e606c791d3 2465211565259e9f 813d040699cde75b 82ec65e973e9b192 82ec65e973e9b192 a38f7b11ccf5b9cc 71e1d471fbb443c5 f18151b0590d68fc 7f0f039ea7571d47 7f0f039ea7571d47 737000970bd9c8ca f7ba4da85f91cf7d d8152287d13a7a08 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 1 dc5e534bd879484d dc5e534bd879484d 241394aaf48d59f5 c3375527c9ab827f 4b7b4f1213c151b2 4ffe6833d0c45181 4ffe6833d0c45181 b08a2b9072bb2ade a708b8ba6a2e616c a2326e0013a85e86 3cbb8b606df8feec 3cbb8b606df8feec ed7f8bc2b57c693a fb117ebbf352ddc5 7b9f38d37b950941 3e4a32939b0a05cd 3e4a32939b0a05cd 31b02bbb26e14c75 410d28afe373d887 e7eb356198750bc1 6b23e5d49cf69304 6b23e5d49cf69304 85c7a8d60f8e1445 4b7bb65799ac90f8 bd49c8142ab9918a da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n -1 f127151d0a46af74 f127151d0a46af74 31ef3d0d1863941d 392e94cce69169a3 a4c442ab161ad328 c12bb785cde25e9b c12bb785cde25e9b d2395f675e714043 65bc3321769a910e 5882a33c881f1e55 163db6ea471abb98 163db6ea471abb98 d2993d444e488daf 13129ada9010fa0e b0b90e8847722e5f 3c3c37c455cdf89a 3c3c37c455cdf89a b8994c3ff582efb5 34a63a77e6c01670 75496c220e140bc9 e209daed716a5535 e209daed716a5535 cbc838e427df50d1 23026dece33bf884 87770f3e9a737f06 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 1337 be2b8d6c3e416dd4 be2b8d6c3e416dd4 5760169023b85dba 5b4fd348a22c17f1 efe2d6d72a9c886c 105f45640f45db89 105f45640f45db89 f9a4b30491077c9c 2fad81c433971775 d0149e49e29ca83 9ef33dabe9d1e111 9ef33dabe9d1e111 d8152f9de98e149b 27c06ba507b3cb79 257b303fee9378a8 4fc99c510d81941f 4fc99c510d81941f 8ba55a78e7668463 2fec0bf4ace64650 f04778d3a478e932 252aded5d22b948a 252aded5d22b948a d7609fb58106849d 8523781dedec75e 65ff27dc9b3c677a da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 123456789 7edb3ce4924a3ace 7edb3ce4924a3ace c01cec06add5fcba ee531290189475a6 7bc4c691936433bb 12dfb842ca65977 12dfb842ca65977 2fdd730b353fe634 569b580e296b343b 431dc25a53c45f86 5ffc0df7c4451eed 5ffc0df7c4451eed 5e7216ca6a11a3d5 a1e106f57f450467 5b9b419ca5308c68 cfcdddbef358b249 cfcdddbef358b249 f4b4033f4ca9164a a35f9d5cbe793b43 91f1616e5663420c 4de0db2d426cca28 4de0db2d426cca28 3cf5ccb27db878a8 c160bc0a5e2fd111 1f2ca968f34ce945 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 2147483647 8648ff5f0fffa445 8648ff5f0fffa445 9111e0c0fdc69d01 6e933b288ce59014 6fb8d9768a4b2204 d12f9618c73207f1 d12f9618c73207f1 4e55aa790481b13a b03cc5de52f079ee 64267f85e5d16143 d5e1cfd2a6b1421 d5e1cfd2a6b1421 c95ac46e0b344eea bfe46355cfae13a6 b8644cfb3aa29280 c0038e4c9160ab11 c0038e4c9160ab11 5c7c4aa820f07428 d828f93dbc289636 44e3ac0be37eddae e4e47bcd1bc98e6f e4e47bcd1bc98e6f 7e675aa525e4019e c3a50e81de852ea7 4054f7a030590dd7 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n -2147483648 e3ce1d5b8316cb3b e3ce1d5b8316cb3b 3eafc67c4368463e 5a7e2dd13f84614 5f07d8c158d7e6c1 493028a9d9bd9ad2 493028a9d9bd9ad2 2984462022c677c2 8acb2c74e508ed96 8bb7d732248588e9 60eb2df5257aa118 60eb2df5257aa118 7f3c636953fd0c22 ab14a5a2a8bbd45e 336e2b353604b9d1 5fffd8a50a6418ff 5fffd8a50a6418ff 1d66e3c7105ee2cd 939178bc9deb339d 59e349bb548d5873 369a6f284ec45604 369a6f284ec45604 d3552f5fbb355471 3da15ab874b79262 46a69b06746b706f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
s 3f000000 3f000000 3f1b40fc 3efc4e4a 3ef7a125 3ea0d008 3eeb3385 3f78daa6
";

    const V: [f32; 41] = [
        0.0,
        -0.0,
        1e-7,
        -1e-7,
        0.1,
        0.2,
        0.25,
        0.3,
        0.5,
        0.7,
        0.75,
        0.8,
        0.9,
        1.0,
        1.1,
        1.5,
        -1.0,
        -0.5,
        2.0,
        -2.0,
        3.3,
        10.0,
        -10.0,
        25.0,
        -25.0,
        100.7,
        -1000.3,
        12345.6,
        1e6,
        -1e6,
        3e7,
        -3e7,
        1e30,
        -1e30,
        f32::MAX,
        -f32::MAX,
        f32::from_bits(1),
        -f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    const POWERS: [i32; 11] = [0, 1, 2, 3, 4, 5, 7, 8, -1, -3, i32::MIN];

    /// Java's `floatToIntBits`: NaNs canonical.
    fn fold(f: i64, value: f32) -> i64 {
        let bits = if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        };
        f.wrapping_mul(31).wrapping_add(i64::from(bits as i32))
    }

    fn all(module: &Module) -> String {
        let mut f = fold(fold(17, module.min_value()), module.max_value());
        for x in V {
            for y in V {
                f = fold(f, module.value(x, y));
            }
        }
        format!("{:x}", f as u64)
    }

    /// The oracle's identity lambda, which reports the `Noise` defaults 0..1.
    fn id() -> Module {
        Module::custom(0.0, 1.0, |x, _| x)
    }

    fn row(tag: String, hashes: impl IntoIterator<Item = String>) -> String {
        hashes
            .into_iter()
            .fold(tag, |line, hash| line + " " + &hash)
    }

    #[test]
    fn climate_primitives_match_engine_oracle() {
        let mut out = Vec::new();
        let nan = f32::NAN;
        let compressors = [
            (0.2, 0.1),
            (0.0, 0.1),
            (0.5, 0.0),
            (0.0, 0.0),
            (0.1, 0.5),
            (-0.1, 0.2),
            (0.3, 0.3),
            (nan, 0.1),
            (0.1, f32::INFINITY),
        ];
        for (i, (inset, amount)) in compressors.into_iter().enumerate() {
            out.push(format!(
                "c {i} {}",
                all(&Compressor::new(id(), inset, amount).module())
            ));
        }
        let ranges = [
            (0.1, 0.3, 0.6, 0.9),
            (0.0, 0.0, 1.0, 1.0),
            (0.3, 0.1, 0.9, 0.6),
            (-1.0, 2.0, -3.0, 4.0),
            (0.25, 0.25, 0.75, 0.75),
            (nan, 0.5, 0.5, nan),
        ];
        for (i, (a, b, c, d)) in ranges.into_iter().enumerate() {
            let compressor = Compressor::with_ranges(id(), a, b, c, d);
            out.push(format!("r {i} {}", all(&compressor.module())));
        }
        let frequencies = [
            0.125,
            1.0,
            1.0 / 2048.0,
            -0.5,
            0.0,
            1e-4,
            3.7,
            nan,
            f32::INFINITY,
        ];
        for (i, frequency) in frequencies.into_iter().enumerate() {
            let hashes = POWERS.map(|power| all(&Temperature::new(frequency, power).module()));
            out.push(row(format!("t {i}"), hashes));
        }
        out.push(row(
            "m".into(),
            POWERS.map(|p| all(&Moisture::from_module(id(), p).module())),
        ));
        let clamped = || id().clamp(Module::constant(0.0), Module::constant(1.0));
        let hashes = POWERS.map(|p| all(&Moisture::from_module(clamped(), p).module()));
        out.push(row("k".into(), hashes));
        for seed in [0, 1, -1, 1337, 123456789, i32::MAX, i32::MIN] {
            let hashes = [1, 7, 400, 2048, -100, 0].into_iter().flat_map(|scale| {
                [0, 1, 2, 3, 6].map(|power| all(&Moisture::new(seed, scale, power).module()))
            });
            out.push(row(format!("n {seed}"), hashes));
        }
        let moisture = Moisture::new(1337, 400, 3);
        let temperature = Temperature::new(1.0 / 2048.0, 2);
        let points = [(0.0, 0.0), (100.0, -250.0), (-5000.5, 7777.0), (1e6, -1e6)];
        out.push(row(
            "s".into(),
            points.into_iter().flat_map(|(x, y)| {
                [moisture.value(x, y), temperature.value(x, y)]
                    .map(|v| format!("{:x}", v.to_bits()))
            }),
        ));
        let expected: Vec<&str> = ORACLE.lines().skip(1).collect();
        assert_eq!(out.len(), expected.len());
        for (line, expected) in out.iter().zip(expected) {
            assert_eq!(line, expected);
        }
    }

    #[test]
    fn climate_settings_match_engine_0_3_0() {
        let settings = ClimateSettings::default();
        assert_eq!(
            (
                settings.temperature.seed_offset,
                settings.temperature.scale,
                settings.temperature.falloff,
                settings.temperature.min.to_bits(),
                settings.temperature.max.to_bits(),
                settings.temperature.bias.to_bits(),
            ),
            (0, 6, 2, 0x00000000, 0x3f7ae148, 0x3d4ccccd)
        );
        assert_eq!(
            (
                settings.moisture.seed_offset,
                settings.moisture.scale,
                settings.moisture.falloff,
                settings.moisture.min.to_bits(),
                settings.moisture.max.to_bits(),
                settings.moisture.bias.to_bits(),
            ),
            (0, 6, 1, 0x00000000, 0x3f800000, 0x00000000)
        );
        assert_eq!(settings.biome_shape, BiomeShape::default());
    }

    #[test]
    fn climate_range_and_edge_noise_match_engine_0_3_0() {
        let range = ClimateRange {
            min: 0.8,
            max: 0.2,
            bias: 2.0,
            ..ClimateRange::default()
        };
        assert_eq!(
            bits([range.min(), range.max(), range.bias()]),
            [0x3e4ccccd, 0x3f4ccccd, 0x3f800000]
        );
        assert_eq!(
            range
                .apply(Module::constant(0.25))
                .value(0.0, 0.0)
                .to_bits(),
            0x3f400000
        );

        let edge = BiomeNoise::default().build(42);
        assert_eq!(
            bits(
                [
                    (0.0, 0.0),
                    (1.0, 2.0),
                    (24.0, 48.0),
                    (-10.0, 7.0),
                    (1000.0, -300.0)
                ]
                .map(|(x, y)| edge.value(x, y))
            ),
            [0x00000000, 0xbdd59d54, 0x3e314904, 0xbea34c20, 0xbe89fdf4]
        );
    }
}
