// Horizon's drainage map (in neither TerraForged tree), for the realistic-terrain settings (#277): where water
// flows over the un-eroded terrain, on two fixed grids. Only the settings built on it read it (flow-routed rivers,
// `terraforged_flow_rivers`; basin erosion, `terraforged_basin_erosion`), each behind its own setting. The rain shadow
// (`terraforged_rain_shadow`) doesn't: the climate must stay cheap.
//
// Clean-room, from published methods: depressions are filled and flats routed by priority-flood (Barnes, Lehman &
// Mulla 2014, "Priority-Flood: An optimal depression-filling and watershed-labeling algorithm"), directions are
// D8 steepest descent (O'Callaghan & Mark 1984) on the filled surface, and accumulation sums upstream area in the
// flood's order. The tiling is fixed-grid with halos (the idea of Barnes 2016's tiled priority-flood, without its
// global spill graph, which an endless world has no room for): every value is a pure function of the source and a
// tile key, so the cache below can drop, recompute or race on tiles without changing a bit.

use super::terraforged_generator_facade::TerraForgedGenerator;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The D8 neighbours, by direction index: east, then clockwise in Minecraft's axes (+z is south).
pub const D8: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
/// The direction of a sea cell: water ends there.
pub const SEA: u8 = 8;
/// The direction of a cell on a tile's outer ring: water leaves the tile there.
pub const EDGE: u8 = 9;

/// One column of the source: the terrain height (blocks, unfloored) and whether it lies under the sea.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub height: f32,
    pub sea: bool,
}

/// One cell of a [`Routing`] or [`FlowTile`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowCell {
    /// A [`D8`] index (the cell water flows to), [`SEA`] or [`EDGE`].
    pub direction: u8,
    /// The area draining through the cell, itself included, in square blocks (an inflow counts as its area).
    pub accumulation: f64,
    /// The source height.
    pub height: f32,
    /// The height with depressions filled up to their spill level: never lower than `height`, and never rising
    /// downstream.
    pub filled: f32,
    /// Blocks along the flow from the cell to the sea (to the tile's edge where the flow leaves it, plus the
    /// coarser level's distance from there).
    pub distance: f32,
}

impl FlowCell {
    /// The offset of the cell water flows to, `None` at the sea or a tile's edge.
    pub fn downstream(&self) -> Option<(i32, i32)> {
        D8.get(self.direction as usize).copied()
    }
}

/// A rectangular height grid to route: row-major, `width` cells along x.
pub struct Dem<'a> {
    pub width: usize,
    pub depth: usize,
    /// Blocks per cell side.
    pub cell: f32,
    pub columns: &'a [Column],
    /// Each cell's own area (square blocks), plus any water that enters there from outside the grid.
    pub runoff: &'a [f64],
    /// Whether water leaves through the outer ring's cell `index`: it is then an outlet ([`EDGE`]).
    pub edge_outlet: &'a dyn Fn(usize) -> bool,
    /// The distance to the sea from an [`EDGE`] cell, by index.
    pub edge_distance: &'a dyn Fn(usize) -> f32,
}

/// A routed [`Dem`].
pub struct Routing {
    pub width: usize,
    pub depth: usize,
    pub direction: Vec<u8>,
    pub accumulation: Vec<f64>,
    pub height: Vec<f32>,
    pub filled: Vec<f32>,
    pub distance: Vec<f32>,
}

impl Routing {
    pub fn cell(&self, index: usize) -> FlowCell {
        FlowCell {
            direction: self.direction[index],
            accumulation: self.accumulation[index],
            height: self.height[index],
            filled: self.filled[index],
            distance: self.distance[index],
        }
    }
}

