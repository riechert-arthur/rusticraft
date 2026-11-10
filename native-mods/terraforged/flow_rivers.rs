// Horizon's flow-routed rivers (in neither TerraForged tree), the `flow_rivers` setting (#278): streams and rivers
// where the drainage map (`terraforged_drainage`) collects water, carved down its valleys into the sea or into
// TerraForged's own rivers, which stay as they are.
//
// Clean-room, from published methods: a channel starts where the area draining through a cell passes a threshold
// (the "constant critical support area" of Montgomery & Dietrich 1988, as in O'Callaghan & Mark 1984), and its width
// and depth grow with that area as downstream hydraulic geometry has them grow with discharge, w ∝ Q^0.5 and
// d ∝ Q^0.4 (Leopold & Maddock 1953), discharge taken proportional to area. The channel's line is a quadratic B-spline
// through the D8 path's cell centres (Chaikin's corner cutting), each centre moved by a seeded offset so the channels
// don't run along the grid. The cross-section (bed, banks, then a valley falling back to the terrain) follows
// TerraForged's `RiverCarver`; the water surface stays stepped, one level per column, as TerraForged's rivers have it.
//
// Every value is a pure function of the seed, the settings and the position: channels come from the drainage map's
// tiles (themselves pure) through fixed 128-block patches, memoised in a lossy cache.

use super::terraforged::{clamp, lerp};
use super::terraforged_basin_erosion::{BasinErosion, BasinMap, BasinMemo};
use super::terraforged_continent_points::BEACH;
use super::terraforged_drainage::{Column, Drainage, FlowCell, L1, SEA, TileMemo};
use super::terraforged_engine::{Terrain, TerrainProperties};
use super::terraforged_eroded_noise_generator::shaped_sample;
use super::terraforged_glacial_landforms::Glaciers;
use super::terraforged_lossy_cache::ConcurrentLossyCache;
use super::terraforged_noise_generator::NoiseGenerator;
use super::terraforged_settings::Settings;
use super::terraforged_terrain_blender::Blender;
use super::terraforged_terrain_data::NoiseSample;
use super::terraforged_terrain_levels::TerrainLevels;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// The area (square blocks) that must drain through a cell for a channel to start there: 0.6 km².
pub const MIN_AREA: f64 = 600_000.0;
/// A channel's width at its head, in blocks.
pub const HEAD_WIDTH: f32 = 2.0;
/// The widest channel, in blocks (TerraForged's own rivers are wider).
pub const MAX_WIDTH: f32 = 20.0;
/// The deepest bed below the water, in blocks.
pub const MAX_DEPTH: f32 = 7.0;
/// How far below the drainage map's (filled) height the water stands, in blocks.
const WATER_DROP: f32 = 1.5;
/// A cell whose filled height stands this far over its height lies in a depression: no channel there.
const DEPRESSION: f32 = 1.0;
/// Cells downstream whose filled height a cell's water level takes the highest of, so the level never rises
/// downstream where two drainage tiles fill a depression by their border to different heights.
const LOOKAHEAD: usize = 8;
/// The largest offset of a channel's line from its cells' centres, along each axis, in blocks.
const JITTER: f32 = 5.0;
/// Banks within this many blocks of the water are raised to the water's height where they dip below it (by less
/// than [`LEVEE_DEPTH`]), so no water stands beside a lower column.
const LEVEE: f32 = 2.0;
const LEVEE_DEPTH: f32 = 4.0;
/// L1 cells per patch side: the channels near 128 blocks are extracted together.
const PATCH: i32 = 8;
/// L1 cells around a patch whose channels can reach into it (a valley half-width plus a curve's reach).
const MARGIN: i32 = 6;

/// A channel's width for `area` square blocks draining through it: [`HEAD_WIDTH`] at [`MIN_AREA`], ∝ area^0.5.
pub fn width(area: f64) -> f32 {
    (f64::from(HEAD_WIDTH) * (area / MIN_AREA).sqrt())
        .clamp(f64::from(HEAD_WIDTH), f64::from(MAX_WIDTH)) as f32
}

/// The bed's depth below the water at the channel's middle: 2 blocks at [`MIN_AREA`], growing ∝ area^0.4.
pub fn depth(area: f64) -> f32 {
    (1.0 + (area / MIN_AREA).max(1.0).powf(0.4)).min(f64::from(MAX_DEPTH)) as f32
}

/// How far from the channel's line the valley reaches, for a channel `width` wide.
pub fn valley(width: f32) -> f32 {
    width * 0.5 + 4.0 + 1.5 * width
}

/// The channel nearest a column, as [`Channels::reach`] finds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    /// Blocks from the channel's line.
    pub distance: f32,
    pub width: f32,
    pub depth: f32,
    /// [`valley`] of the width.
    pub valley: f32,
    /// The water's top block.
    pub water: i32,
}

