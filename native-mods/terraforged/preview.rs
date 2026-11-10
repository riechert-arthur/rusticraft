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

// Ported from TerraForged 6dd607eb
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44):
// `src/main/java/com/terraforged/mod/util/ColorUtil.java` (`getBiomeColor`, `getColor`, `shade`) and
// `src/main/java/com/terraforged/mod/client/ui/Previewer.java` (`paint`, `each`, `Worker`: the view's size, zoom and
// centre). MIT License, Copyright (c) 2021 TerraForged (above).
//
// TerraForged's `ContinentPreview` is a developer window (a Swing `main`, never opened by the game) that colours
// `getBiomeColor(sample, 0.2f)` over a generator of its own at the fixed seed 273651, built on `new TerrainLevels()`
// (frequency 1). `/tf preview` instead draws the player's world: the level's own generator and runtime seed, sampled
// through the world's `IBiomeSampler.Sampler` at each pixel's block. ContinentPreview's own shader (climate at the raw
// coordinates, continent and rivers at `getNoiseCoord(floor(x))`) only lines up at frequency 1; the world's levels
// scale every part by their frequency (0.464 for the bundled preset), as the Sampler does.

use super::{
    terraforged::{floor, lerp},
    terraforged_biome_sampler::{BEACH, ClimateSample, SHALLOW_OCEAN},
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_generator_preset::{build, scale_heights, terrain_levels},
    terraforged_settings::{Settings, set_for_thread},
    terraforged_world_preset::preset_json,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// `getBiomeColor`'s shade strength in `ContinentPreview`.
pub const STRENGTH: f32 = 0.2;

/// What `Previewer`'s window shows: its size, zoom (blocks per pixel) and centre (`posX`, `posY`).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for View {
    /// `Previewer`'s opening view: 400x400 (`setPreferredSize`), zoom 1, centred on 0, 0.
    fn default() -> Self {
        Self {
            width: 400,
            height: 400,
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl View {
    /// The window's corner label: the area shown in blocks, `floor(w * zoom) x floor(h * zoom)` (`NoiseUtil.floor`).
    pub fn label(&self) -> String {
        format!(
            "{}x{}",
            floor(self.width as f32 * self.zoom),
            floor(self.height as f32 * self.zoom)
        )
    }

    /// `Previewer.paint`: each pixel's shader colour (0xRRGGBB), row by row. Pixel (x, y) samples
    /// `ox + x * zoom, oy + y * zoom` with `ox = posX - (w >> 1) * zoom` (all f32, Java's order). Rows are spread over
    /// all processors but one (Java splits the image into tiles over all of them); each pixel depends only on its
    /// position, so the split does not change the picture.
    pub fn render(&self, shader: impl Fn(f32, f32) -> u32 + Sync) -> Vec<u32> {
        let ox = self.x - (self.width >> 1) as f32 * self.zoom;
        let oy = self.y - (self.height >> 1) as f32 * self.zoom;
        let mut pixels = vec![0; self.width as usize * self.height as usize];
        let threads =
            std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1));
        let mut groups: Vec<Vec<(usize, &mut [u32])>> = (0..threads).map(|_| Vec::new()).collect();
        for (y, row) in pixels.chunks_mut(self.width as usize).enumerate() {
            groups[y % threads].push((y, row));
        }
        let (shader, zoom) = (&shader, self.zoom);
        std::thread::scope(|scope| {
            for group in groups {
                scope.spawn(move || {
                    for (y, row) in group {
                        let py = oy + y as f32 * zoom;
                        for (x, pixel) in row.iter_mut().enumerate() {
                            *pixel = shader(ox + x as f32 * zoom, py);
                        }
                    }
                });
            }
        });
        pixels
    }
}

/// `/tf preview`'s shader: the world's `IBiomeSampler.Sampler` sample of the block under `(x, y)`
/// (`NoiseUtil.floor`, as ContinentPreview floors its continent coordinates), coloured by [`biome_color`].
pub fn world_color(sampler: &CurrentBiomeSampler<'_>, seed: i32, x: f32, y: f32) -> u32 {
    biome_color(&sampler.get_sample(seed, floor(x), floor(y)), STRENGTH)
}

/// Horizon's Create World map (neither TerraForged tree has one there: 6dd607eb and the 1.18.2-0.3.1 jar show nothing,
/// TerraForged Reimagined draws its own `PreviewWidget` on its settings screen): `view` of the world that `level_seed`
/// and `settings` will generate, drawn as `/tf preview` draws a made world ([`world_color`] at the world's runtime
/// seed, `Seeds.get` of the level seed). The generator is built as the world's pipeline builds it, from TerraForged's
/// preset levels with the `height_scale` setting and with `settings` for this thread, so the picture is the one
/// `/tf preview` gives once the world is made. `cancelled` is asked before each pixel: once it says yes, the rest of
/// the picture is skipped and the result is `None`. `Err` where the bundled preset or a resource does not decode.
pub fn new_world_map(
    level_seed: i64,
    settings: Settings,
    view: View,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Vec<u32>>, String> {
    let preset: serde_json::Value =
        serde_json::from_str(preset_json()).map_err(|error| error.to_string())?;
    let levels = scale_heights(
        terrain_levels(preset["dimensions"]["minecraft:overworld"]["generator"].get("levels"))?,
        settings.height_scale,
    );
    set_for_thread(Some(settings));
    let generator = build(levels);
    set_for_thread(None);
    let generator = generator?;
    generator.capture_seed(level_seed);
    let sampler = CurrentBiomeSampler::new(generator.noise_generator());
    let seed = generator.seed();
    let stopped = AtomicBool::new(false);
    let pixels = view.render(|x, y| {
        if stopped.load(Ordering::Relaxed) || cancelled() {
            stopped.store(true, Ordering::Relaxed);
            return 0;
        }
        world_color(&sampler, seed, x, y)
    });
    Ok((!stopped.into_inner()).then_some(pixels))
}

/// `ColorUtil.getBiomeColor(sample, strength)`: [`color`] shaded by `lerp(1, biomeNoise, strength)`.
pub fn biome_color(sample: &ClimateSample, strength: f32) -> u32 {
    color(sample, lerp(1.0, sample.biome_noise, strength))
}

/// `ColorUtil.getColor`: deep blue up to the shallow ocean, light blue up to the beach and on rivers (`riverNoise <=
/// 0`), else the climate type's colour (`biomes.txt`) shaded. NaN noise passes every test (Java's `<=`).
pub fn color(sample: &ClimateSample, shade: f32) -> u32 {
    if sample.noise.continent_noise <= SHALLOW_OCEAN {
        return 0x0066DD;
    }
    if sample.noise.continent_noise <= BEACH {
        return 0x0099DD;
    }
    if sample.noise.river_noise <= 0.0 {
        return 0x0099DD;
    }
    let [r, g, b] = sample.climate_type.color();
    shade_rgb(r, g, b, shade)
}

/// `ColorUtil.shade(r, g, b, brightness)` as a `TYPE_INT_RGB` image stores it: each channel
/// `NoiseUtil.floor(c * brightness)`, ORed in with Java's int shifts and no masking (a channel outside 0..=255, from a
/// brightness outside 0..=1, spills into its neighbours as in Java), then the image keeps the low 24 bits.
fn shade_rgb(r: u8, g: u8, b: u8, brightness: f32) -> u32 {
    let channel = |c: u8| floor(f32::from(c) * brightness);
    let rgb = (0xFF << 24) | channel(r).wrapping_shl(16) | channel(g).wrapping_shl(8) | channel(b);
    rgb as u32 & 0xFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::{BiomeType, TerrainRegistry},
        terraforged_generator_preset::{build, terrain_levels},
    };
    use super::*;

    fn fnv(values: impl IntoIterator<Item = u32>) -> u64 {
        values.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, v| {
            (hash ^ u64::from(v)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// `.local/terraforged/oracles/preview/` (`run.sh`, after `../noise_generator/run.sh` and
    /// `../current_biome_sampler/run.sh`): 6dd607eb's real `ColorUtil` written to a `TYPE_INT_RGB` `BufferedImage`
    /// and read back, and `Previewer`'s view over the real `IBiomeSampler.Sampler` (the bundled preset's levels,
    /// continent caches bypassed). Interpreted and JIT-compiled runs agree.
    const JAVA: &str = "\
color 3e4ccccd a5653a924c471135
color 3f800000 d9e90af0a45cee95
render 273651 40 40 3f800000 0 0 26e83d5a7e2061bd 40x40
render 273651 32 24 42c00000 0 0 84debdd052265553 3072x2304
render 0 24 24 42800000 c49a5000 44425000 82070cce4a2b0e34 1536x1536
render -17 20 20 3f800000 c3960000 c2240000 76ebe567645f4725 20x20
render 1234 28 28 42160000 459c4000 c60ca000 fa0705ad41009d5 1050x1050
render -2147483648 16 16 3f400000 c1080000 40400000 5d3612e4c3b7ce25 12x12
";

    const MIN: f32 = f32::from_bits(1);
    const CONTINENT: [f32; 11] = [
        f32::NAN,
        -0.0,
        0.0,
        0.25,
        f32::from_bits(0x3e80_0001),
        0.5,
        f32::from_bits(0x3f00_0001),
        0.75,
        1.0,
        2.0,
        -1.0,
    ];
    const RIVER: [f32; 7] = [f32::NAN, -0.0, 0.0, MIN, -MIN, 0.5, 1.0];
    const BIOME: [f32; 16] = [
        0.0,
        0.25,
        0.5,
        0.999,
        1.0,
        f32::NAN,
        -1.0,
        2.0,
        6.0,
        -6.0,
        1e9,
        -1e9,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.123456,
    ];

    fn generator() -> super::super::terraforged_generator_facade::TerraForgedGenerator {
        build(terrain_levels(None).unwrap()).unwrap()
    }

    #[test]
    fn colors_match_6dd607eb_java() {
        // The colours read no terrain: any registry does.
        let terrains = TerrainRegistry::new();
        let mut lines = Vec::new();
        for strength in [0.2f32, 1.0] {
            let mut colors = Vec::new();
            for climate_type in BiomeType::VALUES {
                for continent in CONTINENT {
                    for river in RIVER {
                        for biome in BIOME {
                            let mut sample = ClimateSample::new(&terrains);
                            sample.climate_type = climate_type;
                            sample.noise.continent_noise = continent;
                            sample.noise.river_noise = river;
                            sample.biome_noise = biome;
                            colors.push(biome_color(&sample, strength));
                        }
                    }
                }
            }
            lines.push(format!("color {:x} {:x}", strength.to_bits(), fnv(colors)));
        }
        let java: Vec<&str> = JAVA.lines().filter(|l| l.starts_with("color ")).collect();
        assert_eq!(lines, java);
    }

    #[test]
    fn world_previews_match_6dd607eb_java() {
        let generator = generator();
        let sampler = CurrentBiomeSampler::new(generator.noise_generator());
        for line in JAVA.lines().filter(|l| l.starts_with("render ")) {
            let f: Vec<&str> = line.split(' ').collect();
            let bits = |s: &str| f32::from_bits(u32::from_str_radix(s, 16).unwrap());
            let seed: i32 = f[1].parse().unwrap();
            let view = View {
                width: f[2].parse().unwrap(),
                height: f[3].parse().unwrap(),
                zoom: bits(f[4]),
                x: bits(f[5]),
                y: bits(f[6]),
            };
            let pixels = view.render(|x, y| world_color(&sampler, seed, x, y));
            assert_eq!(
                format!("{:x} {}", fnv(pixels), view.label()),
                format!("{} {}", f[7], f[8]),
                "{line}"
            );
        }
    }

    #[test]
    fn extreme_views_still_render() {
        // Pixel positions past i32 (NoiseUtil.floor saturates as Java's `(int)` does), infinite (an f32 overflow of
        // `(w >> 1) * zoom`) and NaN (`-inf + inf`): the samples are Java's at the clamped blocks, never a panic.
        let generator = generator();
        let sampler = CurrentBiomeSampler::new(generator.noise_generator());
        for (zoom, x, y) in [
            (1.0, 3e9, -3e9),
            (1e30, 0.0, 0.0),
            (f32::MAX, 0.0, 0.0),
            (1.0, f32::MAX, f32::MIN),
        ] {
            let view = View {
                width: 16,
                height: 16,
                zoom,
                x,
                y,
            };
            let pixels = view.render(|x, y| world_color(&sampler, 273_651, x, y));
            assert_eq!(pixels.len(), 256);
        }
    }

    #[test]
    fn shading_spills_like_java_ints() {
        // A brightness of 2 doubles 200 to 400 (0x190): its ninth bit lands in the channel above, as Java ORs it.
        assert_eq!(shade_rgb(0, 200, 0, 2.0), 0x01_90_00);
        assert_eq!(shade_rgb(10, 20, 30, 1.0), 0x0A_14_1E);
        // A negative channel sets every higher bit (NoiseUtil.floor(-5f) is -6).
        assert_eq!(shade_rgb(0, 0, 10, -0.5), 0xFF_FF_FA);
    }

    #[test]
    fn default_view_is_the_window() {
        let view = View::default();
        assert_eq!(
            (view.width, view.height, view.label()),
            (400, 400, "400x400".to_owned())
        );
        assert_eq!(
            View {
                zoom: 0.0001,
                ..view
            }
            .label(),
            "0x0"
        );
    }

    /// The Create World map of seed 777 is `/tf preview`'s picture of the same world: the generator its pipeline
    /// builds (preset levels, `height_scale`), at its runtime seed. The bundled preset's map reads only climate,
    /// continent and river noise, so no setting changes it (`height_scale` stretches heights, not the map;
    /// `flow_rivers` and `basin_erosion` change heights, not the climate sample).
    #[test]
    fn new_world_maps_are_the_world_s_preview() {
        let view = View {
            width: 32,
            height: 32,
            zoom: 128.0,
            x: 0.0,
            y: 0.0,
        };
        let preset: serde_json::Value = serde_json::from_str(preset_json()).unwrap();
        let levels =
            terrain_levels(preset["dimensions"]["minecraft:overworld"]["generator"].get("levels"))
                .unwrap();
        let world = build(levels).unwrap();
        world.capture_seed(777);
        let sampler = CurrentBiomeSampler::new(world.noise_generator());
        let made = view.render(|x, y| world_color(&sampler, world.seed(), x, y));
        let start = std::time::Instant::now();
        let map = new_world_map(777, Settings::default(), view, &|| false)
            .unwrap()
            .unwrap();
        eprintln!("new world map: {:?}", start.elapsed());
        assert_eq!(map, made);
        for settings in [
            Settings {
                height_scale: 1.85,
                ..Settings::default()
            },
            Settings {
                height_scale: 0.5,
                deterministic: true,
                sparse_forests: true,
                corrected_biomes: true,
                erosion_deposits: true,
                eroded_far_terrain: true,
                realistic_vegetation: true,
                altitude_climate: true,
                altitude_climate_strength: 2.0,
                rock_strata: true,
                coastal_landforms: true,
                glacial_landforms: true,
                flow_rivers: true,
                basin_erosion: true,
            },
        ] {
            assert_eq!(
                new_world_map(777, settings, view, &|| false).unwrap(),
                Some(made.clone()),
                "{settings:?}"
            );
        }
        // A long seed keeps its low 32 bits (`Seeds.get`), as the world's generator does.
        assert_eq!(
            new_world_map(777 + (5 << 32), Settings::default(), view, &|| false).unwrap(),
            Some(made.clone())
        );
        assert_ne!(
            new_world_map(778, Settings::default(), view, &|| false).unwrap(),
            Some(made)
        );
    }

    #[test]
    fn cancelled_new_world_maps_stop() {
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let map = new_world_map(
            777,
            Settings::default(),
            View {
                width: 64,
                height: 64,
                zoom: 64.0,
                x: 0.0,
                y: 0.0,
            },
            &|| asked.fetch_add(1, Ordering::Relaxed) >= 10,
        )
        .unwrap();
        assert_eq!(map, None);
        // Every thread stops at its next pixel: far fewer asks than the 4096 pixels.
        assert!(asked.into_inner() < 200);
    }
}
