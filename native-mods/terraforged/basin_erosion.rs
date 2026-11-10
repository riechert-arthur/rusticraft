// Horizon's basin-scale erosion (in neither TerraForged tree), the `basin_erosion` setting (#279): valleys cut along
// the drainage map (`terraforged_drainage`) before TerraForged's droplet erosion adds its detail on top.
//
// Clean-room, from published methods. Channels incise by the detachment-limited stream-power law
// dz/dt = U - K·A^m·S^n (Howard 1994; Whipple & Tucker 1999) with no uplift, n = 1 and m = 0.5, solved implicitly
// along the drainage tree, receivers first, as Braun & Willett (2013, "A very efficient O(n), implicit and parallel
// method to solve the stream power equation governing fluvial incision and landscape evolution") do; the routing is
// the drainage map's and stays fixed. With n = 1 a knickpoint retreats upstream at K·A^m, so trunk valleys reach far
// into the mountains while small tributaries barely start. Hillslopes follow the threshold-hillslope model (valley
// sides stand at a critical slope; Burbank et al. 1996, Roering et al. 1999): the incision spreads out of a channel
// at that slope, over the terrain's own. Large rivers keep a floor whose half-width grows as A^0.5 (valley width
// scaling with drainage area, as Montgomery & Gran 2001 and Tomkin et al. 2003 measured), and the depth of a cut is
// softly limited.
//
// Hierarchy: L0 tiles (128-block cells) are eroded with their open edges fixed; an L1 tile (16-block cells) is eroded
// with the cells water leaves its window by fixed at the source lowered by L0's erosion there (the boundary condition
// the issue asks for), so a river crossing L1 tiles is cut to the same coarse profile on both sides. A cell's value
// is the tile owning it, blended with the neighbouring tile's view near a border as `Drainage::flow` blends, and a
// block's is the bilinear mix of its four nearest cell centres: a pure function of the seed and the position, with
// no seam at chunks, tiles or L0 borders.

use super::terraforged_drainage::{
    Column, D8, Drainage, EDGE, L0, L1, Routing, SEA, TileGrid, blend_weights,
};
use super::terraforged_flow_rivers::{RiverDrainage, Source};
use super::terraforged_lossy_cache::ConcurrentLossyCache;
use super::terraforged_terrain_data::NoiseSample;
use super::terraforged_terrain_levels::TerrainLevels;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, RwLock};

/// The erosion's parameters (see the module's notes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// K·t, the erodibility times the erosion time: blocks of knickpoint retreat per square root of the drained area
    /// (square blocks). 3 sends a knickpoint about 9 km up a 10 km² valley and 670 blocks up from a channel's head.
    pub erosion: f64,
    /// The area (square blocks) that must drain through a cell for it to incise: a channel's head. Above it the slope
    /// is a hillslope, lowered only by the walls.
    pub channel_area: f64,
    /// The implicit steps the time is split into.
    pub steps: u32,
    /// The critical hillslope (height per block) the incision spreads out of a channel at.
    pub wall: f32,
    /// A valley floor's half-width per square root of the drained area (blocks per block), and at most.
    pub floor_width: f64,
    pub floor_max: f32,
    /// How steeply a floor rises away from its river (height per block).
    pub floor_slope: f32,
    /// The soft limit of a cut's depth: a cut of `d` blocks becomes `d / (1 + d / max_depth)`.
    pub max_depth: f32,
}

/// The setting's parameters.
pub const PARAMS: Params = Params {
    erosion: 3.0,
    channel_area: 50_000.0,
    steps: 8,
    wall: 0.4,
    floor_width: 0.015,
    floor_max: 72.0,
    floor_slope: 0.05,
    max_depth: 72.0,
};

/// A routed grid's root (a cell whose water leaves the grid: the sea, a river, a window's edge): the height
/// incision cuts towards there, and the root's own change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Root {
    pub base: f32,
    pub delta: f32,
}