/// A column of `height` blocks carved by `reach`: the new height and whether it lies in the channel (under its
/// water). The bed is parabolic, at least a block under the water; outside it the ground is lowered towards one
/// block over the water, the less the farther out (squared, as TerraForged's U-shaped valleys), back to `height` at
/// the valley's edge. Never raised, but for banks next to the water that dip under it ([`LEVEE`]).
pub fn cross_section(reach: &Reach, height: f32) -> (f32, bool) {
    let water = reach.water as f32;
    let half = reach.width * 0.5;
    if reach.distance <= half {
        let across = reach.distance / half;
        let bed = water - 1.0 - (reach.depth - 1.0) * (1.0 - across * across);
        return (height.min(bed), true);
    }
    let out = ((reach.distance - half) / (reach.valley - half)).min(1.0);
    let mut carved = lerp(height.min(water + 1.0), height, out * out);
    if reach.distance <= half + LEVEE && carved < water && carved > water - LEVEE_DEPTH {
        carved = water + 0.5;
    }
    (carved, false)
}

/// TerraForged's `getValleyNoise` over a reach: the river noise's factor, 0.05 at the water's edge up to 1 at the
/// valley's.
pub fn valley_noise(reach: &Reach) -> f32 {
    let half = reach.width * 0.5;
    let alpha = clamp((reach.distance - half) / (reach.valley - half), 0.0, 1.0);
    clamp(0.05 + alpha / 0.95, 0.0, 1.0)
}

/// One end of a channel curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub x: f32,
    pub z: f32,
    /// The water level before [`WATER_DROP`], in blocks.
    pub level: f32,
    /// Square blocks draining through.
    pub area: f64,
}

/// One edge of the D8 path, from the middle of a cell's step to the middle of the next (or to an outlet's centre),
/// bending through the cell between.
struct Curve {
    start: Node,
    control: (f32, f32),
    end: Node,
    /// x and z bounds, widened by the valley.
    bounds: [f32; 4],
}

impl Curve {
    const PIECES: usize = 8;

    fn at(&self, t: f32) -> (f32, f32) {
        let u = 1.0 - t;
        (
            u * u * self.start.x + 2.0 * u * t * self.control.0 + t * t * self.end.x,
            u * u * self.start.z + 2.0 * u * t * self.control.1 + t * t * self.end.z,
        )
    }

    /// The distance from (`x`, `z`) and the curve's parameter there, over [`Self::PIECES`] straight pieces.
    fn nearest(&self, x: f32, z: f32) -> (f32, f32) {
        let mut best = (f32::INFINITY, 0.0);
        let mut from = self.at(0.0);
        for piece in 0..Self::PIECES {
            let to = self.at((piece + 1) as f32 / Self::PIECES as f32);
            let (dx, dz) = (to.0 - from.0, to.1 - from.1);
            let length = dx * dx + dz * dz;
            let along = if length > 0.0 {
                (((x - from.0) * dx + (z - from.1) * dz) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, pz) = (from.0 + dx * along, from.1 + dz * along);
            let distance = ((x - px) * (x - px) + (z - pz) * (z - pz)).sqrt();
            if distance < best.0 {
                best = (distance, (piece as f32 + along) / Self::PIECES as f32);
            }
            from = to;
        }
        best
    }
}

/// The channels of one patch: every curve that can reach into it.
pub struct Patch {
    curves: Vec<Curve>,
}

/// The cells (and through them the drainage tiles) one patch build has looked up.
#[derive(Default)]
struct Memo {
    cells: HashMap<(i32, i32), FlowCell>,
    tiles: TileMemo,
    basins: BasinMemo,
}

/// The channel network over a drainage map.
pub struct Channels<S> {
    seed: i32,
    sea_level: i32,
    drainage: Arc<Drainage<S>>,
    /// Horizon's `basin_erosion` over the same drainage map, if it is on: the water follows the eroded valleys.
    basins: Option<Arc<BasinMap<S>>>,
    patches: ConcurrentLossyCache<Patch>,
}

fn hash(seed: i32, x: i32, z: i32, salt: u32) -> f32 {
    let mut h = (seed as u32 ^ salt).wrapping_mul(0x27d4_eb2d);
    h ^= (x as u32).wrapping_mul(0x9e37_79b1);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca77);
    h ^= (z as u32).wrapping_mul(0xc2b2_ae35);
    h = (h ^ (h >> 13)).wrapping_mul(0x2c1b_3c6d);
    (h >> 8) as f32 / (1 << 24) as f32
}

impl<S: Fn(i32, i32) -> Column> Channels<S> {
    /// Patches cached (a few KB each).
    pub const PATCHES: i32 = 1024;

    /// The channels over `source`'s drainage map (see [`Drainage::new`]); water never stands below `sea_level`.
    pub fn new(seed: i32, sea_level: i32, source: S) -> Self {
        Self::over(seed, sea_level, Arc::new(Drainage::new(seed, source)), None)
    }

    /// The channels over `drainage`, their water lowered with `basins` (whose drainage map must be the same).
    pub fn over(
        seed: i32,
        sea_level: i32,
        drainage: Arc<Drainage<S>>,
        basins: Option<Arc<BasinMap<S>>>,
    ) -> Self {
        Self {
            seed,
            sea_level,
            drainage,
            basins,
            patches: ConcurrentLossyCache::new(Self::PATCHES, 8),
        }
    }

    pub fn drainage(&self) -> &Drainage<S> {
        &self.drainage
    }

