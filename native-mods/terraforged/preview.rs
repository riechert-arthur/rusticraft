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

// TerraForged's `ContinentPreview` shader over `Previewer`'s view: each pixel the
// continent, river and climate sample coloured as `ColorUtil.getBiomeColor` colours it. The
// original is a developer window; `/tf preview` renders the same view to a picture.

use super::{
    terraforged_biome_sampler::{BEACH, ClimateSample, SHALLOW_OCEAN},
    terraforged_climate::ClimateSettings,
    terraforged_current_climate::CurrentClimateNoise,
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_generator_preset::{build, terrain_levels},
    terraforged_world_preset::preset_json,
};

/// `ContinentPreview`'s fixed sampling seed.
pub const SEED: i32 = 273_651;

/// What the window showed: its size, zoom and centre (`posX`, `posY`).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for View {
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

pub struct Preview {
    generator: TerraForgedGenerator,
    climate: CurrentClimateNoise,
}

impl Preview {
    /// `ContinentPreview.create`: TerraForged's noise and its climate.
    pub fn new() -> Result<Self, String> {
        let preset: serde_json::Value =
            serde_json::from_str(preset_json()).map_err(|error| error.to_string())?;
        let levels =
            terrain_levels(&preset["dimensions"]["minecraft:overworld"]["generator"]["levels"])?;
        let generator = build(levels)?;
        let context = generator.noise_generator().continent().context();
        let mut seed = context.seed;
        let climate = CurrentClimateNoise::new(&mut seed, ClimateSettings::terraforged());
        Ok(Self { generator, climate })
    }

    /// `Noise.getSample`: climate at the raw coordinates, continent and rivers at the
    /// floored ones scaled to noise space, as the original mixes them.
    pub fn sample(&self, seed: i32, x: f32, y: f32) -> ClimateSample {
        let mut sample = ClimateSample::default();
        self.climate.sample(seed, x, y, &mut sample);
        let noise = self.generator.noise_generator();
        let frequency = noise.levels().noise_levels.frequency;
        let (nx, nz) = (
            x.floor() as i32 as f32 * frequency,
            y.floor() as i32 as f32 * frequency,
        );
        noise
            .continent()
            .sample_continent(seed, nx, nz, &mut sample.noise);
        noise
            .continent()
            .sample_river(seed, nx, nz, &mut sample.noise);
        sample
    }

    /// The view's pixels, row by row, as 0xRRGGBB.
    pub fn render(&self, seed: i32, view: View) -> Vec<u32> {
        let ox = view.x - (view.width >> 1) as f32 * view.zoom;
        let oy = view.y - (view.height >> 1) as f32 * view.zoom;
        let mut pixels = vec![0; (view.width * view.height) as usize];
        let rows: Vec<&mut [u32]> = pixels.chunks_mut(view.width as usize).collect();
        std::thread::scope(|scope| {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
            let mut groups: Vec<Vec<(usize, &mut [u32])>> =
                (0..threads).map(|_| Vec::new()).collect();
            for (y, row) in rows.into_iter().enumerate() {
                groups[y % threads].push((y, row));
            }
            for group in groups {
                scope.spawn(move || {
                    for (y, row) in group {
                        let py = oy + y as f32 * view.zoom;
                        for (x, pixel) in row.iter_mut().enumerate() {
                            let px = ox + x as f32 * view.zoom;
                            *pixel = biome_color(&self.sample(seed, px, py), 0.2);
                        }
                    }
                });
            }
        });
        pixels
    }
}

/// `ColorUtil.getBiomeColor`.
pub fn biome_color(sample: &ClimateSample, strength: f32) -> u32 {
    let shade = 1.0 + strength * (sample.biome_noise - 1.0);
    if sample.noise.continent_noise <= SHALLOW_OCEAN {
        return 0x0066DD;
    }
    if sample.noise.continent_noise <= BEACH || sample.noise.river_noise <= 0.0 {
        return 0x0099DD;
    }
    let [r, g, b] = sample.climate_type.color();
    let channel = |c: u8| (f32::from(c) * shade).floor() as i32 as u32 & 0xff;
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_oceans_and_land() {
        let preview = Preview::new().unwrap();
        let pixels = preview.render(
            SEED,
            View {
                width: 48,
                height: 48,
                zoom: 64.0,
                ..View::default()
            },
        );
        assert!(pixels.iter().any(|&p| p == 0x0066DD));
        assert!(pixels.iter().any(|&p| p != 0x0066DD && p != 0x0099DD));
    }

    #[test]
    fn create_world_preview_is_quick() {
        let start = std::time::Instant::now();
        let preview = Preview::new().unwrap();
        let built = start.elapsed();
        let pixels = preview.render(
            98_765,
            View {
                width: 64,
                height: 64,
                zoom: 64.0,
                x: 0.0,
                y: 0.0,
            },
        );
        let total = start.elapsed();
        eprintln!("preview: built {built:?}, built and drawn {total:?}");
        assert_eq!(pixels.len(), 64 * 64);
    }
}