/// A total order on `f32` for the walls' queue (`f32::total_cmp`).
fn order_key(value: f32) -> u32 {
    let bits = value.to_bits();
    if bits >> 31 == 1 {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

/// Erodes `routing` (cells `cell` blocks wide): stream-power incision along its flow, valley floors (with `floors`),
/// the soft depth limit, then the threshold walls. Returns each cell's change in blocks, never above 0 but at a root,
/// whose change `root` gives (with the height it cuts towards). Deterministic: a fixed order throughout.
pub fn erode(
    routing: &Routing,
    cell: f32,
    params: Params,
    floors: bool,
    root: &dyn Fn(usize) -> Root,
) -> Vec<f32> {
    let (width, n) = (routing.width, routing.width * routing.depth);
    const NONE: u32 = u32::MAX;
    let step_length = |dx: i32, dz: i32| {
        if dx != 0 && dz != 0 {
            cell * std::f32::consts::SQRT_2
        } else {
            cell
        }
    };
    let mut receiver = vec![NONE; n];
    let mut length = vec![0.0f32; n];
    let mut roots = vec![None; n];
    for index in 0..n {
        match routing.direction[index] {
            d if (d as usize) < D8.len() => {
                let (dx, dz) = D8[d as usize];
                receiver[index] =
                    (index as i64 + i64::from(dx) + i64::from(dz) * width as i64) as u32;
                length[index] = step_length(dx, dz);
            }
            _ => roots[index] = Some(root(index)),
        }
    }
    // Receivers before donors: breadth first from the roots, in index order.
    let mut donors = vec![0u32; n + 1];
    for &r in &receiver {
        if r != NONE {
            donors[r as usize + 1] += 1;
        }
    }
    for i in 0..n {
        donors[i + 1] += donors[i];
    }
    let mut fill = donors.clone();
    let mut donor = vec![0u32; donors[n] as usize];
    for (index, &r) in receiver.iter().enumerate() {
        if r != NONE {
            donor[fill[r as usize] as usize] = index as u32;
            fill[r as usize] += 1;
        }
    }
    let mut order: Vec<u32> = (0..n as u32)
        .filter(|&i| roots[i as usize].is_some())
        .collect();
    let mut head = 0;
    while head < order.len() {
        let index = order[head] as usize;
        head += 1;
        order.extend_from_slice(&donor[donors[index] as usize..donors[index + 1] as usize]);
    }
    debug_assert_eq!(order.len(), n, "basin erosion: the flow has a cycle");

    let mut height: Vec<f64> = (0..n)
        .map(|i| match roots[i] {
            Some(root) => f64::from(root.base),
            // The filled surface: a depression erodes from its spill level (lowering it drains the lake), never
            // from its bottom, which would dig a pit beside it.
            None => f64::from(routing.filled[i]),
        })
        .collect();
    // Implicit stream power, n = 1: h_i ← (h_i + F·h_r) / (1 + F), F = K·Δt·A^m / L, receivers already updated, in
    // channels only (a hillslope cell keeps F = 0). The change is the eroded surface against the terrain's own height,
    // so a filled depression changes only where the cut goes below its bottom. A cell below its receiver (a pit) stays: with no uplift the ground
    // only ever goes down.
    let factor: Vec<f64> = (0..n)
        .map(|i| {
            if receiver[i] == NONE || routing.accumulation[i] < params.channel_area {
                0.0
            } else {
                params.erosion / f64::from(params.steps) * routing.accumulation[i].sqrt()
                    / f64::from(length[i])
            }
        })
        .collect();
    for _ in 0..params.steps {
        for &index in &order {
            let index = index as usize;
            let r = receiver[index];
            if r == NONE {
                continue;
            }
            let f = factor[index];
            let cut = (height[index] + f * height[r as usize]) / (1.0 + f);
            if cut < height[index] {
                height[index] = cut;
            }
        }
    }
    if floors {
        // A river keeps a floor of half-width ∝ A^0.5 around it: nothing within it stands higher than the river plus a
        // gentle slope. Read from the incised surface before any floor, in index order.
        let incised = height.clone();
        let depth = routing.depth;
        for index in 0..n {
            if receiver[index] == NONE {
                continue;
            }
            let half = (params.floor_width * routing.accumulation[index].sqrt())
                .min(f64::from(params.floor_max)) as f32;
            if half < cell {
                continue;
            }
            let reach = (half / cell).ceil() as i32;
            let (x, z) = ((index % width) as i32, (index / width) as i32);
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx < 0 || nz < 0 || nx >= width as i32 || nz >= depth as i32 {
                        continue;
                    }
                    let next = nz as usize * width + nx as usize;
                    let distance = cell * ((dx * dx + dz * dz) as f32).sqrt();
                    if roots[next].is_some() || distance > half {
                        continue;
                    }
                    let floor = incised[index] + f64::from(params.floor_slope * distance);
                    if floor < height[next] {
                        height[next] = floor;
                    }
                }
            }
        }
    }
    let mut delta: Vec<f32> = (0..n)
        .map(|i| match roots[i] {
            Some(root) => root.delta,
            None => {
                let cut = (height[i] - f64::from(routing.height[i])).min(0.0) as f32;
                cut / (1.0 - cut / params.max_depth)
            }
        })
        .collect();
    // Threshold walls: a cell lies at most `wall` per block above the cut of any cell beside it (a distance
    // transform, most negative first).
    let mut queue: BinaryHeap<Reverse<(u32, u32)>> = (0..n)
        .filter(|&i| delta[i] < 0.0)
        .map(|i| Reverse((order_key(delta[i]), i as u32)))
        .collect();
    while let Some(Reverse((key, index))) = queue.pop() {
        let index = index as usize;
        if key != order_key(delta[index]) {
            continue;
        }
        let (x, z) = ((index % width) as i32, (index / width) as i32);
        for &(dx, dz) in &D8 {
            let (nx, nz) = (x + dx, z + dz);
            if nx < 0 || nz < 0 || nx >= width as i32 || nz >= routing.depth as i32 {
                continue;
            }
            let next = nz as usize * width + nx as usize;
            if roots[next].is_some() {
                continue;
            }
            let wall = delta[index] + params.wall * step_length(dx, dz);
            if wall < delta[next] {
                delta[next] = wall;
                queue.push(Reverse((order_key(wall), next as u32)));
            }
        }
    }
    delta
}