    /// L1 cell (`x`, `z`): [`Drainage::flow`] at its centre.
    fn cell(&self, memo: &mut Memo, x: i32, z: i32) -> FlowCell {
        if let Some(cell) = memo.cells.get(&(x, z)) {
            return *cell;
        }
        let cell = self
            .drainage
            .flow_in(&mut memo.tiles, L1.center(x), L1.center(z));
        memo.cells.insert((x, z), cell);
        cell
    }

    /// Where the channel's line passes cell (`x`, `z`): its centre, moved by up to [`JITTER`] blocks.
    pub fn point(&self, x: i32, z: i32) -> (f32, f32) {
        let offset = |salt| (hash(self.seed, x, z, salt) * 2.0 - 1.0) * JITTER;
        (
            L1.center(x) as f32 + offset(0x5bd1_e995),
            L1.center(z) as f32 + offset(0x1b87_3593),
        )
    }

    /// Whether a channel runs through `cell`: enough water, on to a neighbour, not in a filled depression.
    fn is_channel(cell: &FlowCell) -> bool {
        cell.accumulation >= MIN_AREA
            && cell.downstream().is_some()
            && cell.filled - cell.height <= DEPRESSION
    }

    /// The water level of cell (`x`, `z`) before [`WATER_DROP`]: the highest filled height (lowered by the basin
    /// erosion, if on) over it and [`LOOKAHEAD`] cells downstream, and at least the sea level plus the drop.
    fn level(&self, memo: &mut Memo, x: i32, z: i32) -> f32 {
        let (mut x, mut z) = (x, z);
        let mut level = self.sea_level as f32 + WATER_DROP;
        for _ in 0..=LOOKAHEAD {
            let cell = self.cell(memo, x, z);
            let lowered = match &self.basins {
                Some(basins) => cell.filled + basins.cell(&mut memo.basins, x, z),
                None => cell.filled,
            };
            level = level.max(lowered);
            let Some((dx, dz)) = cell.downstream() else {
                break;
            };
            (x, z) = (x + dx, z + dz);
        }
        level
    }

    /// The middle of the step out of cell (`x`, `z`), where the curve into it ends and the curve out of it begins:
    /// `None` unless a channel runs through the cell.
    pub fn node(&self, x: i32, z: i32) -> Option<Node> {
        self.node_with(&mut Memo::default(), x, z)
    }

    fn node_with(&self, memo: &mut Memo, x: i32, z: i32) -> Option<Node> {
        let cell = self.cell(memo, x, z);
        if !Self::is_channel(&cell) {
            return None;
        }
        let (dx, dz) = cell.downstream()?;
        let next = self.cell(memo, x + dx, z + dz);
        let (a, b) = (self.point(x, z), self.point(x + dx, z + dz));
        Some(Node {
            x: (a.0 + b.0) * 0.5,
            z: (a.1 + b.1) * 0.5,
            level: (self.level(memo, x, z) + self.level(memo, x + dx, z + dz)) * 0.5,
            area: (cell.accumulation + next.accumulation) * 0.5,
        })
    }

    /// The curve out of cell (`x`, `z`): from its [`Self::node`] through the next cell's point to the next cell's
    /// node, or straight to the next cell's centre when that is an outlet (the sea, or one of TerraForged's rivers
    /// in [`FlowRivers`]' source). `None` without a channel, or where the next cell lies in a depression.
    fn curve(&self, memo: &mut Memo, x: i32, z: i32) -> Option<Curve> {
        let start = self.node_with(memo, x, z)?;
        let (dx, dz) = self.cell(memo, x, z).downstream()?;
        let (nx, nz) = (x + dx, z + dz);
        let next = self.cell(memo, nx, nz);
        let (control, end) = if next.direction == SEA {
            let (px, pz) = (L1.center(nx) as f32, L1.center(nz) as f32);
            let end = Node {
                x: px,
                z: pz,
                level: self.level(memo, nx, nz),
                area: next.accumulation,
            };
            (((start.x + px) * 0.5, (start.z + pz) * 0.5), end)
        } else {
            (self.point(nx, nz), self.node_with(memo, nx, nz)?)
        };
        let reach = valley(width(start.area.max(end.area))) + 1.0;
        let xs = [start.x, control.0, end.x];
        let zs = [start.z, control.1, end.z];
        let bounds = [
            xs.iter().copied().fold(f32::INFINITY, f32::min) - reach,
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max) + reach,
            zs.iter().copied().fold(f32::INFINITY, f32::min) - reach,
            zs.iter().copied().fold(f32::NEG_INFINITY, f32::max) + reach,
        ];
        Some(Curve {
            start,
            control,
            end,
            bounds,
        })
    }

    fn patch(&self, x: i32, z: i32) -> Arc<Patch> {
        let key = (i64::from(x) << 32) | i64::from(z as u32);
        self.patches.compute_if_absent(self.seed, key, |_, _| {
            let mut memo = Memo::default();
            let mut curves = Vec::new();
            let (cx, cz) = (x * PATCH, z * PATCH);
            for z in cz - MARGIN..cz + PATCH + MARGIN {
                for x in cx - MARGIN..cx + PATCH + MARGIN {
                    curves.extend(self.curve(&mut memo, x, z));
                }
            }
            let (min_x, min_z) = ((cx * L1.cell) as f32, (cz * L1.cell) as f32);
            let side = (PATCH * L1.cell) as f32;
            curves.retain(|curve| {
                let [x0, x1, z0, z1] = curve.bounds;
                x1 >= min_x && x0 <= min_x + side && z1 >= min_z && z0 <= min_z + side
            });
            Patch { curves }
        })
    }

    /// The channel whose water's edge lies nearest block (`x`, `z`), if the column lies in its valley.
    pub fn reach(&self, x: i32, z: i32) -> Option<Reach> {
        let size = PATCH * L1.cell;
        let patch = self.patch(x.div_euclid(size), z.div_euclid(size));
        let (px, pz) = (x as f32 + 0.5, z as f32 + 0.5);
        let mut best: Option<(f32, Reach)> = None;
        for curve in &patch.curves {
            let [x0, x1, z0, z1] = curve.bounds;
            if px < x0 || px > x1 || pz < z0 || pz > z1 {
                continue;
            }
            let (distance, t) = curve.nearest(px, pz);
            let area = curve.start.area + (curve.end.area - curve.start.area) * f64::from(t);
            let width = width(area);
            let valley = valley(width);
            if distance >= valley {
                continue;
            }
            let edge = distance - width * 0.5;
            if best.is_some_and(|(nearest, _)| nearest <= edge) {
                continue;
            }
            let level = lerp(curve.start.level, curve.end.level, t);
            let water = ((level - WATER_DROP).floor() as i32).max(self.sea_level);
            best = Some((
                edge,
                Reach {
                    distance,
                    width,
                    depth: depth(area),
                    valley,
                    water,
                },
            ));
        }
        best.map(|(_, reach)| reach)
    }
}