/// A key that orders `f32`s totally (`f32::total_cmp`), so the flood's order is fixed even for NaN.
fn order_key(value: f32) -> u32 {
    let bits = value.to_bits();
    if bits >> 31 == 1 {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

/// Route `dem`: priority-flood from every outlet (sea cells, and the ring cells `edge_outlet` opens), D8 steepest
/// descent on the filled surface, the flood's own step across flats and filled depressions, then accumulation and
/// distance in the flood's order. A cell that reaches no outlet (closed edges and no sea) keeps [`EDGE`].
/// Deterministic: ties pop in the order they were pushed.
pub fn route(dem: &Dem<'_>) -> Routing {
    let (width, depth) = (dem.width, dem.depth);
    let n = width * depth;
    assert!(
        dem.columns.len() == n && dem.runoff.len() == n,
        "drainage: a {width}x{depth} grid"
    );
    let height: Vec<f32> = dem.columns.iter().map(|c| c.height).collect();
    let mut filled = height.clone();
    let mut direction = vec![EDGE; n];
    let mut receiver = vec![usize::MAX; n];
    let mut closed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut queue = BinaryHeap::new();
    let mut sequence = 0u32;
    for index in 0..n {
        let (x, z) = (index % width, index / width);
        let edge = x == 0 || z == 0 || x == width - 1 || z == depth - 1;
        if dem.columns[index].sea || (edge && (dem.edge_outlet)(index)) {
            direction[index] = if dem.columns[index].sea { SEA } else { EDGE };
            closed[index] = true;
            queue.push(Reverse((order_key(filled[index]), sequence, index)));
            sequence += 1;
        }
    }
    loop {
        let Some(Reverse((_, _, index))) = queue.pop() else {
            // Cells no outlet reaches: flood the lowest of them as a sink, so every cell is ordered.
            let Some(sink) = (0..n)
                .filter(|&i| !closed[i])
                .min_by(|&a, &b| height[a].total_cmp(&height[b]))
            else {
                break;
            };
            closed[sink] = true;
            queue.push(Reverse((order_key(filled[sink]), sequence, sink)));
            sequence += 1;
            continue;
        };
        order.push(index);
        let (x, z) = ((index % width) as i32, (index / width) as i32);
        for &(dx, dz) in &D8 {
            let (nx, nz) = (x + dx, z + dz);
            if nx < 0 || nz < 0 || nx >= width as i32 || nz >= depth as i32 {
                continue;
            }
            let next = nz as usize * width + nx as usize;
            if closed[next] {
                continue;
            }
            closed[next] = true;
            filled[next] = if height[next] < filled[index] {
                filled[index]
            } else {
                height[next]
            };
            receiver[next] = index;
            queue.push(Reverse((order_key(filled[next]), sequence, next)));
            sequence += 1;
        }
    }
    // Steepest strict descent where the filled surface falls; the flood's step elsewhere. Both point at a cell
    // the flood popped earlier, so the flood's order is a topological order of the flow.
    for &index in &order {
        if receiver[index] == usize::MAX {
            continue;
        }
        let (x, z) = ((index % width) as i32, (index / width) as i32);
        let mut best = None;
        let mut steepest = 0.0f32;
        for (d, &(dx, dz)) in D8.iter().enumerate() {
            let (nx, nz) = (x + dx, z + dz);
            if nx < 0 || nz < 0 || nx >= width as i32 || nz >= depth as i32 {
                continue;
            }
            let next = nz as usize * width + nx as usize;
            let drop = (filled[index] - filled[next])
                / if dx != 0 && dz != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
            if drop > steepest {
                steepest = drop;
                best = Some((d, next));
            }
        }
        let (d, next) = best.unwrap_or_else(|| {
            let next = receiver[index];
            let (dx, dz) = ((next % width) as i32 - x, (next / width) as i32 - z);
            (D8.iter().position(|&o| o == (dx, dz)).unwrap(), next)
        });
        direction[index] = d as u8;
        receiver[index] = next;
    }
    let mut accumulation = dem.runoff.to_vec();
    for &index in order.iter().rev() {
        if receiver[index] != usize::MAX {
            accumulation[receiver[index]] += accumulation[index];
        }
    }
    let mut distance = vec![0.0f32; n];
    for &index in &order {
        distance[index] = match direction[index] {
            SEA => 0.0,
            EDGE if receiver[index] == usize::MAX => (dem.edge_distance)(index),
            d => {
                let (dx, dz) = D8[d as usize];
                let step = if dx != 0 && dz != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                distance[receiver[index]] + step * dem.cell
            }
        };
    }
    Routing {
        width,
        depth,
        direction,
        accumulation,
        height,
        filled,
        distance,
    }
}

/// A fixed tiling of the world into square tiles of `core` cells of `cell` blocks, each routed over its core plus
/// `halo` cells on every side. A cell belongs to the one tile whose core holds it; the halo only lets water that
/// crosses the core's border route the same way in both tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileGrid {
    pub cell: i32,
    pub core: i32,
    pub halo: i32,
}

/// The continent level: 8192-block tiles of 128-block cells, with a 2048-block halo.
pub const L0: TileGrid = TileGrid {
    cell: 128,
    core: 64,
    halo: 16,
};
/// The basin level: 1024-block tiles of 16-block cells, with a 128-block halo (one L0 cell, so an L1 window is
/// a whole number of L0 cells).
pub const L1: TileGrid = TileGrid {
    cell: 16,
    core: 64,
    halo: 8,
};
const _: () = assert!(L1.halo * L1.cell == L0.cell && (L1.core * L1.cell) % L0.cell == 0);
// `blend_weights` serves both levels (basin erosion blends L0 too): the same core, and a halo at least `BLEND` wide.
const _: () = assert!(L0.core == L1.core && L0.halo >= L1.halo);
/// Samples per L0 cell side: an L0 cell is the mean of a 2x2 grid of source columns (one every 64 blocks), so a
/// landform narrower than a cell weighs less in it than a single sample would give; under the sea where most of
/// them are. (4x4 costs four times as much, 0.66 s per L0 tile over TerraForged in release, for no gain on the
/// border tests.)
pub const L0_SAMPLES: i32 = 2;
/// L1 cells per L0 cell side.
const RATIO: i32 = L0.cell / L1.cell;

impl TileGrid {
    /// Cells per window side (core and both halos).
    pub const fn side(self) -> i32 {
        self.core + 2 * self.halo
    }

    /// The cell holding block coordinate `block`.
    pub const fn cell_of(self, block: i32) -> i32 {
        block.div_euclid(self.cell)
    }

    /// The tile whose core holds cell `cell`.
    pub const fn tile_of(self, cell: i32) -> i32 {
        cell.div_euclid(self.core)
    }

    /// The first cell of tile `tile`'s window.
    pub const fn origin(self, tile: i32) -> i32 {
        tile * self.core - self.halo
    }

    /// The block a cell is sampled at: its centre.
    pub const fn center(self, cell: i32) -> i32 {
        cell * self.cell + self.cell / 2
    }
}

/// One routed tile of a [`TileGrid`].
pub struct FlowTile {
    pub grid: TileGrid,
    /// The window's first cell, in the grid's cells.
    pub origin: (i32, i32),
    pub routing: Routing,
}

impl FlowTile {
    /// Cell (`x`, `z`) of the grid, which must lie in the window.
    pub fn cell(&self, x: i32, z: i32) -> FlowCell {
        let side = self.grid.side();
        let (lx, lz) = (x - self.origin.0, z - self.origin.1);
        assert!(
            (0..side).contains(&lx) && (0..side).contains(&lz),
            "drainage: cell {x} {z} outside the tile"
        );
        self.routing.cell((lz * side + lx) as usize)
    }

    /// L0 tile (`tile_x`, `tile_z`): each cell the mean of [`L0_SAMPLES`]² source columns, open edges.
    pub fn l0(tile_x: i32, tile_z: i32, source: &impl Fn(i32, i32) -> Column) -> Self {
        let grid = L0;
        let (side, origin) = (grid.side(), (grid.origin(tile_x), grid.origin(tile_z)));
        let step = grid.cell / L0_SAMPLES;
        let columns: Vec<Column> = (0..side * side)
            .map(|i| {
                let (x, z) = (
                    (origin.0 + i % side) * grid.cell,
                    (origin.1 + i / side) * grid.cell,
                );
                let (mut height, mut sea) = (0.0f64, 0);
                for k in 0..L0_SAMPLES * L0_SAMPLES {
                    let column = source(
                        x + step / 2 + k % L0_SAMPLES * step,
                        z + step / 2 + k / L0_SAMPLES * step,
                    );
                    height += f64::from(column.height);
                    sea += i32::from(column.sea);
                }
                Column {
                    height: (height / f64::from(L0_SAMPLES * L0_SAMPLES)) as f32,
                    sea: 2 * sea > L0_SAMPLES * L0_SAMPLES,
                }
            })
            .collect();
        let area = f64::from(grid.cell * grid.cell);
        let runoff = vec![area; columns.len()];
        let dem = Dem {
            width: side as usize,
            depth: side as usize,
            cell: grid.cell as f32,
            columns: &columns,
            runoff: &runoff,
            edge_outlet: &|_| true,
            edge_distance: &|_| 0.0,
        };
        Self {
            grid,
            origin,
            routing: route(&dem),
        }
    }

    /// L1 tile (`tile_x`, `tile_z`) over the L0 cells `l0` gives. The window is a whole number of L0 cells; each
    /// L0 cell just outside it that flows into it brings its accumulation in at the lowest L1 cell on their shared
    /// side (just inside the window's outer ring, an outlet), and water leaving through the window's edge carries
    /// on with the L0 cell's distance to the sea.
    pub fn l1(
        tile_x: i32,
        tile_z: i32,
        source: &impl Fn(i32, i32) -> Column,
        l0: &impl Fn(i32, i32) -> FlowCell,
    ) -> Self {
        Self::basin(L1, tile_x, tile_z, source, l0)
    }

    /// [`Self::l1`] on any grid of L1 cells whose core and halo are whole L0 cells.
    fn basin(
        grid: TileGrid,
        tile_x: i32,
        tile_z: i32,
        source: &impl Fn(i32, i32) -> Column,
        l0: &impl Fn(i32, i32) -> FlowCell,
    ) -> Self {
        debug_assert!(grid.cell == L1.cell && grid.core % RATIO == 0 && grid.halo % RATIO == 0);
        let (side, origin) = (grid.side(), (grid.origin(tile_x), grid.origin(tile_z)));
        let columns: Vec<Column> = (0..side * side)
            .map(|i| {
                source(
                    grid.center(origin.0 + i % side),
                    grid.center(origin.1 + i / side),
                )
            })
            .collect();
        let mut runoff = vec![f64::from(grid.cell * grid.cell); columns.len()];
        let coarse = side / RATIO;
        let coarse_origin = (origin.0.div_euclid(RATIO), origin.1.div_euclid(RATIO));
        let inside = |x: i32, z: i32| (0..coarse).contains(&x) && (0..coarse).contains(&z);
        for cz in -1..=coarse {
            for cx in -1..=coarse {
                if inside(cx, cz) {
                    continue;
                }
                let cell = l0(coarse_origin.0 + cx, coarse_origin.1 + cz);
                let Some((dx, dz)) = cell.downstream() else {
                    continue;
                };
                let (rx, rz) = (cx + dx, cz + dz);
                if !inside(rx, rz) {
                    continue;
                }
                // The L1 cells of the receiving L0 cell that face the donor (one corner for a diagonal), moved
                // in off the window's outer ring, which is an outlet.
                let entry = (0..RATIO)
                    .map(|i| {
                        // `toward`: the donor's side of the receiver along one axis.
                        let side_of = |toward: i32| match toward.signum() {
                            1 => RATIO - 1,
                            -1 => 0,
                            _ => i,
                        };
                        let inner = |f: i32| f.clamp(1, side - 2);
                        let fx = inner(rx * RATIO + side_of(-dx));
                        let fz = inner(rz * RATIO + side_of(-dz));
                        (fz * side + fx) as usize
                    })
                    .min_by(|&a, &b| {
                        columns[a]
                            .height
                            .total_cmp(&columns[b].height)
                            .then(a.cmp(&b))
                    })
                    .unwrap();
                runoff[entry] += cell.accumulation;
            }
        }
        // A ring cell lets water out only where its L0 cell flows out of the window (or ends there), so the
        // halo drains the way L0 does instead of leaking wherever the fine terrain dips outward.
        let edge_outlet = |index: usize| {
            let (x, z) = (index as i32 % side / RATIO, index as i32 / side / RATIO);
            l0(coarse_origin.0 + x, coarse_origin.1 + z)
                .downstream()
                .is_none_or(|(dx, dz)| !inside(x + dx, z + dz))
        };
        let edge_distance = |index: usize| {
            let (x, z) = (index as i32 % side, index as i32 / side);
            l0(coarse_origin.0 + x / RATIO, coarse_origin.1 + z / RATIO).distance
        };
        let dem = Dem {
            width: side as usize,
            depth: side as usize,
            cell: grid.cell as f32,
            columns: &columns,
            runoff: &runoff,
            edge_outlet: &edge_outlet,
            edge_distance: &edge_distance,
        };
        Self {
            grid,
            origin,
            routing: route(&dem),
        }
    }
}

/// The drainage map of one source: L0 and L1 tiles computed on demand and memoised in [`TileCache`]s. A tile
/// computed twice is bit-identical, so eviction only costs time.
pub struct Drainage<S> {
    seed: i32,
    source: S,
    l0: TileCache<FlowTile>,
    l1: TileCache<FlowTile>,
}

/// A cache of tiles that take a long time to build (an L1 tile about 150 ms, an L0 tile more), shared by every
/// generation thread: least recently used out past `capacity`, each tile built once by its first caller while
/// callers of the same tile wait for it and callers of other tiles go on. No lock is held during a build, so a build
/// may look up other tiles (L1 reads L0) and slow tiles never block hits.
///
/// TerraForged's lossy cache (`terraforged_lossy_cache`) is no fit here: its bucket is the key's low bits, which for a
/// tile key are the tile's z alone, so every tile of a row shares one bucket; each tile has a single slot in it, so
/// two tiles of a working set rebuild each other at every alternation; and it computes under the bucket's lock. In
/// play (lodDistance 2048, `basin_erosion`) its L0 tiles were built about 300 times each in under three minutes.
pub struct TileCache<T> {
    capacity: usize,
    slots: Mutex<TileSlots<T>>,
}

/// One tile per key, with the lookup that last used it.
struct TileSlots<T> {
    clock: u64,
    tiles: HashMap<(i32, i32), (u64, Arc<OnceLock<Arc<T>>>)>,
}

impl<T> TileCache<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            slots: Mutex::new(TileSlots {
                clock: 0,
                tiles: HashMap::with_capacity(capacity + 1),
            }),
        }
    }

    /// Tile (`x`, `z`), built by `build` unless it is cached or being built. A `build` that panics leaves the tile
    /// unbuilt: the next caller (or a waiting one) builds it again.
    pub fn get_or_build(&self, x: i32, z: i32, build: impl FnOnce() -> T) -> Arc<T> {
        let slot = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            slots.clock += 1;
            let clock = slots.clock;
            if let Some((used, slot)) = slots.tiles.get_mut(&(x, z)) {
                *used = clock;
                Arc::clone(slot)
            } else {
                if slots.tiles.len() >= self.capacity
                    && let Some(oldest) = slots
                        .tiles
                        .iter()
                        .min_by_key(|(_, (used, _))| *used)
                        .map(|(&key, _)| key)
                {
                    slots.tiles.remove(&oldest);
                }
                let slot = Arc::new(OnceLock::new());
                slots.tiles.insert((x, z), (clock, Arc::clone(&slot)));
                slot
            }
        };
        Arc::clone(slot.get_or_init(|| Arc::new(build())))
    }

    /// The tiles cached or being built.
    pub fn len(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tiles
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The tiles one caller has already looked up, by tile: a run of queries then takes the shared cache's lock once per
/// tile, and its tiles stay alive while it runs even if the cache drops them. Values are the same either way.
#[derive(Default)]
pub struct TileMemo {
    tiles: HashMap<(i32, i32), Arc<FlowTile>>,
}

impl<S: Fn(i32, i32) -> Column> Drainage<S> {
    /// L0 tiles cached (about 190 KB each).
    pub const L0_TILES: usize = 16;
    /// L1 tiles cached (about 130 KB each): more than the 6x6 tiles distant terrain at 2048 blocks reads, so a
    /// player's working set stays cached as it moves.
    pub const L1_TILES: usize = 96;

    /// The map of `source`, which must be a pure function of the position (`seed`'s world's).
    pub fn new(seed: i32, source: S) -> Self {
        Self {
            seed,
            source,
            l0: TileCache::new(Self::L0_TILES),
            l1: TileCache::new(Self::L1_TILES),
        }
    }

    /// The runtime seed the map is of.
    pub fn seed(&self) -> i32 {
        self.seed
    }

    pub fn l0_tile(&self, x: i32, z: i32) -> Arc<FlowTile> {
        self.l0
            .get_or_build(x, z, || FlowTile::l0(x, z, &self.source))
    }

    /// L0 cell (`x`, `z`), from the tile whose core holds it.
    pub fn l0_cell(&self, x: i32, z: i32) -> FlowCell {
        self.l0_tile(L0.tile_of(x), L0.tile_of(z)).cell(x, z)
    }

    pub fn l1_tile(&self, x: i32, z: i32) -> Arc<FlowTile> {
        self.l1.get_or_build(x, z, || {
            // An L1 window's ring often spans two L0 tiles: look each up once.
            let memo = RefCell::new(HashMap::new());
            FlowTile::l1(x, z, &self.source, &|cx, cz| {
                let key = (L0.tile_of(cx), L0.tile_of(cz));
                let tile = Arc::clone(
                    memo.borrow_mut()
                        .entry(key)
                        .or_insert_with(|| self.l0_tile(key.0, key.1)),
                );
                tile.cell(cx, cz)
            })
        })
    }

    /// [`Self::l1_tile`] through `memo`.
    pub fn l1_tile_in(&self, memo: &mut TileMemo, x: i32, z: i32) -> Arc<FlowTile> {
        Arc::clone(
            memo.tiles
                .entry((x, z))
                .or_insert_with(|| self.l1_tile(x, z)),
        )
    }

    /// L1 cell (`x`, `z`), from the tile whose core holds it.
    pub fn l1_cell(&self, x: i32, z: i32) -> FlowCell {
        self.l1_tile(L1.tile_of(x), L1.tile_of(z)).cell(x, z)
    }

    /// The flow at block (`x`, `z`): its L1 cell, with the accumulation and distance blended across tile borders.
    ///
    /// Two L1 tiles can see a cell near their border differently: each takes the far side's water from L0, and L0
    /// may join a tributary to the river a cell earlier or later than L1 does. So within [`BLEND`] cells of a
    /// border the owner's accumulation and distance are mixed with the neighbour's view of the same cell (both
    /// windows hold it), half and half at the border: a river's width and phase change smoothly where its owner
    /// does. Direction, height and filled height are the owner's. Away from borders it is [`Self::l1_cell`].
    pub fn flow(&self, x: i32, z: i32) -> FlowCell {
        self.flow_in(&mut TileMemo::default(), x, z)
    }

    /// [`Self::flow`] with the L1 tiles looked up through `memo`, for a caller that asks about many cells.
    pub fn flow_in(&self, memo: &mut TileMemo, x: i32, z: i32) -> FlowCell {
        let (cx, cz) = (L1.cell_of(x), L1.cell_of(z));
        let (tx, tz) = (L1.tile_of(cx), L1.tile_of(cz));
        let mut cell = self.l1_tile_in(memo, tx, tz).cell(cx, cz);
        let (mut accumulation, mut distance) = (0.0f64, 0.0f64);
        for (ox, wx) in blend_weights(cx - tx * L1.core) {
            for (oz, wz) in blend_weights(cz - tz * L1.core) {
                let weight = wx * wz;
                if weight > 0.0 {
                    let view = if (ox, oz) == (0, 0) {
                        cell
                    } else {
                        self.l1_tile_in(memo, tx + ox, tz + oz).cell(cx, cz)
                    };
                    accumulation += weight * view.accumulation;
                    distance += weight * f64::from(view.distance);
                }
            }
        }
        cell.accumulation = accumulation;
        cell.distance = distance as f32;
        cell
    }
}

/// Cells on each side of an L1 border over which [`Drainage::flow`] blends the two tiles' views (one halo).
pub const BLEND: i32 = L1.halo;

/// The owner's and one neighbour's weights along one axis for core position `local` (0..core): the neighbour's
/// falls linearly from 1/2 at the border to 0 [`BLEND`] cells in.
pub fn blend_weights(local: i32) -> [(i32, f64); 2] {
    let from_border = |cells: i32| 0.5 - (f64::from(cells) + 0.5) / f64::from(2 * BLEND);
    if local < BLEND {
        let w = from_border(local);
        [(0, 1.0 - w), (-1, w)]
    } else if local >= L1.core - BLEND {
        let w = from_border(L1.core - 1 - local);
        [(0, 1.0 - w), (1, w)]
    } else {
        [(0, 1.0), (0, 0.0)]
    }
}

/// The source over a TerraForged generator: [`TerraForgedGenerator::drainage_height`] (un-eroded, without
/// TerraForged's rivers), under the sea below its sea level. A sample that fails reads as sea.
pub fn terraforged_source(generator: &TerraForgedGenerator) -> impl Fn(i32, i32) -> Column + '_ {
    let sea = generator.sea_level() as f32;
    move |x, z| match generator.drainage_height(x, z) {
        Ok(height) => Column {
            height,
            sea: height < sea,
        },
        Err(_) => Column {
            height: sea,
            sea: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routed(
        width: usize,
        depth: usize,
        open: bool,
        height: impl Fn(i32, i32) -> Column,
    ) -> Routing {
        let columns: Vec<Column> = (0..width * depth)
            .map(|i| height((i % width) as i32, (i / width) as i32))
            .collect();
        let runoff = vec![1.0; width * depth];
        route(&Dem {
            width,
            depth,
            cell: 1.0,
            columns: &columns,
            runoff: &runoff,
            edge_outlet: &|_| open,
            edge_distance: &|_| 0.0,
        })
    }

    fn land(height: f32) -> Column {
        Column { height, sea: false }
    }

    /// Every water path ends at an outlet and never climbs the filled surface; outlets hold all the runoff.
    fn check_paths(routing: &Routing, runoff: f64) {
        let mut out = 0.0;
        for index in 0..routing.width * routing.depth {
            let cell = routing.cell(index);
            assert!(cell.filled >= cell.height);
            let Some((dx, dz)) = cell.downstream() else {
                out += cell.accumulation;
                continue;
            };
            let next = (index as i32 + dx + dz * routing.width as i32) as usize;
            assert!(routing.filled[next] <= cell.filled, "{index} climbs");
            assert!(routing.accumulation[next] > cell.accumulation);
            assert!(routing.distance[next] < cell.distance);
        }
        assert_eq!(out, runoff);
    }

    #[test]
    fn a_cone_drains_outward_from_its_peak() {
        let routing = routed(21, 21, true, |x, z| {
            land(100.0 - (((x - 10).pow(2) + (z - 10).pow(2)) as f32).sqrt())
        });
        check_paths(&routing, 441.0);
        let peak = routing.cell(10 * 21 + 10);
        assert_eq!(peak.accumulation, 1.0);
        for index in 0..441 {
            let (x, z) = (index as i32 % 21 - 10, index as i32 / 21 - 10);
            if let Some((dx, dz)) = routing.cell(index).downstream() {
                assert!(
                    (x, z) == (0, 0) || dx * x + dz * z > 0,
                    "{x} {z} flows inward"
                );
            }
        }
        // Straight down the east slope: the ray gathers at least its own nine cells.
        assert!(routing.cell(10 * 21 + 19).accumulation >= 9.0);
        assert_eq!(routing.cell(10 * 21 + 11).direction, 0);
    }

    #[test]
    fn a_pit_in_a_tilted_plane_fills_to_its_spill_and_drains_on() {
        // A plane falling west (x = 0 is the only outlet, the rest of the ring is closed) with a pit at (8, 5).
        let routing = routed(16, 11, false, |x, z| {
            if x == 0 {
                return Column {
                    height: -1.0,
                    sea: true,
                };
            }
            let pit = if (x - 8).abs() <= 1 && (z - 5).abs() <= 1 {
                10.0
            } else {
                0.0
            };
            land(x as f32 - pit)
        });
        check_paths(&routing, 176.0);
        let pit = routing.cell(5 * 16 + 8);
        assert_eq!(pit.height, -2.0);
        // The pit (lowered by 10) fills to its lowest rim, the cells at x = 6.
        assert_eq!(pit.filled, 6.0);
        assert!(routing.cell(5 * 16 + 7).filled == 6.0);
        // Each column of 11 cells is one row of the sea's catchment: x = 1 gathers everything east of it.
        let column_one: f64 = (0..11).map(|z| routing.cell(z * 16 + 1).accumulation).sum();
        assert_eq!(column_one, 165.0);
        assert_eq!(
            (0..11)
                .map(|z| routing.cell(z * 16).accumulation)
                .sum::<f64>(),
            176.0
        );
    }

    #[test]
    fn two_basins_drain_to_their_own_sea() {
        // A ridge at x = 7 splits a closed 15x9 grid into two cones, each around its own sea cell.
        let routing = routed(15, 9, false, |x, z| {
            let sea_x = if x < 7 { 2 } else { 12 };
            let radius = (((x - sea_x).pow(2) + (z - 4).pow(2)) as f32).sqrt();
            if x == 7 {
                land(20.0)
            } else if (x, z) == (sea_x, 4) {
                Column {
                    height: 0.0,
                    sea: true,
                }
            } else {
                land(radius)
            }
        });
        check_paths(&routing, 135.0);
        let west = routing.cell(4 * 15 + 2).accumulation;
        let east = routing.cell(4 * 15 + 12).accumulation;
        // 63 cells each side; the ridge's 9 go one way or the other.
        assert_eq!(west + east, 135.0);
        assert!((63.0..=72.0).contains(&west), "{west}");
        for index in 0..135 {
            let (x, z) = (index as i32 % 15, index as i32 / 15);
            if x != 7 {
                let sea_x = if x < 7 { 2 } else { 12 };
                assert!(
                    routing.cell(index).distance
                        <= ((x - sea_x).abs() + (z - 4).abs()) as f32 + 0.001
                );
            }
        }
    }

    #[test]
    fn a_closed_flat_basin_still_orders_every_cell() {
        let routing = routed(6, 6, false, |_, _| land(5.0));
        assert_eq!(
            routing.accumulation.iter().filter(|&&a| a == 36.0).count(),
            1
        );
        assert_eq!(routing.direction.iter().filter(|&&d| d == EDGE).count(), 1);
        check_paths(&routing, 36.0);
    }

    /// A pure, seam-free test landscape: a sea to the west, land rising east with value-noise hills and a valley
    /// along z = 520 that drains west.
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

    #[test]
    fn tiles_are_bit_identical_in_any_order_on_any_threads() {
        let tiles: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|z| (-1..=1).map(move |x| (x, z)))
            .collect();
        let snapshot =
            |drainage: &Drainage<fn(i32, i32) -> Column>, tiles: &[(i32, i32)]| -> Vec<u64> {
                let mut bits = Vec::new();
                for &(tx, tz) in tiles {
                    let tile = drainage.l1_tile(tx, tz);
                    for i in 0..L1.side() * L1.side() {
                        let cell = tile.routing.cell(i as usize);
                        bits.extend([
                            u64::from(cell.direction),
                            cell.accumulation.to_bits(),
                            u64::from(cell.filled.to_bits()),
                            u64::from(cell.distance.to_bits()),
                        ]);
                    }
                }
                bits
            };
        let reference = snapshot(&Drainage::new(1, landscape), &tiles);
        // A fresh map, its tiles first asked in a shuffled order from six threads at once.
        let shuffled = Drainage::new(1, landscape as fn(i32, i32) -> Column);
        std::thread::scope(|scope| {
            for thread in 0..6 {
                let shuffled = &shuffled;
                let tiles = &tiles;
                scope.spawn(move || {
                    for k in 0..tiles.len() {
                        let (x, z) = tiles[(k * 5 + thread * 7) % tiles.len()];
                        shuffled.flow(x * 1024 + 300, z * 1024 + 700);
                    }
                });
            }
        });
        assert_eq!(snapshot(&shuffled, &tiles), reference);
        // And without the caches: every tile and every L0 cell computed from scratch.
        let direct = FlowTile::l1(1, -1, &landscape, &|x, z| {
            FlowTile::l0(L0.tile_of(x), L0.tile_of(z), &landscape).cell(x, z)
        });
        let cached = shuffled.l1_tile(1, -1);
        assert_eq!(direct.routing.accumulation, cached.routing.accumulation);
        assert_eq!(direct.routing.direction, cached.routing.direction);
    }

    #[test]
    fn a_tile_is_built_once_however_many_threads_ask() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = TileCache::new(4);
        let builds = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let tile = cache.get_or_build(3, -2, || {
                        builds.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        7
                    });
                    assert_eq!(*tile, 7);
                });
            }
        });
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        // A slow build never blocks a hit on another tile.
        cache.get_or_build(0, 0, || 1);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                cache.get_or_build(9, 9, || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    2
                })
            });
            std::thread::sleep(std::time::Duration::from_millis(20));
            let started = std::time::Instant::now();
            assert_eq!(*cache.get_or_build(0, 0, || unreachable!()), 1);
            assert!(started.elapsed() < std::time::Duration::from_millis(200));
        });
    }

    #[test]
    fn the_least_recently_used_tile_goes_first() {
        let cache = TileCache::new(3);
        for x in 0..3 {
            cache.get_or_build(x, 0, || x);
        }
        cache.get_or_build(0, 0, || unreachable!());
        cache.get_or_build(3, 0, || 3);
        assert_eq!(cache.len(), 3);
        // (1, 0) was the least recently used: it is built again, and (0, 0) is still cached.
        assert_eq!(*cache.get_or_build(1, 0, || 10), 10);
        assert_eq!(*cache.get_or_build(3, 0, || unreachable!()), 3);
        // A build that panics leaves the tile to the next caller.
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.get_or_build(5, 5, || panic!("no tile"))
        }));
        assert!(failed.is_err());
        assert_eq!(*cache.get_or_build(5, 5, || 55), 55);
    }

    #[test]
    fn accumulation_is_continuous_across_l1_borders() {
        let drainage = Drainage::new(1, landscape as fn(i32, i32) -> Column);
        let block = |c: i32| L1.center(c);
        // The valley's channel: follow the flow west from x = 4000 across the borders at 3072 and 2048.
        let (mut x, mut z) = (L1.cell_of(4000), L1.cell_of(520));
        let mut crossings = 0;
        let mut last = drainage.l1_cell(x, z);
        while let Some((dx, dz)) = last.downstream() {
            let (nx, nz) = (x + dx, z + dz);
            let next = drainage.l1_cell(nx, nz);
            let crossing = L1.tile_of(nx) != L1.tile_of(x) || L1.tile_of(nz) != L1.tile_of(z);
            // Within a tile the filled surface never rises downstream; two tiles may fill a depression by their
            // border to different spill levels.
            assert!(crossing || next.filled <= last.filled);
            let (here, there) = (
                drainage.flow(block(x), block(z)),
                drainage.flow(block(nx), block(nz)),
            );
            if crossing {
                crossings += 1;
                // The upstream tile's view of the cell across the border against its owner's: they may differ
                // by a tributary that L0 and L1 join at different cells...
                let near = drainage.l1_tile(L1.tile_of(x), L1.tile_of(z)).cell(nx, nz);
                let mismatch = (near.accumulation - next.accumulation).abs();
                let gap = (near.distance - next.distance).abs();
                // ...but the blended flow steps across the border by about a 16th of it, plus a cell's own share.
                let step = (there.accumulation - here.accumulation).abs();
                let stride = (there.distance - here.distance).abs();
                eprintln!(
                    "crossing at {nx} {nz}: views {} / {} (distance {} / {}), blended step {step} ({stride}), \
                     filled {} -> {}",
                    near.accumulation,
                    next.accumulation,
                    near.distance,
                    next.distance,
                    last.filled,
                    next.filled
                );
                assert!(
                    step <= mismatch / 8.0 + f64::from(L0.cell * L0.cell),
                    "{step} {mismatch}"
                );
                assert!(stride <= gap / 8.0 + 2.0 * L1.cell as f32, "{stride} {gap}");
            } else if (8..56).contains(&(nx - L1.tile_of(nx) * L1.core))
                && (8..56).contains(&(nz - L1.tile_of(nz) * L1.core))
            {
                assert_eq!(there, next);
            }
            (x, z, last) = (nx, nz, next);
        }
        assert_eq!(last.direction, SEA);
        assert!(crossings >= 2, "{crossings}");
        // The channel collects the valley's sides: more than a 1 km strip upstream of the sea.
        assert!(last.accumulation > 1.0e6, "{last:?}");
    }

    #[test]
    fn l0_tiles_agree_on_the_cells_they_share() {
        let drainage = Drainage::new(1, landscape as fn(i32, i32) -> Column);
        let (a, b) = (drainage.l0_tile(0, 0), drainage.l0_tile(1, 0));
        let mut same = 0;
        for z in 0..L0.core {
            for x in L0.core - 4..L0.core + 4 {
                same += usize::from(a.cell(x, z).direction == b.cell(x, z).direction);
            }
        }
        assert!(same >= (8 * L0.core) as usize * 9 / 10, "{same}");
    }

    #[test]
    fn terraforged_seed_777_drains_to_the_sea() {
        use super::super::terraforged_generator_preset::{build, terrain_levels};
        let generator = build(terrain_levels(None).unwrap()).unwrap();
        generator.capture_seed(777);
        let source = terraforged_source(&generator);
        // The source is the un-eroded height without rivers: floored, it is far_heights' terrain height wherever
        // TerraForged carves no river.
        let (mut same, mut land, mut sea) = (0, 0, 0);
        let mut shore = None;
        for i in 0..400 {
            let (x, z) = ((i % 20 - 10) * 211 + 3, (i / 20 - 10) * 197 - 5);
            let column = source(x, z);
            same +=
                usize::from(column.height.floor() as i32 == generator.far_heights(x, z).unwrap().0);
            land += usize::from(!column.sea);
            // A failed sample reads as sea at exactly the sea level: this counts only real sea.
            sea += usize::from(column.sea && column.height < generator.sea_level() as f32);
            if column.sea && shore.is_none() {
                shore = Some((x, z));
            }
        }
        assert!(
            same > 200 && land > 40 && sea > 0 && land + sea == 400,
            "{same} {land} {sea}"
        );
        // A 40x40 grid of 64-block cells around the first sea sample, so it holds land and sea (debug builds
        // sample slowly; full tiles: the benchmark).
        let (sx, sz) = shore.unwrap();
        let columns: Vec<Column> = (0..1600)
            .map(|i| source(sx + (i % 40 - 20) * 64, sz + (i / 40 - 20) * 64))
            .collect();
        let runoff = vec![4096.0; 1600];
        let routing = route(&Dem {
            width: 40,
            depth: 40,
            cell: 64.0,
            columns: &columns,
            runoff: &runoff,
            edge_outlet: &|_| true,
            edge_distance: &|_| 0.0,
        });
        check_paths(&routing, 1600.0 * 4096.0);
        let sea_cells = routing.direction.iter().filter(|&&d| d == SEA).count();
        let biggest = routing.accumulation.iter().copied().fold(0.0, f64::max);
        // Land cells whose water reaches the sea inside the grid, rather than leaving through its edge.
        let to_sea = (0..1600)
            .filter(|&start| {
                let mut index = start;
                while let Some((dx, dz)) = routing.cell(index).downstream() {
                    index = (index as i32 + dx + dz * 40) as usize;
                }
                routing.direction[start] != SEA && routing.direction[index] == SEA
            })
            .count();
        eprintln!(
            "seed 777: {sea_cells} sea cells of 1600, {to_sea} land cells drain to them, biggest catchment \
             {biggest} square blocks"
        );
        assert!(sea_cells > 0 && to_sea > 100, "{sea_cells} {to_sea}");
        assert!(biggest > 50.0 * 4096.0);
    }

    #[test]
    #[ignore = "benchmark; run by hand with --release --ignored --nocapture"]
    fn drainage_benchmark() {
        use super::super::terraforged_generator_preset::{build, terrain_levels};
        let generator = build(terrain_levels(None).unwrap()).unwrap();
        generator.capture_seed(98765);
        let source = terraforged_source(&generator);
        let start = std::time::Instant::now();
        let l0 = FlowTile::l0(0, 0, &source);
        let l0_time = start.elapsed();
        let start = std::time::Instant::now();
        std::hint::black_box(FlowTile::l0(0, 0, &landscape));
        let route_time = start.elapsed();
        let start = std::time::Instant::now();
        for tile in 0..8 {
            std::hint::black_box(FlowTile::l1(tile, 3, &source, &|x, z| l0.cell(x, z)));
        }
        let l1_time = start.elapsed() / 8;
        eprintln!("L0 tile {l0_time:?} (routing alone {route_time:?}), L1 tile {l1_time:?}");
    }
}