/// One eroded tile: each window cell's change, in blocks.
pub struct ErosionTile {
    pub grid: TileGrid,
    pub origin: (i32, i32),
    pub delta: Vec<f32>,
}

impl ErosionTile {
    /// The change of cell (`x`, `z`), which must lie in the window.
    pub fn cell(&self, x: i32, z: i32) -> f32 {
        let side = self.grid.side();
        let (lx, lz) = (x - self.origin.0, z - self.origin.1);
        assert!(
            (0..side).contains(&lx) && (0..side).contains(&lz),
            "basin erosion: cell {x} {z} outside the tile"
        );
        self.delta[(lz * side + lx) as usize]
    }
}

/// The tiles and cells one caller has looked up (see `terraforged_drainage::TileMemo`).
#[derive(Default)]
pub struct BasinMemo {
    /// The [`BasinMap`] the tiles are of (its `id`; 0 for none yet).
    owner: u64,
    l0: HashMap<(i32, i32), Arc<ErosionTile>>,
    l1: HashMap<(i32, i32), Arc<ErosionTile>>,
    cells: HashMap<(i32, i32), f32>,
}

impl BasinMemo {
    /// Tiles and cells a memo keeps between queries: more and it starts over.
    const L1_TILES: usize = 16;
    const CELLS: usize = 1 << 14;

    /// Makes the memo `owner`'s, dropping another map's tiles, and starts over when it has grown past its bounds.
    fn bind(&mut self, owner: u64) {
        if self.owner != owner
            || self.l1.len() > Self::L1_TILES
            || self.l0.len() > Self::L1_TILES
            || self.cells.len() > Self::CELLS
        {
            *self = Self {
                owner,
                ..Self::default()
            };
        }
    }
}

/// Runs `f` with this thread's memo, for callers that sample one column at a time (`NoiseGenerator::sample` in
/// `terraforged_noise_generator`): a fresh memo per column would ask the lossy caches for every tile, and two tiles
/// sharing a slot would then rebuild each other at every column. Values are the same with any memo.
pub fn with_thread_memo<R>(f: impl FnOnce(&mut BasinMemo) -> R) -> R {
    thread_local! {
        static MEMO: std::cell::RefCell<BasinMemo> = std::cell::RefCell::default();
    }
    MEMO.with(|memo| match memo.try_borrow_mut() {
        Ok(mut memo) => f(&mut memo),
        Err(_) => f(&mut BasinMemo::default()),
    })
}

/// A unique id per [`BasinMap`], so a memo never mixes two maps' tiles.
static MAP_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn tile_key(x: i32, z: i32) -> i64 {
    (i64::from(x) << 32) | i64::from(z as u32)
}

/// The value of cell (`cx`, `cz`) of `grid`: its owner tile's, blended with the neighbouring tile's view within
/// [`super::terraforged_drainage::BLEND`] cells of a border, half and half at the border (as `Drainage::flow`).
fn blended(grid: TileGrid, cx: i32, cz: i32, mut view: impl FnMut(i32, i32) -> f32) -> f32 {
    let (tx, tz) = (grid.tile_of(cx), grid.tile_of(cz));
    let mut value = 0.0f64;
    for (ox, wx) in blend_weights(cx - tx * grid.core) {
        for (oz, wz) in blend_weights(cz - tz * grid.core) {
            let weight = wx * wz;
            if weight > 0.0 {
                value += weight * f64::from(view(tx + ox, tz + oz));
            }
        }
    }
    value as f32
}

/// The bilinear mix at block (`x`, `z`) of the four cell values of `grid` around it (cell values at cell centres).
fn bilinear(grid: TileGrid, x: i32, z: i32, mut cell: impl FnMut(i32, i32) -> f32) -> f32 {
    let half = grid.cell / 2;
    let (fx, fz) = (x - half, z - half);
    let (cx, cz) = (fx.div_euclid(grid.cell), fz.div_euclid(grid.cell));
    let (tx, tz) = (
        fx.rem_euclid(grid.cell) as f32 / grid.cell as f32,
        fz.rem_euclid(grid.cell) as f32 / grid.cell as f32,
    );
    let (a, b) = (cell(cx, cz), cell(cx + 1, cz));
    let (c, d) = (cell(cx, cz + 1), cell(cx + 1, cz + 1));
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * tz
}