/// A boxed drainage source, shareable between generation threads.
pub type Source = Box<dyn Fn(i32, i32) -> Column + Send + Sync>;

/// The drainage source of [`FlowRivers`] and Horizon's basin erosion: its own copy of TerraForged's un-eroded
/// generator (without `basin_erosion`, so the map never routes the valleys it cuts), rivers included, and the world's
/// coasts and glaciers, which reshape the ground the channels run over.
pub struct RiverSource {
    generator: NoiseGenerator,
    blenders: Mutex<Vec<Blender>>,
    /// `coastal_landforms` (holding whether `rock_strata` is on) and `glacial_landforms`' glaciers, as the world's
    /// [`ErodedNoiseGenerator`](super::terraforged_eroded_noise_generator::ErodedNoiseGenerator) has them.
    coasts: Option<bool>,
    glaciers: Option<Glaciers>,
}

impl RiverSource {
    /// TerraForged's un-eroded sample of block (`x`, `z`), its rivers carved, as the distant terrain has it: with
    /// `coastal_landforms` the coast's landform, with `glacial_landforms` the troughs (`shaped_sample`), so a channel
    /// never takes its water level from ground a trough or a tidal flat lowered. One of TerraForged's river or lake
    /// beds is an outlet at its water level, so the channels end in TerraForged's rivers; a failed sample is sea.
    pub fn column(&self, seed: i32, x: i32, z: i32) -> Column {
        let levels = self.generator.levels();
        let mut blender = self
            .blenders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop()
            .unwrap_or_else(|| self.generator.blender_resource());
        let sampled = if self.coasts.is_none() && self.glaciers.is_none() {
            let mut sample = NoiseSample::new(&self.generator.continent().context().terrain);
            let sampled = self
                .generator
                .sample(seed, x, z, &mut sample, &mut blender)
                .is_ok();
            sampled.then_some(sample)
        } else {
            shaped_sample(
                &self.generator,
                self.coasts,
                self.glaciers.as_ref(),
                seed,
                (x, z),
                &mut blender,
            )
            .ok()
        };
        self.blenders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(blender);
        let sea = levels.sea_level as f32;
        let Some(sample) = sampled else {
            return Column {
                height: sea,
                sea: true,
            };
        };
        let height = levels.scaled_height(sample.height_noise);
        if sample.terrain_type.is_river() || sample.terrain_type.is_lake() {
            let water = levels.scaled_base_level(sample.base_noise).floor();
            return Column {
                height: height.min(water.max(sea)),
                sea: true,
            };
        }
        Column {
            height,
            sea: height < sea,
        }
    }
}

/// The world's drainage map over [`RiverSource`], shared by the settings that read it (`flow_rivers`,
/// `basin_erosion`), one per runtime seed.
pub struct RiverDrainage {
    source: Arc<RiverSource>,
    map: RwLock<Option<(i32, Arc<Drainage<Source>>)>>,
}

impl RiverDrainage {
    /// The map over `generator`, a second copy of the world's un-eroded generator (built the same way, without
    /// Horizon's settings), without Horizon's other settings.
    pub fn new(generator: NoiseGenerator) -> Self {
        Self::with_settings(generator, super::terraforged_settings::FAITHFUL)
    }