/// The erosion over one drainage map: L0 and L1 tiles eroded on demand and memoised in lossy caches. A tile computed
/// twice is bit-identical, so eviction and races between threads only cost time.
pub struct BasinMap<S> {
    id: u64,
    seed: i32,
    sea_level: f32,
    params: Params,
    drainage: Arc<Drainage<S>>,
    l0: ConcurrentLossyCache<ErosionTile>,
    l1: ConcurrentLossyCache<ErosionTile>,
}

impl<S: Fn(i32, i32) -> Column> BasinMap<S> {
    /// L0 tiles cached (about 37 KB each).
    pub const L0_TILES: i32 = 16;
    /// L1 tiles cached (about 26 KB each).
    pub const L1_TILES: i32 = 64;

    /// The erosion over `drainage`; the sea cuts no deeper than `sea_level`.
    pub fn new(seed: i32, sea_level: f32, params: Params, drainage: Arc<Drainage<S>>) -> Self {
        Self {
            id: MAP_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            seed,
            sea_level,
            params,
            drainage,
            l0: ConcurrentLossyCache::new(Self::L0_TILES, 8),
            l1: ConcurrentLossyCache::new(Self::L1_TILES, 8),
        }
    }

    pub fn drainage(&self) -> &Arc<Drainage<S>> {
        &self.drainage
    }

    /// A sea (or river) root cuts towards the water, never under the sea level; its own ground stays.
    fn sea_root(&self, height: f32) -> Root {
        Root {
            base: height.max(self.sea_level),
            delta: 0.0,
        }
    }

    /// L0 tile (`x`, `z`), its window's edge fixed.
    pub fn l0_tile(&self, x: i32, z: i32) -> Arc<ErosionTile> {
        self.l0
            .compute_if_absent(self.seed, tile_key(x, z), |_, _| {
                let flow = self.drainage.l0_tile(x, z);
                let routing = &flow.routing;
                let delta = erode(routing, L0.cell as f32, self.params, false, &|i| {
                    if routing.direction[i] == SEA {
                        self.sea_root(routing.height[i])
                    } else {
                        Root {
                            base: routing.height[i],
                            delta: 0.0,
                        }
                    }
                });
                ErosionTile {
                    grid: L0,
                    origin: flow.origin,
                    delta,
                }
            })
    }

    fn l0_tile_in(&self, memo: &mut BasinMemo, x: i32, z: i32) -> Arc<ErosionTile> {
        Arc::clone(memo.l0.entry((x, z)).or_insert_with(|| self.l0_tile(x, z)))
    }

    /// L0's change at block (`x`, `z`): its cells blended across tile borders, then bilinear.
    pub fn l0_delta(&self, memo: &mut BasinMemo, x: i32, z: i32) -> f32 {
        memo.bind(self.id);
        bilinear(L0, x, z, |cx, cz| {
            blended(L0, cx, cz, |tx, tz| {
                self.l0_tile_in(memo, tx, tz).cell(cx, cz)
            })
        })
    }

    /// L1 tile (`x`, `z`): the cells water leaves its window by are fixed at L0's change there.
    pub fn l1_tile(&self, x: i32, z: i32) -> Arc<ErosionTile> {
        self.l1
            .compute_if_absent(self.seed, tile_key(x, z), |_, _| {
                let flow = self.drainage.l1_tile(x, z);
                let routing = &flow.routing;
                let side = L1.side();
                let memo = std::cell::RefCell::new(BasinMemo::default());
                let delta = erode(routing, L1.cell as f32, self.params, true, &|i| {
                    let height = routing.height[i];
                    if routing.direction[i] == SEA {
                        return self.sea_root(height);
                    }
                    debug_assert_eq!(routing.direction[i], EDGE);
                    let (lx, lz) = (i as i32 % side, i as i32 / side);
                    let ring = lx == 0 || lz == 0 || lx == side - 1 || lz == side - 1;
                    let delta = if ring {
                        self.l0_delta(
                            &mut memo.borrow_mut(),
                            L1.center(flow.origin.0 + lx),
                            L1.center(flow.origin.1 + lz),
                        )
                    } else {
                        0.0
                    };
                    Root {
                        base: height + delta,
                        delta,
                    }
                });
                ErosionTile {
                    grid: L1,
                    origin: flow.origin,
                    delta,
                }
            })
    }

    fn l1_tile_in(&self, memo: &mut BasinMemo, x: i32, z: i32) -> Arc<ErosionTile> {
        Arc::clone(memo.l1.entry((x, z)).or_insert_with(|| self.l1_tile(x, z)))
    }