    /// [`Self::new`] with the world's `settings`: the map samples the ground `coastal_landforms` and
    /// `glacial_landforms` reshape, as the chunks have it.
    pub fn with_settings(generator: NoiseGenerator, settings: Settings) -> Self {
        let glaciers = settings
            .glacial_landforms
            .then(|| Glaciers::new(&generator, settings));
        Self {
            source: Arc::new(RiverSource {
                generator,
                blenders: Mutex::new(Vec::new()),
                coasts: settings.coastal_landforms.then_some(settings.rock_strata),
                glaciers,
            }),
            map: RwLock::new(None),
        }
    }

    pub fn sea_level(&self) -> i32 {
        self.source.generator.levels().sea_level
    }

    /// The map for runtime `seed` (a world has one; another seed replaces it).
    pub fn map(&self, seed: i32) -> Arc<Drainage<Source>> {
        if let Some((cached, map)) = &*self.map.read().unwrap_or_else(|e| e.into_inner())
            && *cached == seed
        {
            return Arc::clone(map);
        }
        let mut slot = self.map.write().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, map)) = &*slot
            && *cached == seed
        {
            return Arc::clone(map);
        }
        let source = Arc::clone(&self.source);
        let map = Arc::new(Drainage::new(
            seed,
            Box::new(move |x, z| source.column(seed, x, z)) as Source,
        ));
        *slot = Some((seed, Arc::clone(&map)));
        map
    }
}

/// The `flow_rivers` setting's carver, which [`NoiseGenerator`] runs after TerraForged's rivers.
pub struct FlowRivers {
    drainage: Arc<RiverDrainage>,
    basins: Option<Arc<BasinErosion>>,
    channels: RwLock<Option<(i32, Arc<Channels<Source>>)>>,
}

impl FlowRivers {
    /// The carver over `generator`, a second copy of the world's un-eroded generator (built the same way, without
    /// this carver): the drainage map samples it, without Horizon's other settings.
    pub fn new(generator: NoiseGenerator) -> Self {
        Self::with_settings(generator, super::terraforged_settings::FAITHFUL)
    }

    /// [`Self::new`] with the world's `settings`: the drainage map samples the ground `coastal_landforms` and
    /// `glacial_landforms` reshape, as the chunks have it.
    pub fn with_settings(generator: NoiseGenerator, settings: Settings) -> Self {
        Self::over(
            Arc::new(RiverDrainage::with_settings(generator, settings)),
            None,
        )
    }

    /// The carver over the shared `drainage` map, its water following `basins`' valleys if that setting is on (over
    /// the same map).
    pub fn over(drainage: Arc<RiverDrainage>, basins: Option<Arc<BasinErosion>>) -> Self {
        Self {
            drainage,
            basins,
            channels: RwLock::new(None),
        }
    }

    /// The channels for runtime `seed` (a world has one; another seed replaces them).
    pub fn channels(&self, seed: i32) -> Arc<Channels<Source>> {
        if let Some((cached, channels)) = &*self.channels.read().unwrap_or_else(|e| e.into_inner())
            && *cached == seed
        {
            return Arc::clone(channels);
        }
        let mut slot = self.channels.write().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, channels)) = &*slot
            && *cached == seed
        {
            return Arc::clone(channels);
        }
        let channels = Arc::new(Channels::over(
            seed,
            self.drainage.sea_level(),
            self.drainage.map(seed),
            self.basins.as_ref().map(|basins| basins.map(seed)),
        ));
        *slot = Some((seed, Arc::clone(&channels)));
        channels
    }

    /// Carves block (`x`, `z`)'s `sample` (TerraForged's rivers already carved, heights as `levels` scale them):
    /// in a channel the column becomes `river` terrain, its water level the channel's (its base noise) and its river
    /// noise 0; across the valley the height is lowered and the river noise multiplied as TerraForged's carver does.
    /// The sea, beaches and TerraForged's own rivers and lakes are left alone.
    pub fn carve(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        levels: TerrainLevels,
        river: &Terrain,
        sample: &mut NoiseSample,
    ) {
        if sample.continent_noise < BEACH
            || sample.terrain_type.is_river()
            || sample.terrain_type.is_lake()
        {
            return;
        }
        let Some(reach) = self.channels(seed).reach(x, z) else {
            return;
        };
        let max_y = levels.max_y as f32;
        let (height, channel) = cross_section(&reach, levels.scaled_height(sample.height_noise));
        sample.height_noise = height / max_y;
        if channel {
            sample.terrain_type = river.clone();
            sample.river_noise = 0.0;
            sample.base_noise = water_base_noise(levels, reach.water);
        } else {
            sample.river_noise *= valley_noise(&reach);
        }
    }
}