    /// The change of L1 cell (`cx`, `cz`): its owner tile's, blended with the neighbour's view near a border.
    pub fn cell(&self, memo: &mut BasinMemo, cx: i32, cz: i32) -> f32 {
        memo.bind(self.id);
        if let Some(&value) = memo.cells.get(&(cx, cz)) {
            return value;
        }
        let value = blended(L1, cx, cz, |tx, tz| {
            self.l1_tile_in(memo, tx, tz).cell(cx, cz)
        });
        memo.cells.insert((cx, cz), value);
        value
    }

    /// The change at block (`x`, `z`), in blocks (never above 0): the bilinear mix of the four L1 cells around it.
    pub fn delta(&self, memo: &mut BasinMemo, x: i32, z: i32) -> f32 {
        bilinear(L1, x, z, |cx, cz| self.cell(memo, cx, cz))
    }
}

/// A column of `height` blocks lowered by `delta`, never below a block over `floor` (the land's base level, where
/// TerraForged's rivers stand, or the sea) and never raised.
pub fn lowered(height: f32, delta: f32, floor: f32) -> f32 {
    let floor = floor + 1.0;
    if delta < 0.0 && height > floor {
        (height + delta).max(floor)
    } else {
        height
    }
}

/// The `basin_erosion` setting's map, over the world's shared drainage map ([`RiverDrainage`]).
pub struct BasinErosion {
    drainage: Arc<RiverDrainage>,
    map: RwLock<Option<(i32, Arc<BasinMap<Source>>)>>,
}

impl BasinErosion {
    pub fn new(drainage: Arc<RiverDrainage>) -> Self {
        Self {
            drainage,
            map: RwLock::new(None),
        }
    }

    /// The map for runtime `seed` (a world has one; another seed replaces it).
    pub fn map(&self, seed: i32) -> Arc<BasinMap<Source>> {
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
        let map = Arc::new(BasinMap::new(
            seed,
            self.drainage.sea_level() as f32,
            PARAMS,
            self.drainage.map(seed),
        ));
        *slot = Some((seed, Arc::clone(&map)));
        map
    }