/// The base noise whose base height (`TerrainLevels::scaled_base_level`, floored) is block `water`.
pub fn water_base_noise(levels: TerrainLevels, water: i32) -> f32 {
    let noise = levels.noise_levels;
    ((water as f32 + 0.5) / levels.max_y as f32 - noise.height_min) / noise.base_range
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pure test landscape: a sea to the west, land rising east with a valley along z = 520 and value-noise hills
    /// that send tributaries into it.
    fn landscape(x: i32, z: i32) -> Column {
        fn hash(x: i32, z: i32) -> f32 {
            let h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (z as u32).wrapping_mul(0x85eb_ca77);
            let h = (h ^ (h >> 15)).wrapping_mul(0x2c1b_3c6d);
            (h >> 8) as f32 / (1 << 24) as f32
        }
        let value = |s: i32| {
            let (fx, fz) = (x as f32 / s as f32, z as f32 / s as f32);
            let (ix, iz) = (fx.floor() as i32, fz.floor() as i32);
            let (tx, tz) = (fx - ix as f32, fz - iz as f32);
            let a = hash(ix, iz) + (hash(ix + 1, iz) - hash(ix, iz)) * tx;
            let b = hash(ix, iz + 1) + (hash(ix + 1, iz + 1) - hash(ix, iz + 1)) * tx;
            a + (b - a) * tz
        };
        let height = 62.0
            + 0.02 * x as f32
            + 0.05 * (z - 520).abs() as f32
            + 30.0 * value(700)
            + 6.0 * value(90)
            - 40.0;
        Column {
            height,
            sea: height < 62.0,
        }
    }

    fn channels() -> Channels<fn(i32, i32) -> Column> {
        Channels::new(3, 62, landscape)
    }

    /// The valley's channel, followed downstream from x = 4000 to the sea: its cells and nodes.
    fn main_channel(channels: &Channels<fn(i32, i32) -> Column>) -> Vec<((i32, i32), Node)> {
        let (mut x, mut z) = (L1.cell_of(4000), L1.cell_of(520));
        let mut path = Vec::new();
        loop {
            let cell = channels.drainage().flow(L1.center(x), L1.center(z));
            let Some((dx, dz)) = cell.downstream() else {
                break;
            };
            if let Some(node) = channels.node(x, z) {
                path.push(((x, z), node));
            }
            (x, z) = (x + dx, z + dz);
        }
        path
    }

    #[test]
    fn channels_never_climb_and_widen_smoothly_across_tile_borders() {
        let channels = channels();
        let path = main_channel(&channels);
        assert!(path.len() > 100, "{}", path.len());
        let mut crossings = 0;
        for pair in path.windows(2) {
            let ((a, from), (b, to)) = (pair[0], pair[1]);
            assert!(to.level <= from.level, "{a:?} {from:?} -> {b:?} {to:?}");
            // A channel only gains water downstream, and its width changes by little at each step, borders too.
            let (w0, w1) = (width(from.area), width(to.area));
            assert!(w1 >= w0 - 0.05 * w0, "{a:?} {w0} -> {b:?} {w1}");
            if L1.tile_of(a.0) != L1.tile_of(b.0) || L1.tile_of(a.1) != L1.tile_of(b.1) {
                crossings += 1;
                assert!((w1 - w0).abs() <= 1.0, "border {a:?} {w0} -> {b:?} {w1}");
            }
        }
        assert!(crossings >= 2, "{crossings}");
        // It reaches the sea at the sea's level, as a wide river.
        let (_, last) = path[path.len() - 1];
        assert!(last.level <= 62.0 + WATER_DROP + 2.0, "{last:?}");
        assert!(width(last.area) > 3.0 * HEAD_WIDTH, "{last:?}");
        // Along its line the water is the nodes' (the reach there is the channel's middle).
        for &(_, node) in path.iter().step_by(7) {
            let reach = channels
                .reach(node.x.floor() as i32, node.z.floor() as i32)
                .unwrap();
            assert!(
                reach.distance < 1.0 && reach.water >= 62,
                "{node:?} {reach:?}"
            );
            assert!(
                (reach.water as f32 - (node.level - WATER_DROP)).abs() <= 1.0,
                "{node:?} {reach:?}"
            );
        }
    }

    #[test]
    fn the_water_level_never_rises_downstream_along_columns() {
        // Block by block along the channel's curves, the stepped water never rises, and the column there is in
        // a channel (the main one, or a tributary at a confluence) at its line.
        let channels = channels();
        let mut memo = Memo::default();
        let mut last = i32::MAX;
        for ((x, z), _) in main_channel(&channels) {
            let Some(curve) = channels.curve(&mut memo, x, z) else {
                continue;
            };
            for k in 0..24 {
                let t = k as f32 / 24.0;
                let (px, pz) = curve.at(t);
                let water = ((lerp(curve.start.level, curve.end.level, t) - WATER_DROP).floor()
                    as i32)
                    .max(62);
                assert!(water <= last, "{px} {pz}: {water} after {last}");
                last = water;
                let reach = channels
                    .reach(px.floor() as i32, pz.floor() as i32)
                    .unwrap();
                assert!(reach.distance <= 1.5, "{px} {pz}: {reach:?}");
            }
        }
        assert_eq!(last, 62);
    }

    #[test]
    fn tributaries_join_the_valley() {
        // Over a 1 km stretch of the valley, channels come in from the sides (more channel cells than the valley's
        // own path), and every channel cell flows on to a channel cell or the sea.
        let channels = channels();
        let mut memo = Memo::default();
        let (mut side, mut stuck) = (0, 0);
        for z in L1.cell_of(520 - 600)..L1.cell_of(520 + 600) {
            for x in L1.cell_of(2000)..L1.cell_of(3000) {
                if channels.node_with(&mut memo, x, z).is_none() {
                    continue;
                }
                side += usize::from((L1.center(z) - 520).abs() > 160);
                let (dx, dz) = channels.cell(&mut memo, x, z).downstream().unwrap();
                let next = channels.cell(&mut memo, x + dx, z + dz);
                stuck += usize::from(
                    next.direction != SEA
                        && channels.node_with(&mut memo, x + dx, z + dz).is_none()
                        && next.filled - next.height <= DEPRESSION,
                );
            }
        }
        assert!(side > 20, "{side}");
        assert_eq!(stuck, 0);
    }

    #[test]
    fn reaches_are_the_same_in_any_order_on_any_threads() {
        // A row of chunks across the valley and an L1 border (x = 3072), asked in two orders from four threads
        // on a fresh map, against one thread in order.
        let columns: Vec<(i32, i32)> = (0..12 * 16)
            .flat_map(|x| (480..560).step_by(3).map(move |z| (2976 + x, z)))
            .collect();
        let snapshot = |channels: &Channels<fn(i32, i32) -> Column>| -> Vec<Option<Reach>> {
            columns.iter().map(|&(x, z)| channels.reach(x, z)).collect()
        };
        let reference = snapshot(&channels());
        assert!(reference.iter().filter(|r| r.is_some()).count() > 100);
        let fresh = channels();
        std::thread::scope(|scope| {
            for thread in 0..4 {
                let (fresh, columns) = (&fresh, &columns);
                scope.spawn(move || {
                    let n = columns.len();
                    for k in 0..n {
                        let i = if thread % 2 == 0 {
                            n - 1 - k
                        } else {
                            (k * 7 + thread) % n
                        };
                        fresh.reach(columns[i].0, columns[i].1);
                    }
                });
            }
        });
        assert_eq!(snapshot(&fresh), reference);
    }

    #[test]
    fn cross_sections_cut_a_bed_banks_and_a_valley() {
        let reach = |distance| Reach {
            distance,
            width: 8.0,
            depth: 3.0,
            valley: valley(8.0),
            water: 70,
        };
        // The bed: under the water everywhere in the channel, deepest in the middle.
        assert_eq!(cross_section(&reach(0.0), 80.0), (67.0, true));
        assert_eq!(cross_section(&reach(4.0), 80.0), (69.0, true));
        assert_eq!(cross_section(&reach(2.0), 60.0), (60.0, true));
        // The banks a block over the water, rising back to the ground at the valley's edge; low banks raised.
        assert_eq!(cross_section(&reach(4.001), 80.0).0.floor(), 71.0);
        assert_eq!(cross_section(&reach(5.0), 69.0), (70.5, false));
        assert_eq!(cross_section(&reach(5.0), 64.0), (64.0, false));
        assert_eq!(cross_section(&reach(valley(8.0)), 80.0), (80.0, false));
        let mut last = 0.0;
        for d in 5..20 {
            let (h, channel) = cross_section(&reach(d as f32), 90.0);
            assert!(!channel && h >= last && h <= 90.0, "{d} {h}");
            last = h;
        }
        assert_eq!(valley_noise(&reach(4.0)), 0.05);
        assert_eq!(valley_noise(&reach(valley(8.0))), 1.0);
        // Hydraulic geometry: four times the area, twice the width, 1.74 times the depth's growth.
        assert_eq!(width(MIN_AREA), HEAD_WIDTH);
        assert!((width(4.0 * MIN_AREA) - 2.0 * HEAD_WIDTH).abs() < 1e-5);
        assert!((depth(4.0 * MIN_AREA) - 1.0 - 4f32.powf(0.4)).abs() < 1e-5);
        assert_eq!(width(1e12), MAX_WIDTH);
    }

    #[test]
    fn water_base_noise_gives_the_water_level() {
        use super::super::terraforged_generator_preset::terrain_levels;
        let levels = terrain_levels(None).unwrap();
        for water in [62, 63, 100, 250, 400] {
            let base = water_base_noise(levels, water);
            assert_eq!(levels.scaled_base_level(base).floor() as i32, water);
        }
    }

    /// With `coastal_landforms` and `glacial_landforms` the drainage map samples the ground the distant terrain has
    /// (the coast's landform, the troughs: `shaped_sample`), so a channel never takes its water level from ground
    /// a tidal flat or a trough lowered under it. Seed 777 on a 256-block grid: every land column of the map is the
    /// far tiles' height, and the coasts and troughs move some of them off TerraForged's.
    #[test]
    fn the_drainage_map_samples_the_coasts_and_troughs() {
        use super::super::terraforged_generator_preset::{build, terrain_levels};
        use super::super::terraforged_settings::{FAITHFUL, Settings, set_for_thread};
        let levels = terrain_levels(None).unwrap();
        let make = |settings: Settings| {
            set_for_thread(Some(settings));
            let generator = build(levels).unwrap();
            set_for_thread(None);
            generator.capture_seed(777);
            generator
        };
        let shaped = Settings {
            coastal_landforms: true,
            glacial_landforms: true,
            ..FAITHFUL
        };
        let (flowing, ground) = (
            make(Settings {
                flow_rivers: true,
                ..shaped
            }),
            make(shaped),
        );
        let seed = flowing.seed();
        let source = &flowing
            .noise_generator()
            .flow_rivers()
            .unwrap()
            .drainage
            .source;
        assert!(source.coasts == Some(false) && source.glaciers.is_some());
        let plain = ground.noise_generator();
        let (mut land, mut moved) = (0, 0);
        for i in 0..48 * 48 {
            let (x, z) = ((i % 48 - 24) * 256, (i / 48 - 24) * 256);
            let column = source.column(seed, x, z);
            if column.sea {
                continue;
            }
            land += 1;
            assert_eq!(
                levels.height(column.height),
                ground.far_heights(x, z).unwrap().0,
                "{x} {z}"
            );
            let mut sample = NoiseSample::new(&plain.continent().context().terrain);
            plain
                .sample(seed, x, z, &mut sample, &mut plain.blender_resource())
                .unwrap();
            moved += usize::from(column.height != levels.scaled_height(sample.height_noise));
        }
        eprintln!("{land} land columns, {moved} moved by the coasts or troughs");
        assert!(land > 100 && moved > 0, "{land} {moved}");
    }

    /// TerraForged seed 777 with the setting: channels near spawn, carved as river terrain under stepped water; every
    /// column away from them as without the setting; and chunks the same whichever order and thread make them.
    #[test]
    #[ignore = "real terrain: every query builds drainage tiles, too slow in debug; run with --profile fast --ignored"]
    fn terraforged_seed_777_carves_channels_and_nothing_else() {
        use super::super::terraforged_generator_preset::{build, terrain_levels};
        use super::super::terraforged_settings::{FAITHFUL, Settings, set_for_thread};
        let levels = terrain_levels(None).unwrap();
        let on = || {
            set_for_thread(Some(Settings {
                flow_rivers: true,
                ..FAITHFUL
            }));
            let generator = build(levels).unwrap();
            set_for_thread(None);
            generator.capture_seed(777);
            generator
        };
        let (flowing, faithful) = (on(), build(levels).unwrap());
        faithful.capture_seed(777);
        assert!(faithful.noise_generator().flow_rivers().is_none());
        let seed = flowing.seed();
        let channels = flowing
            .noise_generator()
            .flow_rivers()
            .unwrap()
            .channels(seed);
        // A coarse search of 2 km around spawn (debug builds sample slowly), then the columns around one channel.
        let start = std::time::Instant::now();
        let found = (0..64 * 64)
            .map(|i| ((i % 64 - 32) * 32, (i / 64 - 32) * 32))
            .find(|&(x, z)| {
                channels
                    .reach(x, z)
                    .is_some_and(|r| r.distance <= r.width * 0.5)
            });
        eprintln!("search {:?}: {found:?}", start.elapsed());
        let (cx, cz) = found.expect("a channel within 1 km of spawn");
        let (mut inside, mut valley) = (0, 0);
        for i in 0..48 * 48 {
            let (x, z) = (cx + i % 48 - 24, cz + i / 48 - 24);
            let a = flowing.noise_generator().height_noise(seed, x, z).unwrap();
            let b = faithful.noise_generator().height_noise(seed, x, z).unwrap();
            match channels.reach(x, z) {
                None => assert_eq!(a.to_bits(), b.to_bits(), "{x} {z}"),
                Some(reach) => {
                    assert!(
                        a <= b + LEVEE_DEPTH / levels.max_y as f32,
                        "{x} {z}: {a} {b}"
                    );
                    if reach.distance <= reach.width * 0.5 {
                        inside += 1;
                        assert!(reach.water >= levels.sea_level, "{reach:?}");
                    } else {
                        valley += 1;
                    }
                }
            }
        }
        assert!(inside > 10 && valley > inside, "{inside} {valley}");
        // Three chunks across the channel, in order on one generator and backwards from two threads on a fresh one.
        let chunks: Vec<(i32, i32)> = (-1..=1).map(|d| ((cx >> 4) + d, cz >> 4)).collect();
        let fold =
            |generator: &super::super::terraforged_generator_facade::TerraForgedGenerator,
             (x, z): (i32, i32)| {
                let mut bits = Vec::new();
                generator
                    .noise_generator()
                    .generate(seed, x, z, |data| {
                        for dz in 0..16 {
                            for dx in 0..16 {
                                bits.push(data.height().get(dx, dz).to_bits());
                                bits.push(data.base().get(dx, dz).to_bits());
                                bits.push(data.river().get(dx, dz).to_bits());
                                bits.push(u32::from(
                                    data.terrain().get(dx, dz).unwrap().is_river(),
                                ));
                            }
                        }
                    })
                    .unwrap();
                bits
            };
        let reference: Vec<_> = chunks.iter().map(|&c| fold(&flowing, c)).collect();
        let fresh = on();
        let mut again: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunks
                .iter()
                .rev()
                .map(|&c| {
                    let fresh = &fresh;
                    scope.spawn(move || fold(fresh, c))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        again.reverse();
        assert_eq!(again, reference);
        eprintln!(
            "seed 777: channel at {cx} {cz}, {inside} channel and {valley} valley columns ({:?})",
            start.elapsed()
        );
    }
}