    /// Lowers block (`x`, `z`)'s raw terrain `sample` (its height noise as `levels` scale it, before TerraForged's
    /// rivers) by the basin erosion there ([`lowered`]).
    pub fn lower(
        &self,
        memo: &mut BasinMemo,
        seed: i32,
        x: i32,
        z: i32,
        levels: TerrainLevels,
        sample: &mut NoiseSample,
    ) {
        let height = levels.scaled_height(sample.height_noise);
        let floor = levels
            .scaled_base_level(sample.base_noise)
            .max(levels.sea_level as f32);
        if height > floor + 1.0 {
            let delta = self.map(seed).delta(memo, x, z);
            let lowered = lowered(height, delta, floor);
            if lowered != height {
                sample.height_noise = lowered / levels.max_y as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A routed plane tilted towards a sea at x = 0, with low hash noise so it routes into channels.
    fn tilted(width: usize, depth: usize) -> Routing {
        fn hash(x: i32, z: i32) -> f32 {
            let h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (z as u32).wrapping_mul(0x85eb_ca77);
            let h = (h ^ (h >> 15)).wrapping_mul(0x2c1b_3c6d);
            (h >> 8) as f32 / (1 << 24) as f32
        }
        let columns: Vec<Column> = (0..width * depth)
            .map(|i| {
                let (x, z) = ((i % width) as i32, (i / width) as i32);
                let height = 2.0 * x as f32 + 3.0 * hash(x, z);
                Column {
                    height,
                    sea: x == 0,
                }
            })
            .collect();
        let runoff = vec![256.0; width * depth];
        super::super::terraforged_drainage::route(&super::super::terraforged_drainage::Dem {
            width,
            depth,
            cell: 16.0,
            columns: &columns,
            runoff: &runoff,
            edge_outlet: &|_| false,
            edge_distance: &|_| 0.0,
        })
    }

    /// [`PARAMS`] for the small synthetic grids: channels start at ten cells.
    const SMALL: Params = Params {
        channel_area: 2560.0,
        ..PARAMS
    };

    fn sea_roots(routing: &Routing) -> impl Fn(usize) -> Root + '_ {
        |i| Root {
            base: routing.height[i],
            delta: 0.0,
        }
    }

    #[test]
    fn a_tilted_plane_develops_channels_and_never_rises() {
        let routing = tilted(48, 48);
        let delta = erode(&routing, 16.0, SMALL, true, &sea_roots(&routing));
        assert!(delta.iter().all(|&d| d <= 0.0), "uplift with U = 0");
        // Channels (the cells draining most) are cut deeper than the hillslopes beside them, and the cut grows with
        // the area drained.
        let (mut channel, mut slope) = ((0.0, 0), (0.0, 0));
        for i in 0..48 * 48 {
            let x = i % 48;
            if !(8..40).contains(&x) {
                continue;
            }
            if routing.accumulation[i] > 40.0 * 256.0 {
                channel = (channel.0 + delta[i], channel.1 + 1);
            } else if routing.accumulation[i] <= 256.0 {
                slope = (slope.0 + delta[i], slope.1 + 1);
            }
        }
        let (channel, slope) = (channel.0 / channel.1 as f32, slope.0 / slope.1 as f32);
        eprintln!("mean cut: channels {channel}, hillslopes {slope}");
        assert!(channel < slope - 4.0 && channel < -4.0, "{channel} {slope}");
        // Downstream along the biggest channel the ground never rises after erosion.
        let mut index = (0..48 * 48)
            .filter(|&i| i % 48 == 40)
            .max_by(|&a, &b| routing.accumulation[a].total_cmp(&routing.accumulation[b]))
            .unwrap();
        let eroded = |i: usize| routing.height[i] + delta[i];
        while let Some((dx, dz)) = routing.cell(index).downstream() {
            let next = (index as i32 + dx + dz * 48) as usize;
            if routing.direction[next] == SEA {
                break;
            }
            // (Out of a pit, the terrain itself rises: erosion leaves a depression's bottom alone.)
            let pit = routing.filled[index] > routing.height[index];
            assert!(
                pit || eroded(next) <= eroded(index) + 1e-3,
                "{index} -> {next}: {} {}",
                eroded(index),
                eroded(next)
            );
            index = next;
        }
    }

    #[test]
    fn no_erosion_time_changes_nothing_and_more_cuts_deeper() {
        let routing = tilted(32, 32);
        let none = Params {
            erosion: 0.0,
            ..SMALL
        };
        assert!(
            erode(&routing, 16.0, none, true, &sea_roots(&routing))
                .iter()
                .all(|&d| d == 0.0)
        );
        let raw = Params {
            wall: f32::INFINITY,
            max_depth: f32::INFINITY,
            ..SMALL
        };
        let short = erode(&routing, 16.0, raw, false, &sea_roots(&routing));
        let long = erode(
            &routing,
            16.0,
            Params {
                erosion: 4.0 * SMALL.erosion,
                ..raw
            },
            false,
            &sea_roots(&routing),
        );
        let total = |d: &[f32]| d.iter().map(|&v| f64::from(v)).sum::<f64>();
        assert!(total(&long) < total(&short) && total(&short) < 0.0);
        assert!(short.iter().zip(&long).all(|(s, l)| l <= s));
    }

    #[test]
    fn the_implicit_solver_matches_the_analytic_profile() {
        // One straight channel (every cell incising), its area growing by a cell's 256 square blocks from the head, the
        // sea at x = 0. One implicit step, receivers first, is the closed form for each cell given its receiver.
        let routing = tilted(20, 1);
        let delta = erode(
            &routing,
            16.0,
            Params {
                steps: 1,
                channel_area: 0.0,
                wall: f32::INFINITY,
                max_depth: f32::INFINITY,
                ..SMALL
            },
            false,
            &sea_roots(&routing),
        );
        // From the filled surface, never rising; the change is against the terrain's own height.
        let mut below = f64::from(routing.height[0]);
        for x in 1..20 {
            let f = SMALL.erosion * routing.accumulation[x].sqrt() / 16.0;
            let filled = f64::from(routing.filled[x]);
            let eroded = ((filled + f * below) / (1.0 + f)).min(filled);
            let expected = eroded.min(f64::from(routing.height[x]));
            let got = f64::from(routing.height[x] + delta[x]);
            assert!((got - expected).abs() < 1e-3, "{x}: {got} {expected}");
            below = eroded;
        }
    }

    #[test]
    fn walls_stand_at_the_critical_slope_and_the_cut_is_limited() {
        let routing = tilted(40, 40);
        let delta = erode(&routing, 16.0, SMALL, true, &sea_roots(&routing));
        for i in 0..40 * 40 {
            let (x, z) = ((i % 40) as i32, (i / 40) as i32);
            assert!(-delta[i] < SMALL.max_depth);
            if routing.direction[i] == SEA {
                continue;
            }
            for &(dx, dz) in &D8 {
                let (nx, nz) = (x + dx, z + dz);
                if !(0..40).contains(&nx) || !(0..40).contains(&nz) {
                    continue;
                }
                let next = (nz * 40 + nx) as usize;
                let run = 16.0 * ((dx * dx + dz * dz) as f32).sqrt();
                assert!(
                    delta[i] <= delta[next] + SMALL.wall * run + 1e-3,
                    "{x} {z}: {} beside {}",
                    delta[i],
                    delta[next]
                );
            }
        }
    }

    #[test]
    fn lowering_keeps_above_the_base_and_never_raises() {
        assert_eq!(lowered(100.0, -10.0, 63.0), 90.0);
        assert_eq!(lowered(70.0, -10.0, 63.0), 64.0);
        assert_eq!(lowered(63.5, -10.0, 63.0), 63.5);
        assert_eq!(lowered(100.0, 0.0, 63.0), 100.0);
        assert_eq!(lowered(100.0, f32::NAN, 63.0), 100.0);
    }

    /// The drainage tests' landscape: a sea to the west, land rising east with hills and a valley along z = 520.
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
        let height =
            0.02 * x as f32 + 0.05 * (z - 520).abs() as f32 + 30.0 * value(700) + 6.0 * value(90)
                - 40.0;
        Column {
            height,
            sea: height < 0.0,
        }
    }

    type Landscape = fn(i32, i32) -> Column;

    fn map() -> BasinMap<Landscape> {
        BasinMap::new(
            1,
            0.0,
            PARAMS,
            Arc::new(Drainage::new(1, landscape as Landscape)),
        )
    }

    #[test]
    fn the_valley_is_cut_and_continuous_across_l1_borders() {
        let map = map();
        let mut memo = BasinMemo::default();
        // Along the valley (z = 520) from x = 3600 to 1500: borders at 3072 and 2048.
        let mut deepest = 0.0f32;
        let mut previous = map.delta(&mut memo, 3600, 520);
        for x in (1500..3600).rev() {
            let here = map.delta(&mut memo, x, 520);
            assert!(here <= 0.0);
            // Bilinear between blended cells: one block never steps by more than the walls allow over a block.
            assert!(
                (here - previous).abs() <= PARAMS.wall * 1.5,
                "{x}: {previous} -> {here}"
            );
            deepest = deepest.min(here);
            previous = here;
        }
        // Across the borders (the cells either side) and everywhere along them, the cells' values differ by a bounded
        // jump, not the step two unblended tiles would give.
        let mut worst = 0.0f32;
        for border in [L1.cell_of(2048), L1.cell_of(3072)] {
            for cz in L1.cell_of(0)..L1.cell_of(1024) {
                let (a, b) = (
                    map.cell(&mut memo, border - 1, cz),
                    map.cell(&mut memo, border, cz),
                );
                worst = worst.max((a - b).abs());
            }
        }
        eprintln!("valley cut to {deepest}, worst cell step across an L1 border {worst}");
        assert!(deepest < -4.0, "{deepest}");
        assert!(worst <= PARAMS.wall * L1.cell as f32 * std::f32::consts::SQRT_2 + 1e-3);
    }

    #[test]
    fn tiles_are_bit_identical_in_any_order_on_any_threads() {
        let blocks: Vec<(i32, i32)> = (0..64)
            .map(|i| ((i % 8) * 389 - 1200, (i / 8) * 311 - 900))
            .collect();
        let reference: Vec<u32> = {
            let map = map();
            let mut memo = BasinMemo::default();
            blocks
                .iter()
                .map(|&(x, z)| map.delta(&mut memo, x, z).to_bits())
                .collect()
        };
        let shuffled = map();
        let results: Vec<Vec<(usize, u32)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..6)
                .map(|thread| {
                    let (shuffled, blocks) = (&shuffled, &blocks);
                    scope.spawn(move || {
                        let mut memo = BasinMemo::default();
                        (0..blocks.len())
                            .map(|k| {
                                let i = (k * 5 + thread * 7) % blocks.len();
                                (
                                    i,
                                    shuffled
                                        .delta(&mut memo, blocks[i].0, blocks[i].1)
                                        .to_bits(),
                                )
                            })
                            .collect()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for result in results {
            for (i, bits) in result {
                assert_eq!(bits, reference[i], "block {:?}", blocks[i]);
            }
        }
    }

    /// The setting's generators over TerraForged seed 777: the un-eroded noise with and without the erosion, and the
    /// eroded (droplet) generator over the first.
    fn seed_777() -> (
        super::super::terraforged_noise_generator::NoiseGenerator,
        super::super::terraforged_noise_generator::NoiseGenerator,
        Arc<BasinErosion>,
        i32,
    ) {
        use super::super::terraforged_generator_preset::{
            build, build_noise_generator, terrain_levels,
        };
        let levels = terrain_levels(None).unwrap();
        let seed = {
            let generator = build(levels).unwrap();
            generator.capture_seed(777);
            generator.seed()
        };
        let basins = Arc::new(BasinErosion::new(Arc::new(RiverDrainage::new(
            build_noise_generator(levels).unwrap(),
        ))));
        let eroding = build_noise_generator(levels)
            .unwrap()
            .with_basin_erosion(Arc::clone(&basins));
        (
            eroding,
            build_noise_generator(levels).unwrap(),
            basins,
            seed,
        )
    }

    /// Seed 777 with the setting: valleys are cut (never a column raised, never below the base level), the cut is
    /// continuous block to block, and eroded chunks are the same whichever order and thread make them.
    #[test]
    #[ignore = "real terrain: every query builds drainage tiles, too slow in debug; run with --profile fast --ignored"]
    fn terraforged_seed_777_cuts_valleys_and_stays_pure() {
        use super::super::terraforged_eroded_noise_generator::ErodedNoiseGenerator;
        use super::super::terraforged_erosion::HeightmapSmoothingSettings;
        use super::super::terraforged_generator_preset::erosion_settings;
        let (eroding, faithful, basins, seed) = seed_777();
        let levels = faithful.levels();
        let start = std::time::Instant::now();
        let (mut lowered, mut deepest, mut total, mut worst_step) = (0, 0.0f32, 0.0f64, 0.0f32);
        let mut at = (0, 0);
        let side = 192;
        for i in 0..side * side {
            let (x, z) = (600 + (i % side) as i32 * 4, -900 + (i / side) as i32 * 4);
            let a = levels.scaled_height(eroding.height_noise(seed, x, z).unwrap());
            let b = levels.scaled_height(faithful.height_noise(seed, x, z).unwrap());
            assert!(a <= b, "{x} {z}: raised {b} -> {a}");
            if a < b {
                lowered += 1;
                if a - b < deepest {
                    deepest = a - b;
                    at = (x, z);
                }
                total += f64::from(a - b);
            }
        }
        let mut memo = BasinMemo::default();
        let map = basins.map(seed);
        for z in (-900..-900 + 4 * side as i32).step_by(2) {
            let mut last = map.delta(&mut memo, 600, z);
            for x in 601..600 + 4 * side as i32 {
                let here = map.delta(&mut memo, x, z);
                worst_step = worst_step.max((here - last).abs());
                last = here;
            }
        }
        eprintln!(
            "seed 777: {lowered} of {} columns lowered, deepest {deepest} at {at:?}, mean {} blocks, worst step {worst_step} \
             per block ({:?})",
            side * side,
            total / (side * side) as f64,
            start.elapsed()
        );
        assert!(
            lowered > side * side / 10 && deepest < -10.0,
            "{lowered} {deepest}"
        );
        assert!(worst_step <= PARAMS.wall * 1.5, "{worst_step}");
        // Three eroded chunks, in order on one generator and backwards from three threads on a fresh one.
        let fold = |generator: &ErodedNoiseGenerator, (x, z): (i32, i32)| {
            let mut bits = Vec::new();
            generator
                .generate(seed, x, z, |data| {
                    for dz in 0..16 {
                        for dx in 0..16 {
                            bits.push(data.height().get(dx, dz).to_bits());
                        }
                    }
                })
                .unwrap();
            bits
        };
        let eroded = |noise| {
            ErodedNoiseGenerator::new(
                noise,
                erosion_settings(),
                HeightmapSmoothingSettings::default(),
            )
        };
        let chunks: Vec<(i32, i32)> = (-1..=1).map(|d| (60 + d * 9, -40)).collect();
        let first = eroded(eroding);
        let reference: Vec<_> = chunks.iter().map(|&c| fold(&first, c)).collect();
        let fresh = eroded(seed_777().0);
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
    }

    #[test]
    #[ignore = "benchmark; run by hand with --profile fast --ignored --nocapture"]
    fn basin_erosion_benchmark() {
        let (eroding, faithful, basins, seed) = seed_777();
        let map = basins.map(seed);
        // A cold row of L1 tiles (drainage and L0 erosion built on the way), then the next row (L0 warm), then the
        // solver alone on one tile's routing.
        let time = |z: i32| {
            let start = std::time::Instant::now();
            for tile in 0..8 {
                std::hint::black_box(map.l1_tile(tile, z));
            }
            start.elapsed() / 8
        };
        let (cold, warm) = (time(3), time(4));
        let flow = map.drainage().l1_tile(2, 4);
        let start = std::time::Instant::now();
        std::hint::black_box(erode(&flow.routing, 16.0, PARAMS, true, &|i| Root {
            base: flow.routing.height[i],
            delta: 0.0,
        }));
        let solver = start.elapsed();
        let column = |noise: &super::super::terraforged_noise_generator::NoiseGenerator| {
            let start = std::time::Instant::now();
            for i in 0..4096 {
                std::hint::black_box(
                    noise
                        .height_noise(seed, i % 64 * 16 + 512, i / 64 * 16 + 3072)
                        .unwrap(),
                );
            }
            start.elapsed() / 4096
        };
        column(&eroding);
        let (with, without) = (column(&eroding), column(&faithful));
        eprintln!(
            "L1 tile: {cold:?} cold (drainage and L0 included), {warm:?} with L0 built (drainage included), solver \
             {solver:?}; a column's height {with:?} with the setting (tiles built), {without:?} without"
        );
    }
}
