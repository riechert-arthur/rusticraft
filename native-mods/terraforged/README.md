# TerraForged for Rusticraft

An exact Rust translation of [TerraForged](https://github.com/TerraForged/TerraForged) at commit
`6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` (its 1.19 port, over the seed-first Noise2D and Engine
0.3.0), delivered as a stacked series of small native-mod changes.

## Status

Still for development only (see [Status: development only](#status-development-only)). Create World
opens on **World Type: TerraForged** (its World tab; one press goes on to Normal). The tab then shows,
under the seed box, the map `/tf preview` gives of the new world: the 4096 blocks around spawn for the
typed seed, redrawn as the seed changes (a blank seed is random: the map shows once one is typed). The world
saves TerraForged's overworld stem and a `datapacks/terraforged` pack with TerraForged's
1024-block overworld (y −64 to 959) and TerraForged's biomes, and generates its overworld with
TerraForged (vanilla world types keep 26.3's overworld):

- terrain: continents, rivers, lakes, the 14 terrain graphs blended by `TerrainBlender`, and
  hydraulic erosion with 6dd607eb's `withErosion` settings and no heightmap smoothing
  (`NoiseGenerator`, `ErodedNoiseGenerator`), also in Horizon's distant tiles (un-eroded, or eroded
  near the player with the `eroded_far_terrain` setting); the `height_scale` setting stretches it
  vertically, `coastal_landforms` cuts cliffs and lays tidal flats along the shore, `glacial_landforms`
  widens cold valleys into U-shaped troughs;
- biomes: `ClimateNoise`, `BiomeMapManager`'s climate pools and `BiomeSampler`'s ocean, beach,
  river and lake overrides, also for `/locate biome`, plus TerraForged's own `terraforged:cave`
  and `terraforged:oak_forest` (`ModBiomes`), derived from the player's own 26.3 dripstone caves
  and plains;
- surfaces: the vanilla surface rules over TerraForged's preliminary heights, then `Surface`; the
  `erosion_deposits` setting lays erosion's sediment over them, `realistic_vegetation` podzol, moss and
  coarse dirt by slope and water; `rock_strata` bands the stone under them;
- caves: TerraForged's five noise caves and their biomes (global caves write `terraforged:cave`);
  no vanilla carvers run;
- structures: vanilla placement over TerraForged's (eroded) base heights and biomes, with terrain
  adapted under beardified structures' pieces (`StructureTerrain`);
- decoration: `FeatureDecorator` with TerraForged's vegetation (jittered, fitness-gated trees, as
  dense as TerraForged's worlds are in play; `sparse_forests` thins them, `realistic_vegetation` weighs
  them by aspect and water, `altitude_climate` by height), cave decoration, water
  smoothing and snow (with `altitude_climate`, down to a snow line).

In TerraForged worlds, a player's boat rises to a river's stepped water surface (`BoatHook`), and
operators get `/locateterrain <terrain> [radius]`, `/tf export structures`, `/tf regen <radius>` and
`/tf preview [size] [zoom]` (see the differences below).

`/locate biome` in a TerraForged world also finds the biomes only caves hold (`terraforged:cave`, and
with `corrected_biomes` the cave biomes unique caves draw), at a block of the cave, where TerraForged's
search asks only the noise biome and never finds them. `terraforged:oak_forest` is registered but no
climate places it, so it is found only where a unique cave draws it (never with `corrected_biomes`).

The Nether and the End stay vanilla, as in TerraForged. Deliberate fidelity to the pinned source,
including its quirks: JSON `Bias` nodes decode as 0 (Engine's spec reads a module as a float), cave
and other non-surface overworld biomes stay in the climate pools (so `deep_dark` can reach the
surface), unique caves draw from every registered biome (TerraForged's two included; Horizon's
`corrected_biomes` setting changes both), `oak_forest`
is in no climate pool (it lacks `minecraft:is_overworld`), so it never generates on the surface,
and outside the vegetation step each chunk is decorated with the one biome at its lowest corner. Vegetation counts
only the features that placed (`offset++`) to seed the next, so a tree blocked by a neighbour's
leaves reseeds the rest of its chunk: the trees depend on the order chunks are decorated in, which
varies with threads in Java and here alike (`/tf regen` included); Horizon's `deterministic` setting
([Settings](#settings)) takes most of that away.

Differences and gaps:

- Some parts follow TerraForged Reimagined instead (credited in `THIRD_PARTY_NOTICES.md`): the
  eroded-region builder, structures' eroded base heights, the erodible-block tags
  (`#minecraft:substrate_overworld` and `#minecraft:snow`), the pale garden tree keyword, the
  taller overworld's dimension type and, with the `corrected_biomes` setting, the biome classification. `base_column` reports vanilla's `NoiseColumn` shape (solid from
  the level bottom up to the eroded height), departing from both TerraForged trees.
- Apart from the decoration order above, results never depend on thread scheduling: per-thread
  carry-over and pooled-object reuse in Java's caches are not reproduced. The one carry-over that
  shapes the world is replaced by the value it settles at: 6dd607eb reuses each worker's chunk
  samples without resetting their river noise, which the river carver only multiplies (and sets back
  to 1 seaward of the beach), so a few chunks into play nearly every land sample starts at 0, or
  within a few hundredths of it (on seed 777, 98.5% of the land samples of 625 chunks around spawn
  on six workers, every one from a worker's third chunk on). In Java a worker's first chunks, and
  its first after crossing the sea, start higher, and a worker starting far from rivers stays sparse
  for a few dozen chunks; which chunks those are depends on scheduling. Every chunk sample here
  starts at 0. That gives TerraForged's dense forests and, as in Java's play, two more effects:
  every river or lake cell fills with water up to its base height (`getWaterLevel` needs river
  noise 0), and on land the noise caves' ceiling stays 8 to 17 blocks under the ground (their
  carving mask, `1 - breach noise * river noise`, stays 1), so they don't breach it.
  `sparse_forests` starts at 1 instead.
- TerraForged's biomes `terraforged:cave` and `terraforged:oak_forest` exist in TerraForged worlds
  only; vanilla worlds' biomes, synced registry and commands stay 26.3's. The bundle ships no
  vanilla biome file: the two are derived at runtime from the game's own 26.3 data, as 6dd607eb's
  `ModBiome.create` derives them. `oak_forest` copies plains with no `minecraft:` tag, as in 6dd607eb;
  TerraForged Reimagined copies forest and tags it `minecraft:is_overworld` (a surface biome) and
  `minecraft:is_forest`, which this port does not follow. As in Java, a mod biome keeps its parent's
  music, ambient sounds, water fog and ambient particles: `terraforged:cave` plays dripstone caves'
  `music.overworld.dripstone_caves` (Horizon reads those from the world's biome table, #266).
- TerraForged's pack makes every overworld taller; here only TerraForged worlds get it.
- Far terrain (Horizon's distant tiles; TerraForged has none) shows TerraForged's ground and biomes with
  the vanilla surface rule's top blocks, but from the un-eroded height: erosion's regions are too costly to
  build for distant tiles (about 20 s per wide tile), so far ground can sit a few blocks (about 2 on average,
  at most about 10) off the real chunks'. The `eroded_far_terrain` setting ([Settings](#settings)) gives the
  finest tiles, near the player, the eroded ground once a background thread has built their regions.
  With Reduced far detail (Horizon's video setting, off by default) the surface rule paints only the top two blocks of a far column instead of eight, so the sides of steep far
  columns show stone under them (grey stripes on distant badlands slopes, which Full far detail paints in
  terracotta bands); heights, top blocks, water and biomes are unchanged. It saves 10–35% of a far tile's
  sampling; most of Reduced's speed comes from Horizon merging far tiles. Decorated far chunks
  (Horizon's far decoration) are generated in full, eroded. Each far-decoration thread runs its own pipeline (its
  structure pieces' placement flags can't be shared), but the pipelines of one world share the first one's noise
  (terrain graphs, climate, continent and river caches; TerraForged has one `Generator` per level), each with its own
  terrain cache, task threads and erosion regions, so far decoration never evicts the regions the level's chunks
  read. Results are the same either way: the noise is a pure function of seed and position.
- The boat hook (boats float up small water steps) works only in TerraForged worlds (Arthur's
  choice); TerraForged's mixin works in every world.
- `/locateterrain`, `/tf export structures`, `/tf regen` and `/tf preview` exist only in TerraForged worlds
  (TerraForged registers its three everywhere); `/locateterrain`'s answer comes a moment later, and a radius
  above 100 answers "Unable to locate" where TerraForged's search never ends.
- `/tf regen <radius>` takes a radius of at most 32 and keeps the area's entities, as TerraForged does. Its
  outermost ring is decorated against the chunks kept around it as they were generated, not as they are: what it
  writes into them lands only where they still hold their generated blocks, without block entities. What the kept
  chunks' own decoration once spread into the area (leaves of trees rooted next door) is replayed into it;
  TerraForged's regen loses it.
- `/tf preview [size] [zoom]` is Horizon's: TerraForged has no in-game preview, only a developer window
  (`Previewer`, never opened by the game). The command draws that window's map of the player's own world to
  `terraforged-preview.png` in the world folder.
- The Create World map is Horizon's: neither TerraForged tree shows a map on that screen (TerraForged
  Reimagined draws its own `PreviewWidget`, with relief shading, render modes and zoom, on its settings
  screen, which is not ported). It is `/tf preview`'s picture, 64 pixels over 4096 blocks.
- TerraForged's dimension effects (`DimensionEffects`: clouds at y=300, everything else vanilla's) need no
  port: 26.3 has no effects registry, and the world pack's dimension type (Reimagined's) sets the same height
  as its `minecraft:visual/cloud_height` attribute, which Horizon honours. So TerraForged worlds draw their
  clouds at y=300, on their server thread, for LAN guests and for a dedicated server's players; other worlds
  keep vanilla's 192.33.
- A data pack's climate files (`data/terraforged/worldgen/climate/*.json`, TerraForged's `ClimateType`
  registry) are not read: the climate pools are built as from TerraForged's own eleven empty files.
- Not ported: the unregistered test volcano feature.

## Settings

Horizon's own additions to TerraForged (none of them is TerraForged's), every one off by default, which
is the faithful port. A new TerraForged world takes them from `config/terraforged/settings.json` in the
game folder (a dedicated server's own folder, beside `server.properties`, for the world its
`level-type=terraforged:normal` makes), written with every setting off the first time, and keeps them in its overworld generator
(`generator.horizon_settings` in `world_gen_settings.dat`, beside TerraForged's own generator fields),
so changing the file later leaves existing worlds as they are; Re-Create keeps the world's own. The
Create World map is drawn with the settings the new world will get, though none of them changes it. A world
made before the settings existed has every setting off. Vanilla worlds never get the block or the file. The game installs a world's
settings as it opens it (also on a dedicated server running a TerraForged world); clients of a server
don't generate, so they need none.

- `deterministic`: the trees depend much less on the order the chunks decorate in. Vegetation seeds
  each placement with how many features placed before it in the chunk (`offset++`), and reads the
  ground from the heightmap, where a neighbour's overhanging leaves may already be: whether a
  neighbour decorated first changes both, and with them every later tree of the chunk. With the
  setting, each placement is seeded from a fixed slot (its kind, the biome's place in the chunk, its
  column or grass pass, and the feature's place in its list), on the ground under any logs, leaves,
  huge mushrooms, vines or bee nests. Decorating a wooded 3x3 in two orders, about 15% of its tree
  blocks differ with the setting and 35% without. Not covered: trees that grow into each other
  across a chunk border, a tree whose own placement (its `heightmap` modifier, which 6dd607eb keeps)
  lands on a neighbour's leaves, and vanilla's underground blobs (ores, granite...) that overlap a
  border, which depend on the order in every world. The terrain needs no setting: every chunk sample
  starts from the same river noise (see `sparse_forests`), so heights, rivers and biomes never depend
  on threads or order.
- `sparse_forests`: every chunk sample's river noise starts at 1, a new sample's value and what
  TerraForged's code intends (TerraForged Reimagined resets it to 1 on every call), instead of the 0
  6dd607eb's reused samples settle at in play (the default, see the differences above). Tree fitness
  (`SaturationViability`, 1 − river noise) then falls away from rivers and lakes, and the forests
  with it: they gather along the water and thin out elsewhere. Cave breaches open through land again
  away from rivers, and river cells above the river bed fill to sea level instead of their base
  height (where the ground is higher anyway). Deterministic either way, with `deterministic` on or
  off.
- `corrected_biomes`: TerraForged Reimagined's biome fixes (its `BiomeUtil.isClimateBiome` and
  `CaveBiomeSampler`, MIT). TerraForged's climate pools take every overworld biome no climate file
  names, so `deep_dark` spreads sculk over grass and `deep_ocean` covers dry land. With the setting
  they leave out the cave biomes (Reimagined's `terraforged:non_surface` tag: `deep_dark`,
  `dripstone_caves`, `lush_caves`, `sulfur_caves`, `terraforged:cave`, plus a data pack's additions to
  that tag and any biome whose path starts with `cave/` or ends in `_caves`) and the oceans, rivers,
  beaches and `stony_shore`, which TerraForged's own overrides place where they belong. A climate file
  that names one of them still places it. Unique caves draw from those cave biomes (in registry order;
  `terraforged:cave` alone if there are none) instead of every registered biome, the End's and the
  Nether's included. Structures still see every overworld biome as possible. `/locate biome` finds
  the cave biomes either way: off as noise biomes (at the search's first height), on where the
  unique caves carve them (at a block of the cave); `/tf preview` colours by climate and is the same
  either way. Off, generation is unchanged.
- `height_scale` (a number, 1 by default, held to 0.5–1.85): every height above sea level and depth
  below it times the setting. TerraForged puts a land column at `sea_level + base_height × base +
  (max_y − sea_level − base_height) × terrain`, so the world's levels become `base_height × scale`,
  `max_y` and `sea_floor` stretched around the sea level (the floor no lower than y 0, the levels
  codec's bound, so oceans deepen only up to 1.55); the levels saved in the world stay TerraForged's.
  Landforms keep their width (TerraForged's `auto_scale` would widen them with the range), so they grow
  steeper as well as taller. At 1.85 even the highest land the noise can make (476 blocks above the
  sea at 1) stays under the overworld's top, y 959, so nothing is cut flat. Whatever reads TerraForged's
  levels follows: rivers and lakes, erosion, surfaces, vegetation and steepness (which scale with
  `max_y`, as with a data pack's own levels), structures, distant tiles, `/locateterrain` and `/tf
  preview`. A river's bed and banks keep their heights in blocks around its base level (the carver
  counts them in `1 / max_y` units), so the valleys through the taller land get deeper; caves keep
  their own heights, so tall mountains have no caves near their tops; the climate ignores height, as
  TerraForged's does, so a taller summit keeps its biome (vanilla's height-adjusted temperature still
  snows on it, and `altitude_climate` cools it further). Neither TerraForged tree has this setting: their heights change only with a pack's
  `levels` (where `auto_scale` widens the land too), and TerraForged Reimagined's
  `globalVerticalScale` multiplies only the land terrain's noise through terrain overrides, which this
  port, following 6dd607eb, doesn't apply.
- `eroded_far_terrain`: Horizon's finest distant tiles (cells of up to 4 blocks, which it draws within its
  far-decoration radius: about 1024 blocks around the player at Full far detail, 512 at Reduced) stand on the
  eroded ground, the chunks' own, instead of the un-eroded ground a few blocks off it, so they no longer pop when
  the chunks replace them. A tile still draws un-eroded first (as fast as without the setting) and queues the
  erosion regions it reads (128 blocks a side); one background thread per world builds them into a far cache of
  its own (256 regions, 16 MiB), never into the 16 regions the chunks being generated read, and a region the chunks
  have built already is read from their cache without building or evicting anything. Tiles sampled after their
  region is ready are eroded; Horizon keeps a tile it has drawn until its plan changes (the player moves closer or
  away), so near the spawn the first tiles stay un-eroded until then. The thread is started by the world's first
  far-decoration pipeline on its thread, and a new thread inherits its creator's scheduling class on Linux (and
  its quality of service on macOS), so it runs at idle priority like Horizon's far decoration; on Windows it runs
  at normal priority. Without far decoration no region is built. Wider tiles, farther out, keep the un-eroded
  heights: erosion's few blocks don't show there, and eroding them would cost a region per 128 blocks. Changes no
  block of the world; off, distant tiles are as before.
- `erosion_deposits`: shows where the erosion pass left sediment. Neither TerraForged tree places
  any: Engine 0.3.0's `Erosion` keeps a column's deposits in `Cell.sediment`, but nothing reads it,
  and the erosion this port runs (6dd607eb's, over Reimagined's regions) works on a bare height map
  that keeps only the result. So the setting reads that result: each column's change from erosion
  (its eroded height minus its raw one) against the mean of the 7x7 columns around it, measured on
  the erosion region's map, so neighbouring chunks agree. Erosion lowers nearly all ground a little;
  where a column came out at least as high as that mean (it lost less than its surroundings, or
  gained), one to three blocks of the natural surface (grass, dirt, coarse dirt, podzol, sand, red
  sand, stone, mud, from the top down to the first other block) turn into gravel and clay in river
  and lake beds, gravel on the banks beside them, and scree (gravel, some cobblestone) at the foot
  of slopes, where ground within two blocks stands four or more higher. Only blocks change, never a
  height, so water, rivers, structures, spawn and everything that reads heights are as without it.
  The sea floor and its shores keep their blocks (vanilla's surface rules already give them sand,
  gravel and clay); snow from decoration still covers deposits on cold slopes. Distant tiles don't
  show them, `/tf preview` neither. Whether a column is a bank or the foot of a slope is judged
  inside its chunk, so a bank whose water lies in the next chunk stays bare. Off, generation is
  unchanged.
- `realistic_vegetation`: trees and ground cover follow the light and the water, which no
  TerraForged tree models (its vegetation weighs slope, height, noise and "saturation", 1 − river
  noise, the same everywhere inland in play). Two pointwise measures come with every column: its
  aspect, as the topographic northness `cos(aspect) · sin(slope)` (−1 to 1, from the heights either
  side of it in the chunk's bordered noise map, as TerraForged's gradient; `TerrainData::facing`), and
  its distance to water (0 to 1: the least of the nearest river's and lake's, 0 on the bed and 1 at
  the valley's edge, and the coast's, 0 at the beach and 1 from `INLAND` on; `RiverCarver::water_distance`,
  from the carver's own node sample). Every vegetation config's fitness becomes
  `clamp(fitness + 0.4 · aspect − 0.3 · water − 0.05, 0, 1)` through two Horizon rules, `horizon:Aspect`
  (northness ramped from 0 at −0.5 to 1 at 0.5) and `horizon:WaterDistance` (ramped 0 to 1), so level
  ground half-way out from water keeps TerraForged's fitness, shaded slopes by water gain up to 0.35
  and sunny slopes far from water lose as much. A data pack's vegetation config may use the two rules
  itself (`{"type": "horizon:Aspect", "facing": "north", "min": -1, "max": 1}`, `facing` one of the
  four compass names; `{"type": "horizon:WaterDistance", "min": 0, "max": 1}`); NaN passes through
  them as through `Saturation`. Then, after `Surface`, grass blocks open to the sky on slopes facing
  north by more than 0.15 (about 9 degrees due north) turn, in 2x2 patches, to podzol (and, a third of
  them within 0.3 of water, moss): a share growing with the shade up to a northness of 0.5, from 40%
  far from water to all of them at it. On slopes facing south as much, a share growing the same way,
  from 40% at the water to all far from it, turns to coarse dirt. Only those top blocks change, never a
  height. Across a wide area about two thirds of the land lies 0.9 or more from water (seeds 777 and
  98765); spawn, by water, is wetter than most. North is shaded by the convention of the Northern Hemisphere's
  mid latitudes, where north-facing slopes get less sun and keep more moisture and forest
  (topographic insolation, e.g. Holland and Steyn 1975, "Vegetational responses to latitudinal
  variations in slope angle and aspect", J. Biogeography 2; riparian forests along water courses):
  Minecraft's sun stands overhead at noon, so its sky casts no such shade. Distant tiles and `/tf
  preview` don't show the ground cover. Off, generation is unchanged.
- `altitude_climate` (with `altitude_climate_strength`, a number, 1 by default, held to 0–2): the
  climate cools with height. TerraForged picks a column's biome from 2D temperature and moisture noise
  alone, so a summit keeps its lowland's climate: at `height_scale` 1.85 seed 777's highest warm land, y 443
  at (1024, 2208), is sparse jungle. Only vanilla's height-adjusted temperature (`Biome.getHeightAdjustedTemperature`,
  0.00125 colder per block above sea level + 17, with a little noise) cools it, which snows on plains
  (0.8) only 520 blocks above that. With the setting each block above sea level + 17 cools by
  `0.00275 × strength` more per block (0.004 in all at strength 1, about three times vanilla's: the
  environmental lapse rate, about 6.5 °C per km, steepened for Minecraft's short mountains; at strength 0
  the climate is vanilla's). The biomes stay where TerraForged puts them (moving them would reshuffle every
  biome border on relief; see the issue), and `/locate biome` and `/tf preview` are unchanged. Two things
  follow the cooled air:
  - the snow line: after the decoration, the decorator runs vanilla's `freeze_top_layer` again over the
    chunk with each block's cooled temperature: open water freezes and the top gets a snow layer (the block
    under it `snowy`) where vanilla's own `shouldFreeze` and `shouldSnow` pass (under 0.15). Short grass,
    ferns and their tall forms are cleared where it snows, or TerraForged's grass passes would keep the
    snow off most columns. TerraForged's `Surface.applyPost` then smooths that snow on gentle ground and
    strips it to stone on steep slopes, as on every snowy mountain, so steep summits turn stony. At
    strength 1 snow lies from about 140 blocks above sea level + 17 in a forest (0.7), 160 in plains
    (0.8), 200 in a jungle (0.95); a desert or savanna (2.0) needs 460, more than TerraForged's land
    reaches at `height_scale` 1.
  - the treeline: a tree's fitness is multiplied by a share that falls from 1 to 0 as the cooled
    temperature drops through 0.35–0.25 (just warmer than the snow line: plains thin out about 50 blocks
    under their snow line and stop 25 under it), or, in a biome colder than 0.55, through the band 0.3
    under its own temperature, so a taiga or snowy plain keeps its lowland forest and stops about 75 blocks above
    sea level + 17. Past the band's middle (at the chunk's corner) a biome without a TerraForged vegetation
    config grows no trees there, and no biome grows its other plants (bamboo, bushes, flowers, huge
    mushrooms), some of which are trees too: a bamboo jungle's `bamboo_vegetation` grows jungle trees.
    With `realistic_vegetation` the share multiplies that setting's fitness, so shaded slopes are still
    wooded up to the treeline, not beyond it.

  The lapse counts blocks above the sea, not a share of the land's height, so with `height_scale` the
  taller land reaches further into the cold, as real mountains do. Every value is a function of the block's
  biome and position, so chunks agree at their borders. Weather in play stays the biome's: rain, not snow,
  falls on a warm biome's snowy summit, and its snow and ice don't melt (only block light melts them).
  Distant tiles don't show the snow line. Off, generation is unchanged.
- `rock_strata`: bands of granite, diorite, andesite and stone underground, and erosion that wears soft
  bands faster than hard ones. The bands are TerraForged Reimagined's `StrataDecorator` (its Miscellaneous
  > Strata Decorator, off by default there too; 6dd607eb has none), with its defaults: regions of 600
  blocks, each with its own sequence of 4–6 bands 3–14 blocks thick, repeated down to y 9 and tilted up to
  12 blocks by a 200-block simplex noise, from 8 blocks under the column's floor (so the surface rules
  still dress plain stone), replacing plain stone only. The palette is always the ore-compatible one
  (`oreCompatibleStoneOnly`, Reimagined's default): exactly `#minecraft:stone_ore_replaceables`, so ores
  still generate through the bands. Two departures:
  - Reimagined's bands change in a vertical wall at a region's border. Here a column's band index (how
    many bands lie under the block, counted through each band's thickness) is the mean of the four
    nearest region centres' indices, weighted by distance (bilinear), so every band's floor moves
    smoothly from one region to the next, at most a block between neighbouring columns up to y 320; at a
    region's centre the bands are Reimagined's. Each band takes its rock from one of those regions: the
    heaviest, once each weight is scaled by a factor (0.5–1.5) drawn per region and band, so the bands
    change rock one at a time between centres, like a lateral change of facies.
  - New (in neither tree): each rock has a hardness, which the erosion pass reads. Where a droplet erodes,
    it takes `erode_speed × (1 − H)`, with `H` the hardness of the rock at the droplet cell's height
    (the block the fill places at that height) plus 0.2 on TerraForged's `plateau` and `badlands`
    terrains (their caprock), at most 0.9: granite 0.8, diorite 0.75, andesite 0.6, stone 0.2. The order
    follows measured rock strength (Selby 1980, "A rock mass strength classification for geomorphic
    purposes", Z. Geomorphologie 24; Sklar and Dietrich 2001, "Sediment and rock strength controls on
    river incision into bedrock", Geology 29): coarse plutonic rock resists best; plain stone stands for the
    weaker sedimentary layers between, so they wear back into benches under hard ledges (differential
    erosion, as in cuestas and mesas). The erosion region keeps its geometry (8×8 chunks and a 4-chunk
    margin), so this adds no seam beyond the regions' own, and heights stay a pure function of the seed and
    the region. A hardness of 0 everywhere erodes exactly as without the setting.

  The bands start 8 blocks under the floor, so they show on cliffs, in caves and in ravines, not on gentle
  ground; TerraForged's `Surface` still turns steep slopes to plain stone at the top, and snow
  (`altitude_climate`), ground cover (`realistic_vegetation`) and deposits (`erosion_deposits`) lie over them
  as before. Distant tiles never show the bands; with `eroded_far_terrain` the finest ones stand on the
  strata's eroded ground, the chunks' own (the far erosion thread erodes with the same hardness), and otherwise on
  the un-eroded ground as before. `/tf preview` shows neither. Off, generation is unchanged.
- `coastal_landforms`: coasts follow their exposure to the waves, with cliffs on exposed shores and
  tidal flats in sheltered ones. TerraForged has one shore profile everywhere: in continent noise from
  `BEACH` to `COAST` (`getBlend`) the land rises linearly from the sea level to the land's height, and
  from `SHALLOW_OCEAN` to `BEACH` the sea floor rises linearly to the sea level. Real coasts sort by the
  wave energy that reaches them and the strength of their rock. Exposed coasts of resistant rock retreat as
  steep cliffs (Sunamura 1992, *Geomorphology of Rocky Coasts*, Wiley; Trenhaile 1987, *The Geomorphology
  of Rock Coasts*, Oxford). Sheltered coasts build wide intertidal mud and sand flats, nearly level and
  barely awash (Dyer 1998, *Estuaries: A Physical Introduction*, Wiley; Masselink and Hughes 2003,
  *Introduction to Coastal Processes and Geomorphology*, Arnold). The setting, in neither tree:
  - an exposure field: two octaves of simplex noise (900 and 250 blocks) of the seed, around 0.5. With
    `rock_strata` it adds `0.5 × (H − 0.5)`, with `H` the hardness of the rock at the sea level, so
    granite coasts lean to cliffs and plain stone coasts to flats;
  - cliffs where the exposure is over 0.55 (full from 0.7): within the first 6% of the coast band the land
    rises up to 30 blocks more than the faithful ramp (`1 − (1 − a / 0.06)²`, steepest at the foot), never
    above the land behind it, then follows the ramp up 30 blocks higher. The shore is a wall of rock:
    TerraForged's `Surface` turns its face to stone, and the beach on its top becomes a stony shore;
  - tidal flats where it is under 0.42 (full under 0.28): the land rises as `a²`, a low coastal plain. The
    outer 20% of the shallow sea floor rises to half a block under the sea level (ramping back to the
    faithful floor over the next 20%), so it lies under one block of water (the sea floor's own blocks, sand
    or gravel by biome).

  The change is an offset on each column's height, added after the erosion and before the rivers carve
  (rivers still cut through cliffs). The chunks, structures' base heights (`base_height`, `base_column`)
  and Horizon's distant tiles all read it: un-eroded tiles add it to the un-eroded height, and with
  `eroded_far_terrain` the finest tiles add it to the eroded one, as the chunks do, so they still stand on the
  chunks' ground. It is a function of the seed, the column's position, its un-eroded continent sample and
  the height it is added to, so chunks agree at their borders, whatever the threads or order. Cliff tops
  are higher ground, so `altitude_climate` cools them as any other land. `/tf preview` and the Create World
  map don't show it. Off, generation is unchanged.
- `glacial_landforms`: valleys a glacier has filled are troughs. TerraForged cuts every valley with the
  river's profile (`RiverCarver`), in the tropics as on frozen summits. A glacier widens and deepens its
  valley into a U: a broad, flat floor and walls that steepen as they rise (Svensson 1959, "Glacial erosion
  and the U-shaped valley", *Geografiska Annaler* 41; Harbor 1992, "Numerical modeling of the development of
  U-shaped valleys by glacial erosion", *GSA Bulletin* 104), whose cross-sections fit a power law
  `y = a·x^b` with `b` near 2 (Graf 1970, "The geomorphology of the glacial valley cross section", *Arctic
  and Alpine Research* 2). The setting, in neither tree (the first slice of #276):
  - a glaciation field G, 0 to 1: TerraForged's climate temperature taken at the column itself (the
    field the biome cells sample at their centres, so it has no steps at their edges), put on vanilla's
    scale (`1.6 t − 0.15`: the warm edge of TerraForged's tundra on vanilla's snow line 0.15, its
    temperate middle on a forest's 0.8), cooled with the column's height before the rivers carve it, by
    vanilla's 0.00125 per block plus `altitude_climate`'s 0.00275 times its strength (strength 1 when that
    setting is off), from 17 blocks over the sea, and moved ±0.1 (about 25 blocks of height) by a
    1200-block simplex noise. Ice starts at 0.2 (a little over the snow line: glaciers flow down past it)
    and is full at −0.1, 75 blocks higher. Cold climates are glaciated down to the sea, temperate ones
    where their mountains reach the cold;
  - U-shaped troughs: where G is over 0 the river's valley widens to `(1 + 0.35 G)` times its width,
    with a flat floor at the river's bank level from the banks `0.4 G` of the way out to the trough's
    edge, then walls rising as the square of the distance to the terrain at that edge. On seed 777 the
    floors of snowy valleys lie tens of blocks, up to about 150, under their faithful slopes, and are
    about 50 to 150 blocks wide. A column takes the lower of the river's profile and the trough's, lerped
    by G, and the water level (base noise) follows the same rule, so the river bed stays and no trough
    holds water the river doesn't. Lakes, river noise (biomes, the map) and the terrain type are the
    river's.

  The trough is a function of the column's position, its terrain sample and its nearest river node, so
  chunks agree at their borders whatever the threads or order. The chunks, structures' base heights (ground
  and water level) and Horizon's distant tiles read it: un-eroded tiles take the glaciation over the
  un-eroded height, and with `eroded_far_terrain` the finest tiles take it over the eroded one, as the
  chunks do. `/tf preview` and the Create World map (biome colours and river beds)
  don't change. Rivers carve after `coastal_landforms`, so a trough that reaches a cliff coast cuts a notch
  down to its floor. Not yet: cirques at river sources, fjords where troughs meet the sea, moraines at the
  glaciers' lower limit, and a "past ice age" field for temperate mountains. Off, generation is unchanged.

## Layers

The layers were added in this order; later notes supersede earlier ones, and [Status](#status) sums up what
TerraForged worlds get and what is missing today. Where an early note said a layer was "not ported yet" or that
nothing called it yet and a later layer changed that, the note now points to that later layer or says why no
TerraForged world calls it.

Engine layers not on 6dd607eb's path: 6dd607eb links Engine 0.3.0 but generates with its own `ContinentNoise`
(`continent_noise.rs`), whose `RiverGenerator` (`river_network.rs`) lays rivers and lakes and whose `RiverCarver`
(`continent_carver.rs`) carves them, with its own climate noise (`current_climate.rs`) and bundled terrain noises.
These Engine layers are ported and parity-tested, but no TerraForged world calls them; they stay as reference:

- Engine's `LandForms` (`landforms.rs`), which only 6dd607eb's data generator `ModTerrains` calls (the bundle ships
  the terrain noises it generated; the noise caves borrow only its `noise_warp` helper), its `RegionModule`, populators, `StandardTerrainProvider` and heightmap blenders
  (`region.rs`, `populator.rs`, `provider.rs`, `blend.rs`);
- Engine's simple, advanced and fancy continents (`continent.rs`, `advanced_continent.rs`, `fancy.rs`);
- Engine's river stack (`river.rs`): `GenWarp`, `River`, `RiverCarver`, `LakeCarver`, `WetlandCarver`, networks,
  river maps, `BaseRiverGenerator` and the simple and fancy river generators, and their river caches. 6dd607eb carves
  no wetlands (it ships only a `wetland` terrain type), so TerraForged worlds have none;
- Engine's `Heightmap` (`heightmap.rs`), its tiles, tile generator and tile cache (`tile.rs`, `tile_generator.rs`),
  `WorldFilters` with Engine's own `Erosion`, smoothing, steepness and beach filters (`filters.rs`; the world's
  erosion is Reimagined's, `erosion.rs`, which takes only `FastRandom` from it), and Engine's climate stage
  (`ClimateModule`, `climate.rs`; the world's climate, `current_climate.rs`, takes only its settings, `ClimateRange`, `Moisture`,
  `Temperature` and `perlin_warp`).

The first layer ports the deterministic Noise2D primitives.

On top of them sit TerraForged's octave sources (Perlin, Perlin2, legacy simplex, improved simplex and
cubic) with their configuration, output bounds and the original explicit-seed behavior.

The ridged sources (Ridge, Billow and SimplexRidge) and the `Rand` white-noise source follow, including Engine's
quirks: `Rand.nextInt` divides by `range + range` (so it is almost always 0, and wraps for large ranges), and Engine
throws for ridged sources with more than 30 octaves, where this port keeps going.

The graph layer adds Engine's `Module` graph: constants, source nodes, the Abs, Alpha, Bias, Boost, Clamp, Invert, Map,
Power, Scale and Threshold modifiers and the Add, Multiply and Max combiners, with the shortcuts of Engine's builder
methods (`map`, `clamp`, `pow`, `bias`...). Its tests check the bounds each node reports as well as raw sample bits,
including Engine's quirks: Threshold reports its source's bounds rather than 0..1, and Max's minimum is the larger of
the first source's minimum and the other sources' maxima. `Module::constant` is `Source.constant`, which returns its
shared `ZERO` for -0.0, so it samples +0.0.

The domain layer adds Engine's coordinate warps (`Domain.DIRECT`, `DomainWarp`, `DirectionWarp`, `AddWarp`,
`CompoundWarp` and `CumulativeWarp`) and the `Warp` module that samples its source at the warped point. Its tests check
raw bits, including Engine's quirks: `DirectionWarp` reads `NoiseUtil`'s 4096-entry sine table, `DomainWarp` maps a
constant axis to NaN, and `CumulativeWarp` returns the second warp's coordinate rather than its offset. `CacheWarp`
(`Domain.cache()`) is not ported: Engine never sets its `cached` flag, so it only passes samples through. The seed/scale
shorthands that build noise warps (`Domain.warp(seed, scale, octaves, strength)`, `Module.warp(seed, ...)`...) are not
part of `noise.rs`: the layers that need them build the same warps (`landforms::noise_warp`, `GenWarp` in `river.rs`,
and `ContinentNoise`'s inlined warps).

The selector layer adds Engine's `Base`, `Blend`, `VariableBlend` (`blendVar`) and `Select` modules, with the bounds
Engine reports for them: `Base` keeps its base's minimum and the larger maximum, the others the smaller minimum and
larger maximum of their two sources. Its tests check raw bits, including Engine's quirks: with a zero falloff `Select`
only picks its upper source above `upperBound`, and `Blend` fixes its blend window from the control's bounds when it is
built.

The multi-blend layer adds `MultiBlend` (`Module.blend(blend, sources...)`): it splits the control's 0..1 range into
one evenly spaced slot per source and blends neighbouring slots, with Engine's slot edges, `NoiseUtil.round` slot
lookup and reported bounds (the smallest minimum and largest maximum of the sources, 0 and 0 without sources). Where
Engine throws (a control far enough outside 0..1 to pick a slot that does not exist, or sampling a `MultiBlend` without
sources), this port panics.

The terrace layer adds `Terrace` (`Module.terrace(ramp, cliff, rampHeight, steps, blendRange)`): it clamps its source
to 0..0.999999 and snaps it to flat steps, rising to the next step through a ramp (scaled by `rampHeight`) and a cliff.
As in Engine, the steps are fixed when it is built and start at 0, not at the source's minimum, spaced by the source's
range over `steps - 1`; it reports the source's bounds. Where Engine throws (sampling a NaN, or a terrace with no
steps), this port panics.

The legacy terrace layer adds `LegacyTerrace` (`Module.legacyTerrace(lowerCurve, upperCurve, steps, blendRange)`): it
clamps its source to 0..1 and snaps it to the nearest of the same evenly spaced steps as `Terrace`, easing into the
neighbouring steps with Curve3 transitions whose height is scaled by `lowerCurve` (towards the next step) and
`upperCurve` (from the previous one); it reports the source's bounds. Its tests check raw bits against a Java oracle,
including Engine's quirks: a NaN sample rounds to the first step, and below the first step's window it returns that
step even when a blend range above 1 inverts the windows. Where Engine throws (sampling a legacy terrace with no steps),
this port panics.

The advanced terrace layer adds `AdvancedTerrace` (`Module.terrace(modulation, mask, slope, blendMin, blendMax, steps,
octaves)`): above `blendMin` it snaps its source to `steps * i` steps for each octave `i`, pulls each result back
towards the source by `slope`, adds `modulation` and divides by `modRange` (the source's maximum plus the modulation's,
fixed when it is built), then blends that in, fully above `blendMax` and scaled by `mask`; a zero mask or a sample at
or below `blendMin` returns the source sample unchanged. It reports the source's bounds. Its tests check raw bits
against a Java oracle, including Engine's quirks: zero steps turn every terraced sample into NaN, `steps * i` wraps
like Java's `int`, and a zero `modRange` divides by zero. Where Engine loops forever (`octaves` = `Integer.MAX_VALUE`), this port stops.

The curve layer adds Engine's curve modifiers: `Curve` (`Module.curve(CurveFunc)`, with an `Interpolation`, a
`MidPointCurve` or an `SCurve`), `VariableCurve` (`Module.curve(mid, steepness)` with noise controls), `PowerCurve`
(`Module.powCurve(n)`) and `Steps` (`Module.steps(steps, slopeMin, slopeMax, curve)`). All of them report their source's bounds, as Engine's
modifiers do, even where the output is 0..1. Their tests check raw bits against a Java oracle, including Engine's
quirks: midpoint and variable curves use `NoiseUtil.exp`, `(1 + x / 256)^256`, rather than `Math.exp`; `SCurve` raises a
negative `upper` to `-lower`; `PowerCurve` fixes its curve from the source's bounds when it is built, so a constant
source samples NaN (or 1 for a zero power); `Steps` truncates like Java's `(int)`, so without a slope window (`slopeMax
<= slopeMin`) it rounds samples towards zero onto its steps. `NoiseUtil.pow` keeps `Math.pow`'s NaN for a NaN power and
for a base of ±1 with an infinite power.

The scalar layer adds `Freq` (`Module.freq(x, y)`), which scales the coordinates before sampling its source, `Grad`
(`Module.grad(lower, upper, strength)`), which raises samples at or below `upper` to a power falling to `1 - strength`
at `lower`, `Modulate` (`Module.mod(direction, strength)`), which offsets the coordinates along a direction read through
`NoiseUtil`'s sine table, and the `Min` and `Sub` combiners (`new Min(...)`, `new Sub(...)`). The three modifiers report
their source's bounds. As in Engine, `Min`'s maximum is the smallest of the first source's maximum and the other
sources' minima, and `Sub` subtracts the other sources' maxima from the first source's minimum (and their minima from
its maximum). Their tests check raw bits against a Java oracle, including NaN results: `NoiseUtil.pow` returns the NaN
`Math.pow` returns (its NaN operand, so `Grad` with `lower == upper` gives Java's -NaN from 0/0).

The cache layer adds `Cache` (`Module.cache()`, which returns a module that already is a cache unchanged): it remembers
its last coordinate and sample, shared by every graph that uses it, and reports its source's bounds. Engine's entry is
not synchronised; here it sits behind a lock that is only tried while the entry is copied in or out, so world-generation
threads never wait on one another and a busy cache simply samples its source. Unlike Engine, which compares coordinates
with `==`, the cache compares their bits: -0.0 does not hit +0.0's entry (and a NaN coordinate can hit), so a cached
sample is always the sample of the same input, whichever coordinate another thread sampled last.

The cellular primitive layer adds `NoiseUtil`'s 256-point cell table, the `DistanceFunc` metrics (Euclidean,
Manhattan, Natural), the five `EdgeFunc` functions with their bounds, and the 3×3 searches of `Noise.cell` (for the
`CELL_VALUE` cell function only) and `Noise.cellEdge`. As in Engine, neighbour cell coordinates wrap like Java `int`s
and, among tied cells, the first one found (row by row from the lower corner) wins. Their tests check raw bits against a
Java oracle, including NaN and infinite coordinates.

The cellular source layer adds the graph sources built on them: `CellNoise` (`Builder.cell()`, `Module::cell_source`)
and `CellEdgeNoise` (`Builder.cellEdge()`, `Module::cell_edge_source`). Both sample a single octave at the builder's
frequency and displacement. `CELL_VALUE` maps the nearest cell's value from -1..1 to 0..1, `NOISE_LOOKUP` samples its
lookup module, unmapped, at the nearest cell's feature point (not scaled by the displacement), and `DISTANCE` always
returns 0, as in Engine; `CellEdgeNoise` maps its edge function's value to 0..1 from that function's bounds. Both report
the `Noise` defaults 0..1, whatever the cell function or the lookup's bounds. Their tests check raw bits against a Java
oracle. Where Engine throws (`NOISE_LOOKUP` when no cell's distance is below `Float.MAX_VALUE`: at NaN or infinite
coordinates, or, with the Euclidean and Natural metrics, scaled coordinates beyond about 1.8e19, whose squares overflow),
this port panics.

The sine source layer adds `Sin` (`Builder.sin()`, `Source.sin(scale, alpha)`, `Module::sine_source`): it samples its
`alpha` module, scales the coordinates by its frequency and reads `NoiseUtil`'s 4096-entry sine table at x when alpha is
exactly 0, at y when it is exactly 1, and blends the two by alpha otherwise (so an alpha outside 0..1 extrapolates),
then maps the result from -1..1 to 0..1. It reports the `Noise` defaults 0..1. A NaN alpha samples NaN, but the table
itself never returns one: as in Engine, a NaN coordinate reads entry 0 and an infinite one saturates before it is masked
to an index. Its tests check raw bits against a Java oracle for all three branches, including NaN, infinite and
extrapolating alphas and NaN, infinite and huge coordinates.

The line source layer adds Engine's `Line` (`Line::new`, `Module::line`): a segment whose value falls from 1 on the
segment to 0 where the squared distance reaches its `radius` module's sample (plus an optional minimum), narrowed near
each end by the `fade_in`/`fade_out` modules (fractions of the squared length; zero, negative and NaN fades are
ignored) and scaled by its feather. It reports the `Noise` defaults 0..1. It also ports `Line`'s clipping tests and
static helpers (`dist2`, `sign`, `intersect`, `distance_on_line`, `distance2_line`,
`distance2_line_including_ends`). As in Engine, a zero-length line and the line's own endpoints sample NaN (0/0). Its
tests check raw bits against a Java oracle, including those NaNs, NaN and infinite points and every collinear case of
`intersect`.

The engine type layer starts on Engine's terrain model, in a second module, `terraforged_engine` (`engine.rs`). It ports
`Seed`, whose `next()` returns its cursor and then increments it, wrapping like Java's `int` (`split()` restarts from the
root, `offset(n)` from root + n), the `ITerrain` property flags with their defaults (`is_submerged`, `overrides_river`
and `overrides_coast` follow from the other flags), and the `TerrainCategory` enum: its eleven categories in Engine's
order, each one's flags, and `get_dominant`, which keeps the later category. Engine shares one `Seed` by reference;
this `Seed` is `Copy`, so code that draws from it takes `&mut Seed`. Its tests check every category's flags, the
ordinals and dominance, and the seed cursor's wrapping against a Java oracle.

The terrain descriptor layer adds Engine's `Terrain` (`engine.rs`): an id, a name and a category, with every `ITerrain`
flag delegated to its category or a parent terrain, plus the `ConfiguredTerrain` variants (their own erosion modifier,
mountain and river-override flags), the volcano terrains (which also override coasts and are volcanoes) and
`CompositeTerrain`, named `first-second`, flat only when both are and with the smaller erosion modifier (`Math.min`, so
-0.0 beats +0.0 and NaN wins). As in Engine, terrains compare by identity (`Terrain::is_same`, no `==`), the render hue
is `NoiseUtil.valCoord2D` of the name's Java (UTF-16) string hash, `with_id` delegates to the terrain's parent or to the
terrain itself (so a composite's own flat flag and erosion are not carried over), and a composite always takes its first
terrain's category and flags: Engine's `getDominant` compares the first terrain's category with itself. Its tests check
ids, names, categories, hue and erosion bits and every flag against a Java oracle. The `TerrainType` registry, which
assigns the ids, is not part of this layer.

The terrain registry layer adds Engine's `TerrainType` registry (`TerrainRegistry`): the 16 built-in terrains with
ids 0 to 15 in Engine's order, `register` (which returns the terrain already registered under the name, else re-ids
the new one with `with_id`), `register_composite` (the same object returns itself; otherwise the lower-id terrain comes
first, and each `lower-higher` composite is registered once), lookups by id (unknown ids give `none`) and by name, and
`TerrainHelper.getOrCreate` (`get_or_create`). Names compare like Java's `equalsIgnoreCase`, with Unicode simple case
mappings one code point at a time (`K` matches the Kelvin sign, `ß` does not match `SS`). Engine's registry is one
static list per Java process, so there the ids of composites and custom terrains depend on what earlier worlds
registered; here each world owns a registry, and its built-in terrains are its own objects. Its tests check every
registered terrain's id, name, category, hue, erosion and flags against a Java oracle.

The world-level layer adds Engine's world settings and `Levels` (`engine.rs`): `WorldProperties` (spawn type, world
height 256 and sea level 63 by default), `WorldControlPoints` (the deep ocean, shallow ocean, beach, coast and inland
thresholds) and `ControlPoints`, which falls back to the defaults unless the points rise strictly from a deep ocean of
at least 0 to an inland of at most 1 (a NaN point fails) and adds `coastMarker`, halfway from coast to inland. `Levels`
converts between block heights and Engine's 0..1 heightmap values: the world height is at least 1, `waterY` is the sea
level minus one and `groundLevel` the sea level plus one (wrapping like Java's `int`), `water` is `waterY` and `ground`
the sea level over the world height (each block height capped at the world height), and `scale`, `elevation`,
`water(n)` and `ground(n)` keep Engine's float math and `(int)` truncation. Heights count from block 0, not from the
world's minimum Y: TerraForged's continent noise passes its `maxY` and sea level (480 and 62 by default) although its
terrain starts at y = -64. Its tests check every field and conversion against a Java oracle, including those levels,
320 and 384 block heights, `int` overflow of the sea level and of `y - waterY`, and heights beyond 2^24. Engine's
static `Levels.scale(steps, settings)` is not ported: neither Engine nor TerraForged calls it.

The world-settings layer adds Engine's `WorldSettings` (`engine.rs`): the seed (0 by default; `transient`, so never
serialized in Engine), the properties and control points above, and `Continent` with Engine's defaults: the
`MULTI_IMPROVED` generator (`ContinentType`, with Engine's indices 0 to 3), the Euclidean shape, a scale of 3000, a
jitter of 0.7, skipping and size variance of 0.25, and 5 distortion octaves with a gain of 0.26 and a lacunarity of
4.33. Its test checks every default's bits against Engine 0.3.0. Engine's serializer and its `@Range` limits are not
ported. These are Engine's defaults, not the values TerraForged generates with: its `NoiseGenerator` sets the seed,
sea level and world height from its own levels and replaces the continent scale and control points.

The terrain-settings layer adds Engine's `TerrainSettings` (`engine.rs`) with Engine's defaults: `General`
(`GeneralTerrainSettings`: a terrain seed offset of 0, a terrain region size of 1200, a global vertical scale of
0.98, a global horizontal scale of 1 and fancy mountains on) and one `Terrain` entry (`TerrainFormSettings`: weight,
base scale, vertical scale and horizontal scale) per landform, with weights of 1 for steppe, 2 for plains, 2 for
hills, 1.5 for dales, 1.5 for plateau, 1 for badlands, 2 for torridonian, 2.5 for mountains and 5 for volcano, and
every scale at 1. Its test checks every default's bits, and those of both `Terrain` constructors, against Engine
0.3.0. Engine's serializer, its `@Range` limits and `Terrain.apply` are not ported. These are Engine's defaults, not
what TerraForged generates with: its `ModTerrains` builds its landforms from these settings with a global vertical
scale of 1 (and fancy mountains off for the two `mountains_ridge` terrains), and weights its terrains with its own
values rather than these: 1.5 for steppe, 2.5 for plains, 2 for each hills terrain, 1.5 for dales, 2 for plateau,
1.75 for badlands, 2.5 for torridonian and 1.25 for each mountains terrain and dolomites, with no volcano entry.

The first LandForms layer adds Engine's `LandForms` (`landforms.rs`, module `terraforged_landforms`) with its first two
landforms, `steppe` and `plains`. Each draws five seeds from the `Seed` in Engine's order: a 3-octave Perlin erosion
noise (scale `2 * scaleH`, lacunarity 3.75, alpha 0.45), the two 3-octave Perlin axes of a warp by `scaleH / 4`
(lacunarity 3 for steppe, 3.5 for plains), the 1-octave Perlin base (scale `scaleH`) that the erosion multiplies, and
the seed of a 1-octave Perlin warp at scale 256 (strength 200 for steppe, 256 for plains; its second axis uses that
seed + 1). `scaleH` is `Math.round(250 * horizontalScale)` of the landform's settings. Steppe then scales by 0.08,
plains by `0.15 * globalVerticalScale`, and both subtract 0.02. Its tests check the seed afterwards, the bounds and the
bits of every sample over a grid of positive, negative and large coordinates against a Java oracle, for Engine's
default settings, ModTerrains' (a global vertical scale of 1) and degenerate scales (zero, negative, NaN, huge, and
`Math.round`'s edges).

The hill LandForms layer adds `hills1`, `hills2` and `dales`. `hills1` draws four seeds: a 3-octave Perlin at scale 200
multiplied by a 3-octave Billow at scale 400 (alpha 0.5), then two Perlin noise warps (scale 30, 3 octaves, strength
20; scale 400, 3 octaves, strength 200), scaled by `0.6 * globalVerticalScale`. `hills2` draws five: a 2-octave cubic
at scale 128 times a 4-octave Perlin at scale 32 (alpha 0.075), the same two warps, then times a 2-octave Ridge at
scale 512 (alpha 0.8, with the builder's ridge gain of 0.975), scaled by `0.55 * globalVerticalScale`. `dales` draws
four and ignores the settings: a 1-octave Perlin at scale 400, clamped to 0.3..0.6 and mapped to 0..1, blends linearly
(midpoint 0.4, range 0.75) from a 4-octave Billow at scale 300 (gain 0.8, lacunarity 4, power curve 0.5, times 0.75)
to a 3-octave Billow at scale 350 (gain 0.8, lacunarity 4, raised to 1.25); the result is raised to 1.125, warped
(scale 300, 1 octave, strength 100) and scaled by 0.4. As in Engine, a vertical scale whose product with 0.6 or 0.55
is exactly 1 leaves that hill landform unscaled. The same grid test covers the three, with the same settings cases and
two such vertical scales.

The plateau LandForms layer adds `plateau`, which draws nine seeds and ignores the horizontal scales. Its valley is an
inverted 1-octave Ridge at scale 500 warped twice (1 octave each: scale 100, strength 150; scale 20, strength 15). Its
top, a 3-octave Ridge at scale 150 (lacunarity 2.45) warped twice (scale 300, strength 150; scale 40, 2 octaves,
strength 20) and scaled by 0.15, is multiplied by the valley clamped to 0.02..0.1 and mapped to 0..1. The valley times
a 1-octave cubic at scale 500 (times 0.6, plus 0.3), plus the top, goes through a four-step terrace (lower curve 0.9,
upper curve 0.15, lower height 0.35, blend 0.4); a 3-octave Perlin surface at scale 20 (times 0.05, warped at scale
40, 2 octaves, strength 20) is added and the sum is scaled by `0.475 * globalVerticalScale`, computed in double as in
Engine. The grid test covers it with the same settings cases, a start seed where the surface warp's second axis seed
wraps, and a vertical scale whose product with 0.475 is exactly 1.

The terraced LandForms layer adds `badlands` and `torridonian`; both ignore the settings. `badlands` draws six seeds.
Its hills, a 4-octave Ridge at scale 275 (the builder's ridge gain of 0.975) warped twice (scale 400, 2 octaves,
strength 100; scale 18, 1 octave, strength 20), are multiplied by a 3-octave Perlin mask at scale 270 clamped to
0.35..0.65 and mapped to 0..1. Its detail adds two step layers, the hills in 4 and in 10 linear steps (slope 0.6..0.7)
times 0.6, each plus the hills warped once more (scale 100, 1 octave, strength 50) times 0.4. Its shape is the hills in
4 Curve3 steps (slope 0.65..0.75) times 0.6, plus the hills times a 3-octave Perlin at scale 200 times 0.4, all times
0.6. The shape times the detail at alpha 0.5 is scaled by 0.55 and raised by 0.025. `torridonian` draws nine: a
3-octave Perlin at scale 200 blends linearly (midpoint 0.6, range 0.6) from plains, a 3-octave Perlin at scale 100
warped twice (scale 300, 1 octave, strength 150; scale 20, 1 octave, strength 40) times 0.15, to boosted hills, a
4-octave Perlin at scale 150 warped twice (scale 300, 1 octave, strength 200; scale 20, 2 octaves, strength 20). The
blend goes through an advanced terrace (modulation: a 1-octave Perlin at scale 120 times 0.25; mask: a 1-octave Perlin
at scale 200 times 0.5 plus 0.5; slope 0.5, blend 0..0.3, 6 steps, 1 octave), is boosted again and scaled by 0.5. The
grid test covers both with the same cases, whose start seeds include ones where a warp's second axis seed wraps.

The ocean LandForms layer gives `LandForms::new` Engine's other two constructor arguments, the world's `Levels` and the
`ground` module, and adds `deepOcean` and the two bases: `getOceanBase` is `Source.ZERO` and `getLandBase` returns
the `ground` module. Unlike the other landforms, `deepOcean` takes a plain `int` seed, which it increments six times
(wrapping like Java's `int`) without touching any `Seed`. It scales by the `Levels`' `water` level (`seaLevel` in
Engine): its hills, a 3-octave Perlin at scale 150 times `water * 0.7` plus a 1-octave Perlin at scale 200 times
`water * 0.2`, and its canyons, a 4-octave Perlin at scale 150 through a 0.2 power curve, inverted and times `water *
0.7`, plus a 1-octave Perlin at scale 170 times `water * 0.15`, are blended linearly (midpoint 0.6, range 0.65) by a
1-octave Perlin at scale 500 and warped (scale 50, 2 octaves, strength 50). As in Engine, `water * 0.7` is computed in
double and the other two products in float. Its test checks the water level, the bounds and every grid sample's bits
against a Java oracle for Engine's default levels (256 high, sea level 63), TerraForged's continent levels (480 and
62), ModTerrains' swapped `new Levels(63, 255)` (a water level of 1), a negative and a zero water level and a sea
level whose `- 1` wraps, with start seeds where the increments and the warp's second axis seed wrap. The other
landforms come with the LandForms layers below. TerraForged's `RandSeed` (ModTerrains builds its landforms from a
`SplittableRandom` seed rather than Engine's counter) is not ported: only 6dd607eb's data generators (`ModTerrains`,
`ModCaves`, `ModVegetations`) use it, and the bundle ships the terrain noise, cave and vegetation files they generated.

The erosion Ridge layer adds Engine's `Ridge` (`com.terraforged.engine.module.Ridge`, `ErosionRidge` in `noise.rs`;
not the `RidgeNoise` source) and its `Ridge.Noise` module (`ErosionRidge::wrap`, `Module::erosion_ridge`). For each
octave it finds, in each of the 3×3 cells around the point (`NoiseUtil.cell`'s jittered feature points at the octave's
grid size), the segment from that cell's point to the lowest-sampled point among its own 3×3 neighbours, and takes the
distance to the nearest such segment over the grid size, clamped to 0..1. Each of an octave's 25 distinct candidate
points is sampled once (a sample of exactly -1, the cache's empty marker, is
sampled again). Octaves start at the grid size with a gain of 1; each later one multiplies the gain by
`amplitude`, the grid size by `distanceFallOff` and the coordinates by `lacunarity`, and the weighted sum is divided by
the summed gains (so zero or negative octaves give NaN, as in Engine). The module then lerps from that erosion to the
source sample by `1 - strength` (`CONSTANT`), `1 - strength * value` (`INPUT_LINEAR`) or `1 - strength * erosion`
(`OUTPUT_LINEAR`). The short constructors default to an amplitude of `1 / (octaves + 1)`, a lacunarity of 2.25 and a
fall-off of 0.75. Like every Engine `Noise` it reports 0..1, whatever its output. Its tests check raw bits against a
Java oracle over a grid of positive, negative and large coordinates and at huge, NaN and infinite points, for all
three modes, a source sampling -1 or NaN, zero and negative octaves, negative, zero and tiny grid sizes, and the
`int` wrap of `octaves + 1`.

The mountain LandForms layer adds `mountains`, `mountains2` and `mountains3`. `mountains` multiplies a 4-octave Ridge
(scale `410 * mountains.horizontalScale` through `Math.round`, gain 1.15, lacunarity 2.35) by a Perlin detail and warps
it; `mountains2` and `mountains3` multiply a warped, clamped cell-edge source (cells of 360 and 400, `DISTANCE_2`) by a
Perlin blur and a Ridge surface and raise it to 1.1, and `mountains3` adds an advanced terrace. With
`general.fancyMountains` on (the default), Engine's `makeFancy` wraps each in the erosion Ridge (2 octaves, strength
0.65, grid 128, `CONSTANT`) warped along a Perlin direction and draws two more seeds; with it off (TerraForged's
`ModTerrains` turns it off for `mountains_ridge_1` and `mountains_ridge_2`) the graph is left as is. They scale by `0.7`
and `0.645` times the global vertical scale, computed in double. The landform grid test checks them against the Java
oracle for every settings case, with fancy mountains on and off, including mountain scales whose `Math.round` gives 0
or keeps an odd integer above 2^23 and vertical scales whose `scale(1)` leaves the graph unscaled.

The cell layer adds Engine's `Cell` (`engine.rs`), the per-column sample the later stages fill in: its height value,
erosion, sediment, gradient, moisture and temperature, continent, terrain-region and biome-region ids and edges, the
terrain region's packed centre (a `long`), macro-biome id, river mask, continent cell coordinates, erosion mask,
terrain and biome type, with Engine's defaults (moisture and temperature 0.5, biome-region edge and river mask 1, the
`none` terrain, `GRASSLAND`). As in Engine, `copy_from` copies every field except the terrain region's centre, and
`reset` copies the defaults the same way, so the centre survives it; an empty cell (`Cell.empty()`) stays absent
through both. Engine's default terrain is the static `TerrainType.NONE`; here `Cell::new`, `Cell::empty` and `reset`
take the world's `TerrainRegistry` and use its `none`. Engine's `Cell.empty()` is one shared mutable object and its
cells come from per-thread pools; here `empty` returns a new cell and callers own their cells. It also adds
`BiomeType`'s eleven types in Engine's order with their ids and `isExtreme`; their colours and the climate ranges and
lookup map Engine loads from TerraForged's biome-type image are the biome-type layer's. Its test checks every field's bits after
construction, `copy_from` and `reset` against a Java oracle.

The terrain-populator layer adds Engine's `TerrainPopulator` and `ScaledPopulator` (`populator.rs`), which write one
landform into a cell: `value` becomes base plus variance (each multiplied by its scale in `ScaledPopulator`), clamped
to 0..1 with NaN left as NaN, and `terrain` becomes the populator's terrain, which must come from the world's
`TerrainRegistry`; no other cell field changes. `TerrainPopulator.of` picks `ScaledPopulator` unless the base and
vertical scales are both 1, and `TerrainConfig` keeps a populator's terrain, variance and weight to rebuild it on
another base (its data spec is not ported). Engine's `Populator` is also a noise `Module`; here its `getValue` is
`Populator::value`, which takes the world's registry for its scratch cell. Its test checks every cell field, on a
new and on a fully set cell, against a Java oracle running Engine's populators on Engine cells, including the hills
and mountains landforms; the oracle lives outside Git, in `.local/terraforged/oracles/populator/`.

The region-module layer adds Engine's `RegionModule` and `RegionConfig` (`region.rs`), the populator that splits the
world into terrain regions. It offsets the point by a `DomainWarp` of the config's two axes at its strength, scales it
by `1 / scale` (the region size; Engine's `Heightmap` passes `terrainRegionSize`, 1200 by default, and 1-octave simplex
axes at scale 400 with a strength of 200) and finds the nearest of the 3×3 neighbouring cells' feature points (jitter
0.7, natural distance, seeded with the config's seed plus 7, wrapping like Java's `int`). It writes only three cell
fields: `terrain_region_id`, the nearest cell's `valCoord2D` mapped to 0..1; `terrain_region_edge`, the
`DISTANCE_2_DIV` edge value mapped to 0..1, inverted, raised to 1.5 (`NoiseUtil.pow`) and stretched from 0..0.5 to
0..1; and `terrain_region_center`, the nearest feature point in blocks packed like `PosUtil.pack` (each half truncated
like Java's `(int)`: saturating, NaN to 0). Its test checks every cell field, on a new and on a fully set cell, over a
grid of negative, large, NaN and infinite coordinates against a Java oracle running Engine's `RegionModule` on Engine
cells, for `Heightmap`'s configuration and for overflowing seeds and zero, negative and huge scales and strengths; the
oracle lives outside Git, in `.local/terraforged/oracles/region/`.

The region-selection layer adds Engine's `RegionSelector` and `RegionLerper` (`region.rs`), which Engine's `Heightmap`
uses to pick each terrain region's landform. `RegionSelector` repeats each populator, in list order, by its weight
divided by the smallest non-zero weight (rounded like `Math.round`; a populator that is not a `TerrainPopulator`, such
as a selector, counts as weight 1, and zero-weight populators are left out), falling back to the unweighted list when
no weight is non-zero or every count rounds to 0 (a NaN weight does this, through `Math.min`). It then applies the
populator at `NoiseUtil.round(terrain_region_id * maxIndex)` (`maxIndex` is the repeated list's length minus 1); where Engine throws for an index outside the list,
this port panics. `RegionLerper` reads `terrain_region_edge`: at exactly 0 it applies only its lower populator, at
exactly 1 only its upper one, and otherwise both on the same cell, setting `value` to `NoiseUtil.lerp` of the two
(other fields, the terrain included, keep what the upper one wrote). Heightmap passes a FLATS plains populator as the
lower one (the region borders) and the selector as the upper one. Its test checks the weighted lists and every cell
field against a Java oracle running Engine's selector and lerper on Engine cells, including Heightmap's chain
(`RegionModule`, then the lerper) over real landforms with ModTerrains' weights; the oracle lives outside Git, in
`.local/terraforged/oracles/region_selection/`.

The volcano layer adds Engine's `VolcanoPopulator` (`populator.rs`), the populator TerraForged registers for volcano
regions. It writes only `value` and `terrain`: the levels' ground plus a cone built from the region config's cell edge
distance and scaled by a per-cell height, turning down into a throat near its top, where the terrain becomes
`volcano_pipe`, and blending into ridged lowlands at its foot, where it is `volcano`; between those bands the cell keeps
its terrain. It draws four seeds in Engine's order and weighs in `RegionSelector` by its own weight, as a
`TerrainPopulator` does. `modify_volcano_type` (run by Heightmap after the rivers) turns a `volcano_pipe` cell below
the water level or with a river mask under 0.85 back into `volcano`. Where Engine's cell lookup throws (non-finite
coordinates) this port panics. Its test checks every cell field over grids and around pipes, for Heightmap's region
config, a small one and overflowing seeds, plus `modifyVolcanoType` at its edges and a `RegionSelector` holding
volcanoes, against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/volcano/`.

The standard-provider layer adds Engine's `StandardTerrainProvider` (`provider.rs`): it registers Engine's default
landforms in Engine's order, each drawing its seeds from the provider's seed (the world seed offset by
`terrainSeedOffset`): steppe and plains as mixable flats, dales, both hills and torridonian as mixable hills, plateau and
badlands as mixable, then a second badlands, the three mountains and the volcano as unmixable. Each landform becomes a
`TerrainPopulator`, or a `ScaledPopulator` when its settings scale it, on the land base. `getPopulators` keeps the
mixable populators with a weight above 0 (so a NaN weight drops one too), adds a composite for every pair of them (a
Perlin control at half the region size, warped by a Perlin warp, blending the two variances; registering
`lower-higher` composite terrains, or the terrain itself for two populators of one terrain, and drawing two seeds
each), then the unmixable populators, and shuffles them as `Collections.shuffle` does with a `java.util.Random`
seeded from the next seed; every call draws new seeds and so returns a different order. Composites go into the
world's registry, which the caller passes in, so populator terrains are the world's own (`is_same`). Variant lookups
fall back to the default populator for a negative variant or an unregistered terrain and cap high variants at the
last one; name lookups compare like Java's `equalsIgnoreCase`. Engine's `GeneratorContext` is not ported: the
provider takes the world seed, levels and terrain settings directly. Its tests rebuild a Java oracle's text over two
configurations (Engine's defaults; a seed offset, zero and NaN weights, scaled landforms and a negative long seed):
the seed cursor, registered populators, variant and name lookups, both shuffled lists with samples on a grid, and the
registry's composites; the oracle lives outside Git, in `.local/terraforged/oracles/provider/`.

The heightmap-blending layer adds Engine's `Blender`, `ContinentLerper2` and `ContinentLerper3` (`blend.rs`), the
three populators Heightmap chains into its root: continent-edge lerps from deep to shallow ocean to coast, and from
those oceans to land, where a mountain-shape module blends region terrain with mountain chains. Below a range's lower
bound only the lower populator applies and above its upper bound only the upper one (both bounds belong to the blend);
inside it both apply to the same cell in turn and `value` is lerped (`ContinentLerper2` linearly by default,
`ContinentLerper3` with Curve3, `Blender` always linearly). `Blender` keeps the lower populator's terrain while its
select is below the midpoint the split sets; the lerpers leave the terrain the last populator wrote. NaN selects fall
through to the blend, as in Java. A `Blender` controlled by a populator reads it as Engine's `getValue` does, on a fresh
cell of the world's registry; its tag threshold is stored and, as in Engine, never read. Engine's `Heightmap` (below) chains
them; 6dd607eb's world generation doesn't (see the Engine layers not on 6dd607eb's path, above). Their tests check every cell field and `getValue` at each boundary, one ulp either side, signed zeros,
NaN and infinities, degenerate and reversed ranges, populators that read the cell, and Heightmap's root chain over a
grid, against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/blend/`.

The position-utility layer adds Engine's `PosUtil`, `PosIterator` and `MutableVeci` (`util.rs`). `pack` puts two ints
in a `long` (left in the high half); the float variants either truncate like Java's `(int)` (`pack(float, float)`, which
`RegionModule`'s region centre now uses) or keep the raw bits, NaN payloads included, as `floatToRawIntBits` does
(`packf`, `packMix`). `contains` is half-open, `iterate` visits a rectangle row by row, and `PosIterator` walks x, then
z, then y with Engine's quirks: moving to the next y layer does not advance the index, its size is `w * h * l - 1`, and
all of it wraps like Java `int`s. Its tests fold every result over 0, -1, `MIN`/`MAX`, quiet and signalling NaNs,
-0.0, infinities, subnormals and the `(int)` saturation edges, and every iterator factory (Engine's six-argument
`range2D` is `range_3d` here), against a Java oracle running Engine's classes; the oracle lives outside Git, in
`.local/terraforged/oracles/posutil/`.

The simple-continent layer adds Engine's `MultiContinentGenerator` and `SingleContinentGenerator`
(`continent.rs`): continents are jittered cells of a cellular lattice (frequency `1 / (4 * continentScale)`, the
configured shape as distance function, `continentJitter` as jitter) sampled through two compounded domain warps, and
their `DISTANCE_2_DIV` edge is remapped from 0.2..1 to 0..1 and faded by a simplex shape mask below the `inland` control point.
`apply` writes only `continentId`, `continentEdge`, `continentX` and `continentZ`; the single-continent generator
zeroes those four outside the continent nearest to (0, 0), so its `getValue` reads 0 there while `getEdgeValue`
does not. It also ports the nearest-centre lookup and the edge and ocean distance searches rivers use; its river
generator comes with the simple-river layer below and its river cache (`getRivermap`) with the continent-river-cache
layer. Its tests fold every cell field and every lookup over a grid with -0, huge, infinite
and NaN coordinates for six configs (all three shapes, jitter 0, 1 and NaN, scale 1 and `Integer.MAX_VALUE`, custom
control points), against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/continent/`.

The advanced-continent layer adds Engine's `AdvancedContinentGenerator` (`advanced_continent.rs`), the generator
of Engine's default continent type: continents are jittered cells of a lattice (frequency `1 / (4 * continentScale)`)
sampled through a Perlin2 domain warp built from the continent noise octaves, gain and lacunarity, and their edge
is the distance to the nearest cell bisector, scaled per continent by `continentSizeVariance`, remapped and shaped
by cliff and bay noise near the coast. `apply` writes only `continentId`, `continentEdge`, `continentX` and
`continentZ` (the centre pulled 35% towards its neighbours' mean), and nothing at all for a continent that
`continentSkipping` skips (never the one at cell 0, 0), so `getEdgeValue` and `getNearestCenter` read 0 there. Engine
does not override `getValue`, so it reads the scratch cell's untouched `value`: always 0. The edge and ocean distance
searches rivers use are ported, the simple-river layer below adds its river generator and the
continent-river-cache layer its river cache (`getRivermap`). One difference: Engine throws for a negative
`continentNoiseOctaves` (an octave table lookup out of bounds); the port uses 0 octaves. Its tests fold every cell
field and every lookup over the simple-continent grid for six configs (skipping and size variance off, 1 and NaN,
scale 0, 1, 2 and `Integer.MAX_VALUE`, jitter 0 and NaN, zero octaves with a negative gain, custom control
points), against a second Java oracle in `.local/terraforged/oracles/continent/`.

The fancy-island layer adds the island geometry of Engine's fancy continents (`fancy.rs`): `Segment`, a line
segment with a scale at each end, and `Island`, a polyline of segments with its centre (the mean of its points), its
bounding box (the segments' box grown by the coast distance, then by the coast times the largest scale, at least 1), overlap and
containment tests (strict), and translation. `getEdgeValue` reads the squared distance to the nearest segment,
divided by the squared scale interpolated along it, and maps it to 0 beyond the deep ocean, up to 0.25 at the
shallow ocean, up to 1 at the coast and 1 inland; `getLandValue` curves the distance inside the shallow ocean. As
in Engine, the box's maximum starts at `Float.MIN_VALUE`, and a zero-length segment or a NaN coordinate makes the
distance NaN (Java's `Math.min` keeps it). Its tests fold every segment field, the bounds and the edge, land and containment samples over a grid
with -0, subnormal, huge, infinite and NaN coordinates, before and after translation, for seven islands (empty,
zero coast, zero-length, NaN and infinite segments, zero and NaN scales), against a third Java oracle in
`.local/terraforged/oracles/continent/`.

The fancy-continent layer ports `FancyContinent` and `FancyContinentGenerator` (Engine's experimental continent
type, `fancy.rs`). The continent draws from a `java.util.Random` seeded with the generator's first seed: a main
island of three segments (id 0), then for each of four directions one yaw and three ever smaller islands (ids 1 to 12
in build order), each placed one radius away from the previous island of its arm, with `NoiseUtil`'s sin/cos lookup.
Its value is the largest island edge value, its land value the largest island land value, and `getValueId` packs the
id of the first island with the largest edge value (-1 when none is above 0) with that value. The generator samples it
through three summed domain warps (Simplex at half the continent scale, then Perlin at 80 and 20 blocks), maps the land
value from 0.2..0.4, writes only `continentEdge` (and 0 to `continentX`, `continentZ` and `continentId`), and its
`getValue` is Engine's default: the scratch cell's `value`, always 0. As in Engine, `getNearestCenter` ignores its
arguments and returns half of `getMax - getMin` in blocks, and `getMax` takes the largest island *minimum*, starting
at `Float.MIN_VALUE`. The river map it holds (`FancyRiverGenerator`) comes with the fancy-river layer below. Its tests fold every island's
geometry, `getMin`/`getMax`, the continent's values over a grid in continent units and every cell field and lookup
over the block grid for seven configs (continent scale 0, 1, 2, -3001 and `Integer.MAX_VALUE`, seeds at the `int`
edges, custom control points), against a fourth Java oracle in `.local/terraforged/oracles/continent/`.

The river-configuration layer adds Engine's river settings and the configs its river map builds from them
(`river.rs`). `RiverSettings` (in `engine.rs`) keeps Engine 0.3.0's defaults: seed offset 0, 8 main rivers per
continent; main rivers with bed depth 5, bank heights 2 to 6, bed width 8, bank width 20 and fade 0.75; branch
rivers with 4, 1 to 4, 5, 14 and 0.975; lakes with chance 0.3, start distance 0 to 0.03, depth 10, size 75 to 150
and bank heights 2 to 10; wetlands with chance 0.6 and size 175 to 225. `RiverConfig` and its builder (defaults
bed width 4, bank width 15, bed depth 5, bank heights 1, length 1000, fade 0.2) turn the block counts into heights
normalized by `Levels` (`water(-depth)`, `water(bank)`), with `length2 = length * length` wrapping as a Java `int`;
Engine's river generator builds main rivers with length 5000 and branches with 4500. `createFork` scales a river's
widths down to a connecting width (rounded with `NoiseUtil.round`) and drops its banks one block, to at least
ground level. `LakeConfig` subtracts its sizes as floats, and `WetlandConfig` keeps Engine's `Variance.of(sizeMin,
sizeMax)`, whose second argument is a range: wetlands are 175 to 400 blocks long, not 175 to 225. These are
Engine's settings, not what TerraForged 0.3.1 carves with: TerraForged's `NoiseGenerator` builds Engine's `Settings`
with these river defaults untouched, but its own continent noise never builds Engine's river map and carves rivers
with its own continent river config (`ContinentConfig.rivers`: seed 0, lake density 0.75; rivers with erosion 0.075, bed width
1 to 7, bank width 3 to 30, valley width 80 to 200, bed depth 1.25 to 5 and bank depth 1.25 to 3; lakes with bed
width 8 to 15, bank width 30 to 45, valley width 80 to 120, bed depth 2 to 8 and bank depth 1 to 1.5), which this
layer does not port. Its tests fold every field of the river, fork, lake, wetland and variance configs over edge
cases (`int` limits, NaN, infinite and negative widths, six `Levels`, including TerraForged's swapped
`Levels(63, 255)`), against a Java oracle in `.local/terraforged/oracles/river/`, which also prints TerraForged's
own config.

The river-warp layer adds Engine's `GenWarp` (`river.rs`), the pair of domain warps its river generators build per
continent to carve lakes and river channels. The lake warp adds a Perlin `Domain.warp` with scale 200, 1 octave and
strength 300 to one with scale 50, 2 octaves and strength 50; the river warp adds scale 95, 1 octave, strength 25 to
scale 16, 1 octave, strength 5. As in Engine, the four warps take seeds `seed + 1` to `seed + 4` (wrapping as Java
`int`s), each warp's y noise uses its seed + 1, the continent scale argument is ignored and `GenWarp.EMPTY` is
`GenWarp::direct()`. Engine's private, unused `scaleMegaWarp` is kept as `scale_mega_warp`. Only Engine's river maps (below) use
these warps; 6dd607eb's world generation builds none. Its tests fold every warp's offsets and coordinates over a grid with
-0, huge, infinite and NaN coordinates for eight seeds (the `int` edges among them) against a Java oracle running
Engine's `GenWarp`, `GenWarpOracle.java` in `.local/terraforged/oracles/river/`.

The river-geometry layer adds Engine's `River` and `RiverWarp` (`river.rs`), the channel segments its river
generators lay out. A `River` keeps its direction, normal (NaN for a zero-length segment, as in Engine), length
(`Math.sqrt` of the `float` squared length) and bounds padded by twice the valley radius (275 blocks, 300 from
nodes), and samples positions, angles, segment intersections (optionally extended at both ends), bound overlaps
and shortened copies (`NoiseUtil.round`). A `RiverWarp` bends a river sideways by simplex noise, faded in over
`0..lower` and out over `upper..1`, plus a wiggle of 2 to 45 blocks faded in over the first 7.5% of the river;
its `create` and `createChild` draw from `java.util.Random` in Engine's order. Engine compares `RiverWarp.NONE` by
identity; the port marks it with a flag. Only Engine's river generators below use them, and 6dd607eb's world generation
calls none of those. Its tests
fold every field and method over 2560 segments (zero-length, NaN, infinite and -0 ends, odd radii), every pair's
intersections and overlaps, and warps with degenerate and NaN fades over distances outside 0..1, against a Java
oracle running Engine's classes, `RiverGeometryOracle.java` in `.local/terraforged/oracles/river/`.

The river-carver layer adds Engine's `RiverCarver` (`river.rs`), which carves one river segment into a cell. Within
the valley radius (275 blocks by default; `create` draws 0.8 to 1.5 times that) it lowers `value` toward the bank
height along the valley curve and lowers `riverMask`; nearer in, the banks (widened toward the mouth, where the
cell's continent edge is below 0.5) and then the bed lower `value` to the bed height, setting `erosionMask` and, at or below the water
level, the world registry's `river` terrain, except over wetland-style terrains that override rivers outside
bed-to-water heights. Widths and depths taper in over the first `fade` of the river. `createForkConfig` sizes a fork
from the bed and banks at a point, `getValleyType` and `create` draw from `java.util.Random` in Engine's order. Its
`TerrainPopulator` base (an empty `apply`) and its unused `compareTo` are not ported. Only Engine's river networks below use the
carver; 6dd607eb's world generation carves with its own continent `RiverCarver` (`continent_carver.rs`). Its tests fold every cell field over 337920 carves of fresh and filled cells
(river, wetland, lake and mountain terrains, NaN values, masks and edges) at points on, beside, before and past
four rivers (zero-length and NaN among them), for degenerate configs (wrapping widths, NaN heights, zero and NaN
fades), plus fork configs and random draws, against a Java oracle running Engine's class, `RiverCarverOracle.java`
in `.local/terraforged/oracles/river/`.

The lake layer adds Engine's `Lake` (`LakeCarver` in `river.rs`, since `Lake` is the settings struct) and
`Boundsf`. A lake has a bed of radius `radius * multiplier` inside a valley of radius `275 * multiplier`. In the
valley, a cell at or above the bank height (the config's bank range, interpolated by the cell's own value) is lowered
toward it and its `riverMask` lowered; in the bed, `value` is capped at the bank height, lowered toward the lake
depth and tagged with the world registry's `lake` terrain. `recordBounds` adds the valley's square widened by 20% to
a `Boundsf.Builder`, whose maxima start at `Float.MIN_VALUE` as in Engine, and `overlaps` tests a point against the
squared lake radius plus a margin. Engine's `TerrainPopulator` base is not ported, as for the river carver. Only
Engine's river networks use `LakeCarver`; TerraForged worlds get their lakes from 6dd607eb's continent instead
(`river_network.rs` lays lake nodes at river sources, `continent_carver.rs` carves them). Its tests fold every cell field over 561600
applies of fresh and filled cells (NaN, infinite and -0 values and masks) at points inside, on the edge of and outside 30 lakes (zero,
negative, NaN and infinite sizes), for eight lake configs (wrapping `int` settings, NaN and infinite heights) and three
`Levels` (TerraForged's swapped `Levels(63, 255)` among them), plus bounds built from NaN, -0 and infinite records,
against a Java oracle running Engine's classes, `LakeOracle.java` in `.local/terraforged/oracles/river/`.

The wetland layer adds Engine's `Wetland` (`WetlandCarver` in `river.rs`, since `Wetland` is the settings struct): a
valley of `radius` around a segment, measured at the (warped) river coordinates. In the valley, a cell above the bank
height (`ground(3)`) is lowered toward it and its `riverMask` lowered; past 65% of the way in, a cell between the bed
and the banks is lowered toward the bed, the innermost part sets `erosionMask`, cells where the pool alpha beats an
edge noise are tagged with the world registry's `wetland` terrain, and cells from the bed up to the mound top move
toward seeded mound noise, sampled at the terrain coordinates. `recordBounds` adds the segment's box widened by the
radius. Engine's `TerrainPopulator` base is not ported, as for the river carver and lakes. Only Engine's river networks use
wetlands, and 6dd607eb carves none: TerraForged worlds have no wetlands. Its tests fold every cell field over 1927800 applies of fresh and filled cells
(NaN, infinite and -0 values and masks) at points inside, on the edge of and outside wetlands (degenerate, NaN and
infinite segments; zero, negative, NaN and infinite radii; seeds that wrap) for three `Levels` (TerraForged's swapped
`Levels(63, 255)` among them), with both plain and warped coordinates, against a Java oracle running Engine's class,
`WetlandOracle.java` in `.local/terraforged/oracles/river/`.

The river-network layer adds Engine's `Network` and `Network.Builder` (`RiverNetwork` and `RiverNetworkBuilder` in
`river.rs`): a river carver with its lakes, wetlands and child networks. `carve` warps the coordinates with the
carver's warp (where it applies along the river), carves the river, then applies the wetlands, the lakes (both at the warped coordinates plus the lake
offset) and the children (at the warped coordinates, so each branch warps again). `build` gives the root the bounds of
every river, lake and wetland in the tree and each child `Boundsf.NONE`, as Engine does. The builder's overlap test
(against a branch's own parent, or any other network) and the network's are ported as is; the builder's own
`addBounds` box is kept although Engine never reads it. Only the river maps use river networks; Engine's river generators that
build both come in the layers below. Its tests fold every cell field over 532575 carves of fresh and filled cells at a grid of points
(plus -0, NaN and infinite ones) with three lake offsets (one NaN), for three networks (warped and unwarped carvers,
overlapping lakes and wetlands, branches nested three deep, NaN and infinite rivers, lakes and wetlands) and three
`Levels` (TerraForged's swapped `Levels(63, 255)` among them), plus every builder and network overlap test and the
bounds, against a Java oracle running Engine's classes, `NetworkOracle.java` in `.local/terraforged/oracles/river/`.

The rivermap layer adds Engine's `Rivermap` (`river.rs`): one continent's river networks, its `GenWarp` and the
continent cell (`x`, `z`) it belongs to. `apply` warps a point with the river warp, takes the lake warp's offset at the
warped point and carves, in order, every network whose bounds contain the warped point. The timestamp is
`System.currentTimeMillis()` at creation, as Engine's river cache (below) reads it; Engine's static `Rivermap.get`
comes with the rivermap-reuse layer below. Only the base, fancy and simple river
generators below build river maps; Engine's `Heightmap` (below) reads them, 6dd607eb's world generation doesn't. Its tests fold every cell field over 304380 applies of fresh and filled cells at a grid of
points (plus -0, NaN, infinite and huge ones) for `NetworkOracle`'s networks, four `GenWarp`s (one whose seeds wrap,
and `GenWarp.EMPTY`) and three `Levels`, plus direct samples, against a Java oracle running Engine's class,
`RivermapOracle.java` in `.local/terraforged/oracles/river/`.

The base-river-generator layer adds Engine's `BaseRiverGenerator` (`river.rs`): the main-river, fork, lake and wetland
configs built from the world's river settings (seed `root + seedOffset`, wrapping like Java's `int`) and the placement
its subclasses share. `generate_rivers` seeds a `java.util.Random` with `id + seed` and a `GenWarp` with `(int) id`,
takes the subclass's roots (`generateRoots`; Engine's base class has none, so the caller passes them), shuffles them
like `Collections.shuffle`, adds forks (up to three levels deep, each tested for overlaps against every root's tree as
built so far, with Engine's parent rule, and rejected where either end's continent edge value is below the inland
control point; a NaN edge value passes, as in Engine), a lake per branch with the lake chance, then wetlands, all in
Engine's draw order. `create_main`/`create_fork` are Engine's static `create`/`createFork`, and
`generate_additional_lakes`, which Engine never calls, is ported as is (every lake at angle 0). The simple and fancy
generators' roots come with their own layers. Its tests fold every built network (rivers, carver ranges, configs,
warps, lakes, wetlands, children, bounds) and 499392 applies of the resulting river maps on a grid for four continents
(all land, a radial falloff, NaN and all ocean), four settings variants (a wrapping seed offset, chance-1 lakes and
wetlands, a 384/62 world) and four ids, plus the additional lakes, overlap tests and direct samples, against a Java
oracle running Engine's class through a subclass with radial roots, `BaseRiverGeneratorOracle.java` in
`.local/terraforged/oracles/river/`.

The fancy-river layer adds Engine's `FancyRiverGenerator` (`fancy.rs`), the river generator of the fancy continents.
It seeds the random and the `GenWarp` like the base class but neither shuffles nor forks afterwards: per island, in
order, it projects main rivers across each segment (fewer on the end segments and on later islands) and out of both
endpoints in evenly spaced directions, extends each in steps of 0.075 radii while the point stays on the same island
(`getValueId`) until the continent value falls below 0.1, and skips those that overlap a network (root or fork)
already placed on that island. Each root is added before its forks are generated, so they are tested against it with Engine's parent rule,
then given its wetlands, all in Engine's draw order. Engine's `FancyContinent` builds the generator and its river
cache asks it for maps; here it borrows the continent generator, and the river-cache layer below asks it for maps. Its tests fold
every built network and 151290 applies of the resulting river maps on a grid for five settings variants (a wrapping
seed offset with chance-1 lakes and wetlands, a 384/62 world, continent scales 1000 and 777), two continent seeds and
three ids, plus the first roots and direct samples, against a Java oracle running Engine's classes,
`FancyRiverGeneratorOracle.java` in `.local/terraforged/oracles/river/`.

The river-cache layer adds Engine's `RiverCache` and `LegacyRiverCache` (`river.rs`) and the fancy continent's
`getRivermap` (`fancy.rs`). A cache keeps one river map per continent cell, generated on first use with the id
`PosUtil.pack(x, z)` (`LegacyRiverCache`, the simple continent's: `NoiseUtil.seed(x, z)`, z in the high half) and
shared as an `Arc`. Like Engine's `StampedLongMap`, hits take a read lock and a miss generates under the write lock,
so each map is made once. Maps expire once they are more than five minutes old; Engine sweeps them from a scheduler
thread every minute, the port sweeps on a miss at most once a minute (a regenerated map is the same, so this changes
nothing but memory). `FancyContinentGenerator` now takes what Engine's `FancyContinent` builds its river generator
from: the context's seed root, river settings, levels and the world's terrain registry. The simple and advanced
continents' caches come with the continent-river-cache layer below; Engine's `Heightmap` river stage (below) calls them. Its tests check the ids, the single
generation and the cached instance for negative and extreme cells, and fold the fancy continent's river maps for five
settings variants and six cells, against a Java oracle running Engine's classes, `RiverCacheOracle.java` in
`.local/terraforged/oracles/river/`, plus the expiry.

The simple-river layer adds Engine's `SimpleRiverGenerator` (`river.rs`), the river generator of the simple
(multi and single) and advanced continents, which now implement the edge and ocean-distance lookups it reads. It
spreads the world's river count of main rivers around the continent centre at evenly spaced angles (from one
random start, each with a random offset of up to 3/8 of the spacing either way), each from a random 5% to 50% of
the distance to the ocean (at least 400 blocks) out to the ocean, with both ends truncated to whole blocks, gives
each a valley, warp and lake in Engine's draw order, then hands them to the base generator's shuffle, forks and
wetlands. As in Engine, the start distance is `Math.max(400, ...)` (a NaN distance to the ocean stays NaN). One
difference: Engine throws for a negative river count (`new ArrayList(count)`); the port builds no roots. The
continents' river caches below call it. Its tests fold every built network and 345960
applies of the resulting river maps on a grid for the multi, single and advanced continents, five settings variants
(a wrapping seed offset with chance-1 lakes and wetlands, a 384/62 world, continent scale 1000, one to eight rivers),
two continent seeds and four cells each, plus the networks of extreme and negative cells and direct samples, against
a Java oracle running Engine's classes, `SimpleRiverGeneratorOracle.java` in `.local/terraforged/oracles/river/`.

The continent-river-cache layer wires `getRivermap` into the simple and advanced continents (`continent.rs`,
`advanced_continent.rs`): the multi and single continents keep a `LegacyRiverCache` (ids `NoiseUtil.seed(x, z)`),
the advanced continent a `RiverCache` (ids `PosUtil.pack(x, z)`), each over the continent's own
`SimpleRiverGenerator`. Like `FancyContinentGenerator`, their constructors now take what Engine builds that
generator from: the context's seed root, river settings, levels and the world's terrain registry. The heightmap-river layer
below calls them. Its tests fold the river maps of the three continents through their caches for five settings variants
and nine cells each (six fixed cells, among them extreme and negative ones, plus the centres nearest three points),
checking that a second lookup returns the cached map, against `RiverCacheOracle.java` in
`.local/terraforged/oracles/river/`, which calls Engine's `getRivermap`; the simple-river layer's extreme-cell test
now also goes through the caches.

The heightmap-river layer adds the river stage of Engine's `Heightmap` (`heightmap.rs`, `HeightmapRivers`):
`apply` is `applyRivers(cell, x, z, rivermap)`, which applies the river map to the cell and then turns a volcano pipe
back into a volcano when its value is below the water level or its river mask below 0.85
(`VolcanoPopulator.modifyVolcanoType`); `apply_continent` is the private `applyRivers(cell, x, z)` that
`Heightmap.apply` runs, which takes the river map of the cell's continent cell (`Continent.getRivermap(cell)`). `Heightmap`
(below) runs it. Its tests check the pipe demotion at the 0.85
boundary and with NaN values and masks, and fold every cell field over 522060 applies through the multi, single,
advanced and fancy continents' river caches (five settings variants each, a block grid plus NaN, infinite and -0
points, fresh cells and six fills of value, river mask and terrain), against `HeightmapRiversOracle.java` in
`.local/terraforged/oracles/heightmap/`, which calls Engine's `Heightmap.applyRivers`.

The heightmap-cache layer adds Engine's `HeightmapCache.get` (`heightmap.rs`, `HeightmapCache`): it keeps up to 4096
cells keyed by packed block coordinates and, on a miss, runs the heightmap on a new cell and turns a coast whose value
is above the water level and at most five blocks over it into a beach (the correction `WorldLookup` also makes). `get`
takes `Heightmap.apply` as a closure; it returns a copy of the cached cell where Engine returns
the shared object. A full cache drops its oldest cell, and misses are computed outside the cache's lock (Engine splits
it into sections and computes under a section's lock); neither changes the cells returned. Engine's `generate` (the
tile path) is the contextual layer's, below. Its tests check hits, eviction, sharing between threads and the coast
identity check, and fold every returned cell field over 7932 lookups (a deterministic fill of values around the water and beach levels,
four terrains, three height / sea level variants, extreme blocks, caches of 4096 and 16 cells) against
`HeightmapCacheOracle.java` in `.local/terraforged/oracles/heightmap/`, which calls Engine's `HeightmapCache.get` on a
`Heightmap` whose `apply` is that fill.

The rivermap-reuse layer adds Engine's static `Rivermap.get` (`river.rs`, `Rivermap::get_or_lookup` and
`Rivermap::get_for_cell`, which reads the cell's continent cell): it returns the current river map when it belongs to
the requested continent cell (both `x` and `z` equal) and otherwise asks for the map of that cell. As `Heightmap` is
not ported, the lookup Engine makes (`heightmap.getContinent().getRivermap(x, z)`) is a closure, which callers give a
continent's `rivermap`. Engine's tile and chunk tasks and `HeightmapCache.generate` call it; the cache's `generate` is
ported (below), the tasks are not. Its test checks that a matching map is returned without a lookup and that a
different `x`, a different `z`, swapped coordinates or no current map run exactly one lookup for the requested cell.

The contextual heightmap-cache layer adds Engine's `HeightmapCache.generate(cell, x, z, rivermap)` (`heightmap.rs`,
`HeightmapCache::generate`), the path `Tile.generate(HeightmapCache, ...)` takes for each block: on a miss it runs
the heightmap's base, river and climate stages (closures, e.g. `Heightmap`'s) on the caller's cell in that
order, picking the river map with `Rivermap::get_for_cell` (reusing the caller's current map when it belongs to the
cell's continent cell), caches the cell and returns the map it used; on a hit it copies the cached cell into the
caller's (`copyFrom`, which leaves `terrain_region_center` alone) and returns the caller's map unchanged. It shares
the cells of `get`, but makes no beach correction. Engine caches the caller's `Cell` object itself, so later changes
to that tile cell reach the cache; this caches a snapshot taken after the climate stage. The cached tile fill
(`generate_cached_heightmap`, below) calls it. Its tests check that a hit runs no stage, and fold 600 `generate` and
`get` calls over 120 blocks (every returned cell field, the order the stages run in, the returned map and whether it
is the caller's, the lookups) against `HeightmapGenerateOracle.java` in `.local/terraforged/oracles/heightmap/`, which calls Engine's
`HeightmapCache.generate` and `get` on a `Heightmap` whose stages are the same fills; the caller cells carry markers
that show what a miss keeps and what a hit's `copyFrom` overwrites.

The world-erosion layer adds Engine's `WorldErosion` (`filters.rs`, `WorldErosion`), the thread-safe holder
`WorldFilters` keeps its `Erosion` filter in: `get(ctx)` returns the cached value while a validator accepts it for
`ctx` (`WorldFilters` passes the tile map's total size and checks the filter's size), and otherwise rebuilds it with a
factory under a write lock, checking again first so that threads asking at once share one build. `WorldFilters` uses it
for its erosion filter. Its tests check reuse and rebuilds, one build across threads,
and replay a transcript of the validator and factory calls from `WorldErosionOracle.java` in
`.local/terraforged/oracles/world_erosion/`, which calls Engine's `WorldErosion.get`.

The smoothing layer adds Engine's `Smoothing` filter (`filters.rs`, `Smoothing`, with `SmoothingSettings` for
`FilterSettings.Smoothing` and `Filterable` for the tile map it runs on). Each pass moves every cell's height, in place,
towards the weighted mean of the present cells within the smoothing radius (weight `1 - d² / r²`), skipping the map's
outer `round(r + 0.5)` cells and cells under the erosion mask. The change is at full strength for cells up to one block
above sea level and fades out linearly by 120 blocks above it; it is also weakened near terrain-region edges of
terrains whose erosion modifier is not 1 (badlands and its composites) and near rivers. Engine's settings UI limits the radius to 0..5; a radius below -1, which
only a hand-edited config can give, leaves the map unchanged as in Engine. `WorldFilters` runs it on a tile's
filter region. Its tests replay `SmoothingOracle.java` in `.local/terraforged/oracles/smoothing/`,
which runs Engine's `Smoothing.apply` on the same 14x14 maps for 11 radius, rate, level and iteration cases and
prints every cell's value bits folded together.

The steepness layer adds Engine's `Steepness` filter (`filters.rs`, `Steepness`), which `WorldFilters` builds with
radius 1 and scaler 10. It sets each cell's gradient to `min(1, scaler * Σ |height - max(neighbour, sea level)| /
radius)` over the 15 cells at `(dx, dz) * radius` for `dx` and `dz` from -1 to 2 (Engine's neighbourhood is
asymmetric), skipping absent cells and those outside the map; like Engine's `Tile.getCellRaw`, a neighbour past the
left or right edge is read from the previous or next row while it stays inside the backing array. Radius 0 divides by
zero as in Engine. `WorldFilters` runs it on a tile's filter region. Its tests replay
`SteepnessOracle.java` in `.local/terraforged/oracles/steepness/`, which runs Engine's `Steepness.apply` on the
smoothing oracle's 14x14 maps for 14 radius, scaler, level and NaN/signed-zero height cases and prints every cell's
gradient bits folded together.

The beach-detection layer adds Engine's `BeachDetect` filter (`filters.rs`, `BeachDetect`). It turns a coast cell
(coast, beach, or a terrain delegating to one) whose continent edge is below the beach control point into the world
registry's own beach terrain when its squared height gradient is below 0.275, the gradient being the height
differences to the cells 8 blocks east and west and 8 blocks north and south, each divided by 17. A neighbour that is
absent or outside the map is replaced by the cell itself and takes 8 off that distance; neighbours are read like
Engine's `Tile.getCellRaw`, as for steepness. Invalid control points fall back to the defaults, as Engine's
`ControlPoints` does. `WorldFilters` runs it on a tile's filter region. Its tests replay
`BeachDetectOracle.java` in `.local/terraforged/oracles/beach/`, which runs Engine's `BeachDetect.apply` on hashed
8x8 to 33x33 maps (absent cells, NaN, infinite and signed-zero heights, NaN continent edges, valid and invalid beach
control points) and prints every cell's terrain id folded together with the cells it turned into beach.

The erosion-primitives layer adds the parts Engine's `Erosion` filter is built from (`filters.rs`): its settings
(`ErosionSettings`, Engine's `FilterSettings.Erosion`: 135 droplets per chunk, lifetime 12, droplet volume and velocity
0.7, erosion and deposit rates 0.5), the `FastRandom` stream it reseeds per chunk (`seed(chunkSeed, iterationSeed)`,
which mixes the second value into the gamma, then `nextInt(bound)`), and its droplet brushes (`ErosionBrushes`,
`Erosion.initBrushes`): for each cell, the cells within radius 4 and their normalized `1 - d / 4` weights. As in Engine,
only cells near the map's edge (with an asymmetric extra column on the left) compute their own brush, clipped to the
map; every other cell reuses the last computed brush, re-centred. Its tests replay `ErosionPrimitivesOracle.java` in
`.local/terraforged/oracles/erosion_primitives/`, which draws from Engine's `FastRandom` for 13 bounds (including 0,
negative bounds and ones that make it reject draws) over 27 seeds, and runs `initBrushes` for map sizes 0 to 40 and
radii -1 to 6, printing the draws and every brush's indices and weight bits folded together.

The hydraulic-erosion layer adds the filter itself (`Erosion`, `filters.rs`): for each iteration and each whole chunk
of the map, one droplet starts at a random block of the chunk (`FastRandom` seeded with `NoiseUtil.seed` of the chunk's
world position, borders included, and of the filter seed and the iteration, so neighbouring tiles agree) and runs
downhill for up to `droplet_lifetime` steps: bilinear height and gradient, inertia 0.05, sediment capacity
`max(-Δh · speed · water · 4, 0.01)`, bilinear deposits and radius-4 brush erosion, evaporation 1 % and gravity 3, with
Engine's `(float) Math.sqrt` in double and its NaN guards. Every change goes through Engine's filter `Modifier`, now
ported in both forms: `Modifier::range(min, max)` (erosion uses `range(ground, ground(15))`, seed root + 12768) and
`.invert()` (smoothing). Its test replays `ErosionOracle.java` in `.local/terraforged/oracles/erosion/`, which runs
Engine's `Erosion.apply` on 16 maps (several seeds, sizes from 0 to 64 with and without borders, negative and extreme
block positions, custom settings, zero and negative iterations, NaN and infinite heights) and folds every cell's
value, erosion and sediment bits.

The world-filters layer adds Engine's filter chain (`WorldFilters`, `filters.rs`, with `FilterSettings`), built from
the world's root seed, filter settings, levels, control points and terrain registry as Engine builds it from its
`GeneratorContext`. `apply(map, region_x, region_z, optional)` runs, when `optional` is set, erosion (built for the
map's total size and kept in a `WorldErosion` until the size changes; `erosion.droplets_per_chunk` iterations) then
smoothing (`smoothing.iterations` passes), and always steepness (radius 1, scaler 10) then beach detection;
`apply_tile` (the tile layer, below) runs it on a tile, as the tile generator (below) does after each fill. Its test replays `WorldFiltersOracle.java` in `.local/terraforged/oracles/world_filters/`, which builds Engine's
`WorldFilters` from a `GeneratorContext` for four settings groups (long seeds, other levels, droplet counts and smoothing passes including 0, radii, rates, beach
points, lifetimes) and runs each over several maps in turn (borders, negative positions, the optional filters on and
off, cache reuse and rebuilds, NaN and infinite heights), folding every cell's value, erosion, sediment, gradient and
terrain.

The tile-size layer adds Engine's `Size` (`tile.rs`), the first part of its tile map: a tile's edge length and border
(`chunks(factor, border)` for `1 << factor` chunks, `blocks` for the same tile in blocks), the total edge and array size,
`mask`, `indexOf` and the chunk/block conversions, all in Java `int` math (wrapping sums and products, shift counts
modulo 32, arithmetic `>>`). Its test replays `SizeOracle.java` in `.local/terraforged/oracles/size/` over negative,
overflowing and `i32::MIN`/`MAX` sizes, borders, coordinates and shift factors.

The tile layer adds Engine's `Tile` (`tile.rs`) without its chunk views and generation: a region's chunk and block
position (`region << size`, then `<< 4`), its region id (`PosUtil.pack`), and its bordered block backing, every cell
built from the world's registry. `cell(x, z)` is Engine's `getCell` (masked into the tile, past the border) and
`raw_cell(x, z)` its `getRawCell` (`None` where Java throws). `filterable()` is Engine's `FilterRegion`, the
`Filterable` the filters read: its block position, block size and backing, with `getCellRaw`'s index in Java `int`
math and `Cell.empty()` outside the backing. `WorldFilters::apply_tile(tile, optional)` is Engine's `apply(tile,
optionalFilters)`: the filters over that region, seeded with the tile's region coordinates. The tile generator (below)
builds tiles. Its tests replay `TileOracle.java` in `.local/terraforged/oracles/tile/` over
negative and wrapping region positions, shift factors Java masks to five bits, missing and negative borders, and
negative, border and overflowing coordinates, and run Engine's `WorldFilters.apply` on its own tiles (hashed cells,
borders and the optional filters on and off) against `apply_tile`.

The tile-chunk layer adds the tile's chunk array and its chunk views. `chunk_writer(x, z)` is Engine's `getChunkWriter`:
the `GenChunk` at that tile-local chunk position (border included), created there if the slot is empty; as in Engine a
slot keeps its first chunk, so a position past the tile's edge that aliases a filled slot returns that slot's chunk,
and an index outside the array gives `None` where Java throws. `chunk(x, z)` is `getChunk`: the chunk at a world chunk
position masked into the tile past the border, `None` where Engine throws a `NullPointerException` because no writer
has created it yet. Each chunk knows its world chunk and block position (`tile chunk + local chunk - border`) and
reads its 16x16 cells from the tile backing at `local chunk << 4` plus the block position `& 15`. `ChunkReader` ports
`getCell`, `iterate` and `visit`, with Engine's bugs (`visit` returns early when `maxZ` is past the chunk and runs `z`
up to the clamped `maxX` and `x` up to the clamped `maxZ`); `ChunkWriter` ports `genCell` and `generate`, which also
passes each cell's world block position. These readers borrow the tile and do not count as open (the tile cache's
cached chunks do, see the tile-disposal layer). Its tests
replay the `k` lines of `TileOracle.java` on the same tiles: readers before any chunk exists, writers at negative,
edge, aliasing and out-of-range positions, Engine's `generate(Consumer)` loop, readers over negative, border and
overflowing chunk positions, and `visit` over rectangles around chunks.

The tile-heightmap layer adds `generate_heightmap`, Engine's `Tile.generate(Heightmap)`, the fill
`TileGenerator.generateRegion` runs before the filters. For each tile-local chunk (z rows, then x, border included) it
takes the chunk of that slot as `chunk_writer` does (a slot already holding a chunk keeps it, and that chunk's cells are
filled), and for each of its cells (z rows, then x) runs the base stage, picks the river map of the cell's continent
cell (the previous cell's map when it matches, carried from the tile's first cell to its last, else a lookup:
`Rivermap::get_for_cell`), runs the river stage (`HeightmapRivers::apply`) and the climate stage, each with the cell's
world block position as floats. The stages and the lookup are closures (`Heightmap`'s, below); the
tile generator (below) runs the filters (`WorldFilters::apply_tile`) after the fill. Its tests
replay `TileHeightmapOracle.java` in `.local/terraforged/oracles/tile_heightmap/` on six tiles (negative, wrapping and
borderless ones, and one whose slots hold aliasing chunks made before the fill) with stub stages: the stage trace with
every coordinate and lookup, every cell of the backing and every chunk read afterwards.

`generate_cached_heightmap` is Engine's `Tile.generate(HeightmapCache)`: the same chunk and cell order, with each cell
going through the heightmap cache's tile path (`HeightmapCache::generate`) at its integer world block position, the
cache key. A cached block is copied into the cell without running any stage and keeps the river map carried so far; a
miss runs the base, river-map, river and climate stages on the cell and caches a copy. The same oracle runs the six
tiles through one Engine `HeightmapCache` (overlapping tiles, so hits and misses mix, and the river map carried across
hits), then tiles 0-4 again in fresh tiles, where every block hits. Engine's cache holds the tile's own cell objects, so
a cell refilled later (by a slot holding an aliasing chunk) also changes the block it was first cached for; the port
caches snapshots, so such a block reads its first values. The oracle leaves out the repeat that would show this, and
its cache is large enough that nothing is evicted (Engine evicts per map section, and the sections follow the processor
count).

`generate_zoomed_heightmap` is Engine's `Tile.generate(Heightmap, offsetX, offsetZ, zoom)`: the same fill as
`generate_heightmap`, but the stages get each cell's world block position scaled by the zoom and shifted by the offset
less half the zoomed tile width (`block * zoom + offset - size * zoom / 2`, in Engine's `float` order). The tile
generator's zoomed region (below) runs this fill. Its tests replay the same oracle's zoomed runs: thirteen offset and
zoom pairs over the six tiles, with fractional, unit, large, negative, signed-zero, NaN and overflowing zooms and NaN
and infinite offsets, compared bit for bit (NaNs folded as Java's `floatToIntBits`).

`generate_cached_zoomed_heightmap` is Engine's `Tile.generate(HeightmapCache, offsetX, offsetZ, zoom)`: the zoomed
positions, floored with Engine's `NoiseUtil.floor` (`(int) f`, minus 1 below zero: a NaN sample gives -1, infinite
and huge samples saturate or wrap to `Integer.MAX_VALUE`, and a negative whole sample such as -16.0 gives -17), go
through `HeightmapCache::generate` as the cached fill's block positions do. The stages get that floored position, as
Engine's cache computes a cell from its key, and zoomed blocks that floor to the same position share one cached cell,
also with the unzoomed cached fills on the same cache. The oracle runs sixteen offset and zoom pairs on the cache of
the cached runs: unit, halved and doubled zooms over tile 0's cached blocks, the zoomed runs' pairs except the one on
the aliasing tile (with tile 1's unit pair shifted off the blocks whose cached cells that tile's aliasing chunks
refilled, the snapshot difference above), and a repeat where every block hits; debug and release builds agree.

`Tile::iterate` and `Tile::generate` are Engine's tile visitors, `Tile.iterate(Cell.Visitor)` and
`Tile.generate(Cell.Visitor)`: every non-border cell, z rows then x, with its position inside the tile (0 to the block
size, the border left out); `generate` hands the cells out mutably. With a negative border the indices are Engine's
(Java `int` math): where one leaves the backing the port panics, as Engine throws, and a zero block size visits nothing. `Tile::batch_size` is Engine's
`Tile.getBatchSize`, the chunk count per side divided by the batch count and rounded up (for positive counts) in Java
`int` math, which the batched tile generation (#82) uses. `TileOracle.java`'s `v` and `b` lines check both on the same
tiles and on zero, negative and overflowing sizes and counts; debug and release builds agree.

The tile-generator layer adds Engine's plain, synchronous `TileGenerator` (`tile_generator.rs`; what
`TileGenerator.builder().build()` returns without a batch size) and its `WorldGenerator` (the heightmap and the filters).
The generator takes any `HeightmapSource` (`Heightmap` implements it) (its base, river-map, river and climate
stages), and the filters are any `TileFilters` (`WorldFilters` runs `apply_tile`). The generator holds its tiles' size
factor and border, the world's registry the cells start from (in place of Engine's resource pools) and the builder's
batch size. `chunk_to_region` is `chunkToRegion` (`chunk >> factor`, the shift count modulo 32), `create_empty_region`
is `createEmptyRegion`, and `generate_region(region_x, region_z)` is `generateRegion`: an empty tile filled whole by
`generate_heightmap`, then the filters with the optional ones, so erosion, smoothing, steepness and beach detection see
every cell, border included, only after the fill. `generate_zoomed_region(center_x, center_z, zoom, filter)` is the
zoomed `generateRegion`: region (0, 0) filled by the uncached `generate_zoomed_heightmap` with the center as offset,
then the filters, the optional ones only when `filter` is set. Lazy tiles and the batched and striped generators are the
next layers, then the builder, the tile cache, tile disposal and the async tile factory.
Its test replays `TileGeneratorOracle.java` in `.local/terraforged/oracles/tile_generator/`: Engine's `generateRegion`
with a `WorldGeneratorFactory` over a stub heightmap that hashes heights, continent edges, terrains and erosion and
river masks, for two filter settings groups and six sizes and regions (borderless, wide-border, negative and wrapping),
five zoomed regions on each (fractional, unit, negative, NaN and huge centers and zooms, filters on and off) and
`chunkToRegion` for shift factors Java masks, folding the stage trace and every cell; debug and release builds agree.

The lazy-tile layer adds Engine's `LazyCallable` (`concurrent.rs`, module `terraforged_concurrent`) and the generator's
lazy handles: `get_tile(region_x, region_z)` is `getTile` (a `CallableTile`) and `get_zoomed_tile(center_x, center_z,
zoom, filter)` the zoomed `getTile` (a `CallableZoomTile`). Nothing runs until the first `get` (or `call`), which runs
`generate_region` or `generate_zoomed_region`; every later caller, on any thread, gets that same tile. As with Engine's
`StampedLock`, the value is created under an exclusive lock: callers arriving meanwhile wait instead of generating it
again, and `is_done` also waits, then reports whether a tile was stored. A generation that panics (Engine: throws)
stores nothing, so the next caller, or one already waiting, generates it again. `cancel` and `is_cancelled` always
return `false`. Dropping a handle drops its tile; `LazyCallable` disposes of nothing in Engine either. Its adapters
`adapt` and `adaptComplete`, the timed `get` and `LazySupplier` are not ported; `callAsync` and `then` are
`CacheEntry::compute_async` and `CacheEntry::then` (see the async layers below). Its tests replay
`LazyCallableOracle.java` in `.local/terraforged/oracles/lazy_callable/` (one creation for 16 concurrent callers, all
seeing the same value; retry after a throwing creation; `isDone` and `get` waiting on another thread's creation and
seeing its outcome) and the tile-generator oracle's `l`/`m` lines (each handle's fold equals the eager generation, no
stage runs before the first `get` or on a second one); another test has eight threads share one lazy tile, generated
once. Debug and release builds agree.

The batched-tile layer adds Engine's `TileGeneratorBatched` and `TileGeneratorStriped` as `generate_batched_region` /
`generate_batched_zoomed_region` and `generate_striped_region` / `generate_striped_zoomed_region`, which take the batch
count Engine's builder passes (`batch(n)`). `Tile::generate_area` is `Tile.generateArea`: the tile-local chunks cut into
`n * n` squares of `Tile::batch_size` chunks a side (`d * size` in Java `int` math, clipped to the chunk count), and
`Tile::generate_area_striped` is `generateAreaStriped`: each chunk row cut into `n` stripes. Each batch is an Engine
`ChunkBatchTask` (or `ChunkStripeBatchTask`): its chunks are taken on the calling thread with copies of their cells, up to
`max(2, processors)` scoped threads (Engine's default pool; a generator from the builder below uses its pool) take the batches in order and run `driveOne` on each chunk
without holding a lock, and the cells go back to the tile once all are done; then the filters run on the whole tile as in
`generateRegion`. Like Engine's tasks, each chunk starts without a river map, so batched runs look river maps up more
often than the plain fill, but every cell is the same for any batch count and thread schedule. A stage's panic is
re-raised once the other threads finish (Engine's synchronous batcher rethrows; its thread-pool batcher logs the error
and returns the tile with that batch unfilled). Engine's builder only makes these generators for counts of 1 and more;
called with 0 they panic as Engine throws `ArithmeticException`, and with a negative count, which fills no cell, they
panic where Engine's filters throw `NullPointerException` on the never-created cells. Its tests replay the
tile-generator oracle's `b`/`y` lines (Engine's real batched and striped generators on a four-thread `ThreadPools` pool,
batch counts 1, 2, 3, 7 and 100, zoomed for 3 and 100, folding the lookups, an order-free sum of the stage calls and every
cell; the cells equal the `g`/`z` lines') and `e` lines (counts 0 and -2), and check that one thread or many give the
same cells and that a panic keeps its payload. Debug and release builds agree.

The builder layer adds Engine's `TileGenerator.Builder` (`TileGenerator::builder()`): size factor 0, border 0, batch size
0, not striped and no thread pool until `size`/`factor`/`border`, `batch`, `striped` and `pool` set them (the last call
wins), and `factory(registry, generator)` for Engine's `WorldGeneratorFactory`. `build()` makes Engine's
`TileGeneratorStriped` for a positive batch size with `striped()`, `TileGeneratorBatched` for a positive batch size
without it, and the plain `TileGenerator` otherwise (a count of 1 is still batched, and `striped()` or a pool alone
change nothing). `generate_region`, `generate_zoomed_region` and the lazy `get_tile` handles then run that class's
`generateRegion`, which needs a `Sync` heightmap. `pool(size)` stands for `ThreadPools.create(size)`, whose batches run on
`max(2, size)` threads; without a pool, as with Engine's `ThreadPools.NONE`, every batch runs on the calling thread in
order. Like Engine's, the builder checks no argument; `build()` panics without a factory, where Engine throws
`NullPointerException`. `TileGenerator::new` stays the plain generator. Its test replays the tile-generator oracle's
`u`/`w` lines (Engine's real builder for batch counts unset, 0, -3, 1 and 4, striped or not, with and without a pool:
the class, `generateRegion`, the lazy tile and the zoomed `generateRegion`, with Engine's stage trace when no pool runs
them) and `d`/`n` lines (defaults, overriding calls, no factory); debug and release builds agree.

The cache-entry layer adds Engine's `ExpiringEntry` and `CacheEntry` (`concurrent.rs`), the entries the tile cache (a
later layer) holds. A `CacheEntry` is a `LazyCallable` over a task that runs on the first `get` or `call`, on the
calling thread (the async-entry layer below adds `computeAsync` and `then`), plus the time of its last access:
`System.currentTimeMillis()` (river.rs's `current_time_millis`, an `i64` like Java's `long`) at construction and at the
start of every `get`, before the task runs; `call` and `is_done` leave it, as in Engine, which overrides only `get`.
`is_done` is Engine's `task.isDone()`: it answers at once (it does not wait for another thread's task) and is `true` once
the task ended, even by panicking. As with a failed Engine `Future`, the task never runs twice: after it panicked, every
`get` and `call` panics again. `cancel` and `is_cancelled` return `false`. Engine's `close` closes a closeable value (a
`Tile` hands its cells back to resource pools that are not ported); here it does nothing and the value is dropped with
the entry. When an entry expires is Engine's `Cache`'s rule (the next layer). Its test replays `CacheEntryOracle.java`
in `.local/terraforged/oracles/cache_entry/` (flags before and after `get`, where the timestamp is set, a throwing task, 16
concurrent getters with one run, `isDone` during another thread's run) with an injected clock in place of the oracle's
sleeps; debug and release builds agree.

The expiring-cache layer adds Engine's `Cache` and the `StampedBoundLongMap` behind it (`BoundedLongMap`, both in
`concurrent.rs`). Entries are kept by `long` key in insertion order under one read-write lock; a `compute_if_absent` miss
on a full map first drops the oldest insertion (without closing it), then runs the factory under the write lock, as Engine
does. The sweep is Engine's `run()`: `timeout = now - lifetime` in wrapping Java `long` arithmetic, then every entry whose
timestamp is strictly before it is removed, also without closing it, entries whose task is still running included (their
holders keep them; the next lookup makes a new one). Only `remove` closes the removed entry. Engine's constructor
schedules `run()` every `interval` milliseconds on its `CacheManager` scheduler thread, which is not ported: here a
`compute_if_absent` or `map` runs the sweep first when the clock is an interval or more from the last sweep, so a cache
nobody computes in is not swept (its capacity still bounds it); a swept entry is recomputed on its next lookup, so the
values handed out are the same. As in Engine, a non-positive interval is rejected (a panic where Engine throws) and a map
of capacity 0 panics on its first miss. `Rivermap` is an `ExpiringEntry` by its creation time; river.rs's `RiverCache`
keeps its own sweep (Engine's uses an unbounded `StampedLongMap`). Its test replays `CacheOracle.java` in
`.local/terraforged/oracles/cache/` (Engine's real `Cache` with the clock injected through its `timeout` field: the
sweep for lifetimes and clocks up to the `long` extremes, eviction order, `map`, `remove`, a swept entry whose task never
ran, the factory holding the lock, capacity 0 and bad intervals); debug and release builds agree.

The async-entry layer adds Engine's `CacheEntry.computeAsync` and `then` and the part of Engine's `ThreadPool` they use
(`concurrent.rs`). `ThreadPool::create(size)` is `BatchingThreadPool.of(size)`'s task executor: `max(1, size / 2)`
threads named `TF-Task-Worker-<n>`, started one per submitted task until all exist, taking tasks in submission order;
`size()` adds Engine's `max(2, size)` batch threads (those are the tile generator's own scoped threads). Unlike
`ThreadPools.create`, each call makes its own pool; when the last clone is dropped the threads finish the queued tasks and
exit. On such a pool `compute_async` starts the task at once on a task thread: `is_done` is `false` until it ends
(returning or panicking), `get` and `call` wait for it, and a failure is final, as for a failed Engine `Future`: every
later `get`/`call` panics without running the task again. On `ThreadPool::NONE` (Engine's `EmptyThreadPool`, the tile
generator's default) the entry is done at once, the task runs on the thread of the first `get` or `call`, and a task
that panicked runs again on the next one (Engine's `adaptComplete` turns the exception into a null value that is never
stored). Either way the timestamp is the clock's at creation and at each `get` (river.rs's `current_time_millis` or an
injected clock, as for synchronous entries). `then(pool, f)` is Engine's `computeAsync(() -> f.apply(this.get()), pool)`:
the new entry's task gets the parent on the pool's thread, stamping the parent's access time, and fails for good if the
parent failed or `f` panicked. Engine 0.3.0's `CacheEntry` has no `map` or `supplyAsync`; neither is added. Expiring
`Cache`s hold async entries like the others: a sweep may drop an entry whose task still runs, and its holder still gets
the value. Its test replays the async lines of `CacheEntryOracle.java` (`.local/terraforged/oracles/cache_entry/`:
`isDone` while the task runs, `get` blocking, the task's thread, two tasks at once on `ThreadPools.create(4)`,
timestamps, failures, `then` chains and failing parents, `ThreadPools.NONE`, pool sizes) with an injected clock in place
of the oracle's sleeps. As with `FutureTask.set`, a task thread marks the entry done before it hands over the value or
the failure, so a `get` or `call` that has returned (or panicked) always finds `is_done` true, and in a `then` chain each
function sees its parent done. A second test replays `CacheEntryOrderOracle.java` (same folder, `run-order.sh`): a gated
three-entry chain on pools of 4 and 2, 3,000 concurrent reads of succeeding and failing tasks, and a chain on
`ThreadPools.NONE`.

The tile-cache layer adds Engine's `TileCache` and `TileProvider`'s lookups (`tile_generator.rs`). `cached()` on a
shared (`Arc`) generator makes the cache on the generator's pool: the builder's `pool(size)` (`ThreadPool::create`), else
`ThreadPool::NONE`, as `Builder.getOrDefaultPool`. It is an expiring `Cache` named `TileCache` (256 entries, a 60 second
lifetime, swept every 20 seconds) of async `CacheEntry`s keyed by the packed region id. `get_or_compute` makes a missing
region's entry, whose task is the generator's `generate_region` (`CallableTile`), on the pool, without waiting;
`get_tile` (`getOrCompute(id).get()`) stamps the entry and waits for, or on `NONE` runs, its tile, and returns the entry
(read the tile with `call()`). Concurrent requests for one region share one entry, so its tile is generated once.
`get_tile_if_present` returns an entry only once it is done, stamping it; on `NONE` an entry is done at once, so this
runs a cached region nobody has got yet, as Engine does. `get` neither stamps nor generates. `queue_region` and
`queue_chunk` start tiles only on pools larger than four (`QUEUING_MIN_POOL_SIZE`; `ThreadPool::create(4)` is 6). The
cache has no zoomed path. A swept tile is dropped, not closed (as Engine's sweep); Engine's `Tile.close`, which hands a
tile's cells back to `TileResources` pools, is not ported, and the cells are freed with the last holder of the entry. Its test replays `TileCacheOracle.java` in
`.local/terraforged/oracles/tile_cache/` (Engine's real `TileCache` from `TileGenerator.cached()` over
`TileGeneratorOracle`'s stub heightmap, entry stamps injected through Engine's private fields, here through the
cache's clock): which pools queue, region ids, tiles bit for bit equal to `generateRegion` on `NONE`, on
`ThreadPools.create(8)` and from a batched generator, hits, stamps, the sweep at exactly a lifetime and a millisecond
past it, and eight threads asking for one region; debug and release builds agree.

The cached-chunk layer adds `TileProvider.getChunk` as `TileCache::get_chunk(chunkX, chunkZ)`: `get_tile` of the
chunk's region (`chunkToRegion`, an arithmetic shift, so negative chunks round down), which stamps the entry and waits
for or runs the tile, then the tile's `chunk(chunkX, chunkZ)` (the slot `border + mask(chunk)`). It returns a
`CachedChunk` that holds the tile's cache entry, as Engine's `GenChunk` (an inner class of its `Tile`) keeps its tile:
the chunk stays readable after the cache sweeps the entry, and the next `get_chunk` makes a new tile. `reader()` is the
tile-chunk layer's `ChunkReader` (`cell`, `iterate`, `visit` with Engine's swapped bounds), and reads do not stamp the
entry. `None` stands where Engine would throw `NullPointerException` for a slot with no chunk; every tile the generator
makes fills all its slots, so it does not happen. The test replays the `h` lines of `TileCacheOracle.java`: chunks at negative, extreme and aliasing positions
(region, slot of the chunk array, position, cells, `visit` rectangles), `getChunk` over 8x8 chunks around two regions,
stamping, a reader outliving its swept entry, and eight threads asking for one chunk at once.

The tile-disposal layer adds Engine's `Disposable.Listener` (`DisposeListener`, `concurrent.rs`) and the tile's disposal
counts. `Tile::dispose` counts one disposal (Java `int`) and, once the count reaches `size * size` (the chunks without
the border: Engine's `chunkCount` field, not `getChunkCount()`), tells the tile's listener on every call, on the calling
thread with no lock held. A tile takes the listener its generator has when `createEmptyRegion` makes it
(`TileGenerator::set_listener`; none by default), zoomed tiles too, as region (0, 0). `TileCache::new` sets itself, so
a disposed tile runs `onDispose`, `cache.remove(tile.getRegionId())`: it removes whatever entry the region has at that
moment (a newer tile's too) and closes it outside the cache's lock. A cache entry closes its tile (`Tile::close`) only
once a `get` or `call` has read it (a pool's finished task that no one has read is not closed), and the tile closes
only when no reader is open: `get_chunk` opens one, and
`CachedChunk::close` closes it (up to the caller, as in Engine, whose `WorldLookup` never closes its readers);
`Tile::active` is Engine's count, -1 once closed. The sweep and eviction remove entries without closing them or telling
the listener, as in Engine. The listener is held weakly, so a dropped cache tells no one. Engine's `close` also resets
the cells and hands the arrays back to `TileResources` pools; pools are not ported, so here a closed tile keeps its
cells until the last holder drops it. The test replays `TileDisposalOracle.java` (in `.local/terraforged/oracles/tile_cache/`,
a `TileCache` subclass logging each `onDispose`): the counts, which disposal fires, for which region and tile, open
readers blocking the close, a stale tile's disposal removing the region's new entry, sweep, eviction and `remove`,
zoomed tiles, a second cache taking over the generator's listener, eight threads disposing at once, and an unread
entry on a pool that is not closed.

The async-tile-factory layer adds Engine's `TileGeneratorAsync` (`tile_generator.rs`): `TileGenerator::asynchronous`
(`async()`, on the generator's pool) or `AsyncTileGenerator::new(generator, pool)`. `get_tile` and `get_zoomed_tile`
run the generator's `generate_region` and `generate_zoomed_region` through `CacheEntry::compute_async`, as Engine's
`LazyCallable.callAsync(generator.getTile(...), threadPool)`: on a `ThreadPool::create` pool the tile starts on a task
thread at once and the call returns a pending entry (`is_done` false until the task ends; `get` waits); on
`ThreadPool::NONE` the entry is done at once and the tile is made on the first `get`'s thread. A panicking fill on a
pool fails every later `get` without running again (Engine's `FutureAdapter` throws `NullPointerException`); on `NONE`
each `get` runs it again. Every call makes its own tile: unlike the tile cache, nothing is shared. `chunk_to_region`
and `set_listener` delegate to the wrapped generator, so the tiles take the listener it has when their task runs;
`asynchronous()` on the factory returns itself, and `cached()` is a `TileCache` over the wrapped generator (not the
factory) on the factory's pool, which becomes the generator's listener. The test replays `TileAsyncOracle.java` (in
`.local/terraforged/oracles/tile_cache/`): which thread runs a tile, pending and done states (a stub stage holds the
fill until released), tiles bit for bit the synchronous generator's (plain, zoomed, batched and striped), failures on
both pools, listener delegation and capture, the factory's caches, and eight calls for one region making eight tiles.

The climate-primitives layer adds Engine's `world.climate` building blocks (`climate.rs`): `Compressor` (squeezes a
module's middle band and stretches its ends), `Temperature` (a latitude band from `NoiseUtil`'s sine table) and
`Moisture` (a one-octave simplex clamped to 0.125..0.875 and stretched, sampled at half the x frequency); `module()`
turns each into a graph node with the `Noise` default bounds 0..1. Their powers are `NoiseUtil.pow(float, int)`
(repeated float products, 1 for a negative power; noise.rs `pow_int`), not `Math.pow`, and their signs
`NoiseUtil.copySign` (noise.rs `copy_sign`, which leaves zeros and NaNs alone). The test replays `ClimateOracle.java`
(in `.local/terraforged/oracles/climate/`): compressors with edge-case insets and ranges, temperatures over nine
frequencies and eleven powers, moisture over a module and over seeds, scales and powers, each over 41 inputs from
-0.0 and subnormals to ±infinity and NaN, and a few raw samples. The climate settings, the climate module and the
`Climate` stage follow.

The biome-type layer adds the rest of Engine's `BiomeType` (`biome_type.rs`): the 256×256 type map that
`BiomeTypeLoader` builds from TerraForged's `biomes.png` (squares below the diagonal are `ALPINE`, the rest take the
type with the nearest lookup colour), stored run-length encoded, so no image is decoded; each type's temperature and
moisture ranges (`getRanges` over that map), lookup and display colours (the display colours come from
TerraForged's `biomes.txt`, which overrides every constructor default) and `mapTemperature`/`mapMoisture`. The lookups
are `getCurve` (also `get(float, float)`, here `at_climate`) and `getLinear`, with `NoiseUtil.round` and Java int
math for the map indices; like Engine, an index outside the map fails (a panic here, an
`ArrayIndexOutOfBoundsException` there): a temperature outside about -0.002..1.002 or a moisture below about -0.002
(a NaN input reads as index 0, and a moisture above 1 is capped by the temperature's index). `apply` and `applyCurve` set the cell's biome by the curve, and so does `applyLinear`, whose
Engine body is the same as `applyCurve`'s. The test replays `BiomeTypeOracle.java` (in
`.local/terraforged/oracles/biome_type/`): the whole map, every type's range bits, colours and mapped values, every
lookup and `apply` variant over all pairs of 33 edge inputs (the 0/1 rounding edges, values just outside 0..1,
subnormals, NaN, ±infinity) and both lookups over a 661×661 grid from -0.05 to 1.05.

The climate-settings layer adds Engine's `ClimateSettings` (`climate.rs`) with its defaults: the temperature and
moisture `RangeValue`s (`ClimateRange`: seed offset, scale, falloff, min, max, bias), `BiomeShape` and the biome-edge
`BiomeNoise`. `ClimateRange`'s `min`/`max`/`bias` are `getMin`/`getMax`/`getBias` (`Math.min`/`Math.max` and
`NoiseUtil.clamp`, so swapped bounds are reordered, -0.0 is kept and NaN passes through) and `apply` biases a module
by half the bias and clamps it to that range. `BiomeNoise::build` is Engine's `build(seed)`; as in Engine, its
`SIMPLEX_RIDGE` type builds a `Simplex2` noise, and where Engine throws for a negative octave count this port builds
no octaves. Engine's settings serializer (keys, `@Range` limits, `@Rand` seed offsets) is not ported: the settings are
plain structs. The test replays `ClimateSettingsOracle.java` (in `.local/terraforged/oracles/climate_settings/`):
every default bit for bit, `getMin`/`getMax`/`getBias` and `apply` over all combinations of 15 edge values (0 and 1,
signed zeros, values outside 0..1, subnormals, ±infinity, NaN) for min, max and bias, and `build` over seven noise
types, four seeds and eight scale/octave/gain/lacunarity configurations.

The climate-module layer adds Engine's `ClimateModule` (`climate.rs`): its constructor draws the module, warp and
macro-noise seeds from the context's `&mut Seed`, three from the moisture offset seed and two from the temperature
one, in Engine's order, and builds the warped temperature and moisture modules (warp strengths in double math, as
Engine passes them). `apply` (and `apply_masked`, Engine's `apply(cell, x, y, mask)`) finds the nearest of the 3x3
jittered biome cells around the warped point and writes the region id, moisture, temperature and macro-biome id at its
centre, then (masked) the region edge and the coast mask, the continent-adjusted moisture and the biome type. The
continent is any `LandValue` (`getLandValue`); the coast it writes is the world registry's own. Where Engine's biome
lookup throws (a moisture or temperature outside the type map), this panics. The climate stage below calls it. The
test replays `ClimateModuleOracle.java` (in `.local/terraforged/oracles/climate_module/`), which
builds Engine's real module from a `GeneratorContext`: the seed cursor after the constructor and every field `apply`
writes, the biome, the terrain and the continent's arguments, masked and unmasked, over six settings (seeds, biome
sizes, scales, falloffs, offsets, biases, warps and control points), 625 edge-case coordinate pairs (signed zeros,
huge values, subnormals, ±infinity, NaN) and a 19x19 grid, including the points where Engine throws.

The climate-stage layer adds Engine's `Climate` (`climate.rs`), the heightmap's climate stage
(`Heightmap.applyClimate(cell, x, z)` is `climate.apply(cell, x, z)`). Its constructor builds the climate module, then
draws the `Rand` seed and the two biome-edge offset seeds from the context's `&mut Seed`, in Engine's order. `apply`
runs the masked climate module; a cell at or below the water level then turns `coast` into `ocean`
(`TerrainType.SHALLOW_OCEAN`), while a land cell whose biome-region edge is below 0.4, or a mountain chain, samples the
module again, unmasked, at a point moved by the biome-edge noise (both offsets sampled at the original point); last,
the temperature drops by up to 0.05 above height 0.45 (0.05 above 0.75, floored at 0) and rises by up to 0.05 from 0.45
down to the ground level (capped at 1), with `Math.max`/`Math.min` semantics, so a NaN height changes nothing. Terrains
are compared and written by identity with the world registry's own. The stage
plugs into the heightmap cache's and the tile fills' `apply_climate` closures as `|cell, x, z| climate.apply(cell, x,
z)`. The test replays `ClimateStageOracle.java` (in `.local/terraforged/oracles/climate_stage/`), which builds Engine's
real `Climate` from a `GeneratorContext` over eight settings (the climate-module oracle's, each with its own biome-edge
noise from seven noise types, strengths from -14 to `Integer.MAX_VALUE`, and levels including a ground level of exactly
0.45 and one above it): the seed cursor and levels, `getRand` and the offsets, and every cell field after `apply`, with
starting heights at each threshold and its neighbours, over 625 edge-case coordinate pairs, a 19x19 and a 48x48 grid.

The continent-factory layer adds Engine's `Continent` interface and `ContinentType.create` (`continent_api.rs`):
the `Continent` trait (`apply`, edge and land values, nearest centre, `getRivermap` by continent cell or by cell),
implemented by the multi and single, advanced and fancy continent generators, and `create_continent(type, seed,
...)`, which builds the generator of each type (`MULTI`, `SINGLE`, `MULTI_IMPROVED`, `FANCY`) from the seed, which
it draws from in that constructor's order (4, 4, 7 and 4 draws), and what the constructor reads from Engine's
`GeneratorContext`. Its result plugs into the climate module and stage, which take a continent's land value. Engine's
settings deserializer (which reads the type by its exact name and keeps `MULTI_IMPROVED` for an unknown one) is
not ported; `Heightmap` calls the factory through `context.continent()`. Its test replays `ContinentFactoryOracle.java` (in
`.local/terraforged/oracles/continent_factory/`), which calls Engine's `ContinentType.create` for every type over
five settings variants with the context's seed (as `Heightmap` does) and once with a separate seed: the created
class and the seed draws, every edge, land and nearest-centre value and cell field after `apply` over a 41x41 grid
and NaN, infinite, signed-zero and huge points, the river stage through `getRivermap(cell)` over a 17x17 grid, and
`new Climate(continent, context)` on the created continent (its draws and every cell field after `apply`); the test
also checks that each created continent samples like the generator built directly.

Every continent constructor, and so the factory, takes the context's river settings (`context.settings.rivers`) and
levels, which reach the river generator as in Engine's `BaseRiverGenerator`. A second test replays the same oracle's
`RIVERS` lines, for each type with Engine's default river settings and with every river setting changed (count and
seed offset; main and branch bed and bank widths, bed depths, bank heights given in the wrong order, which the builder
sorts, and fades; lake chance, start distances, depth, sizes and banks; wetland chance and sizes): the river
generator's count and seed and its main and branch `RiverConfig`s (lengths 5000 and 4500), `LakeConfig` and
`WetlandConfig` bit for bit, and every cell field after the river stage over a 151x151 grid with 43- to 47-block
spacing.

The generator-context layer adds Engine's `Settings` and `GeneratorContext` (`context.rs`): the world, climate,
terrain, river and filter settings; the seed (`new Seed(settings.world.seed)`, the long's low 32 bits, no draws), the
levels (`new Levels(settings.world)`) and the world's one terrain registry, shared by everything built from the
context. `new` (Engine's `create` path) and `create_no_cache` build the same context, since Engine's lazy world
generator and tile cache are not forced while it is built (the world generator is `world_generator()`, below; the
tile cache is not ported); `copy` and `split(offset)` share the
levels, settings and registry and start a new seed at `root + offset` (wrapping), and `seed()`/`seed_offset(n)` return
new seeds, as in Engine. Java shares one mutable `Seed` while the Rust `Seed` is `Copy`, so consumers draw through
`&mut context.seed`: `continent()` (`ContinentType.create(context.seed, context)`) and `climate(continent)` (`new
Climate(continent, context)`) do, and pass the context's registry. Its test replays `GeneratorContextOracle.java` (in
`.local/terraforged/oracles/generator_context/`): the seed after both constructors for seven world seeds, every level
for eight heights and sea levels (zero, negative and `Integer.MAX_VALUE` included), the draws (4, 4, 7 and 4 for the
continent, 7 for the climate) and cell fields of a continent of each type and its climate built from the context, and
the seeds of `copy`, `split` and `seed()` after the parent drew.

The heightmap layer adds Engine's `Heightmap` (`heightmap.rs`), built from the generator context in Engine's order:
the terrain-region config (from the seed's root), the warped cell-edge mountain shape (two draws from the terrain seed
offset), the standard terrain provider and its shuffled populators (whose composite terrains go into the context's
registry, so `Heightmap::new` needs that registry unshared), the region borders (`plains`, drawing from the context
seed), the mountain chain, the continent (`context.continent()`), the climate (`context.climate`) and the deep ocean
(one more draw); land (region terrains and mountains blended by the mountain shape) and oceans are blended by the
continent value. `apply` runs `apply_base` (terrain `flats`, the continent and the terrain regions at the block
position, the terrain graph, volcanoes included, at the block position times `1 / globalHorizontalScale`), the river
stage on the cell's continent river map and the climate stage. It implements the tile generator's `HeightmapSource`
and plugs into the heightmap cache and tile fills as closures. `populator(terrain, variant)` returns the heightmap
itself where Engine's provider falls back to its default populator (the heightmap). Its test replays
`HeightmapGraphOracle.java` (in `.local/terraforged/oracles/heightmap_graph/`), which builds the real `Heightmap`
from `GeneratorContext.createNoCache` for four settings variants (horizontal scales 1, 2.5, 0.37 and 4, scaled and
zero-weight landforms, other levels and control points) and every continent type: the seed cursor after the
constructor, the provider's variant counts, and every cell field after `apply` over 196 edge points (signed zeros, a
subnormal, huge values, infinities, NaN) and a 40x40 grid with volcano cells, terrains compared by identity.

The world-generator factory layer adds Engine's `WorldGeneratorFactory` (`tile_generator.rs`): `new(&mut context)`
builds the heightmap (17 seed draws, 20 for `MULTI_IMPROVED`) and then the world filters (`WorldFilters::from_context`,
no draws, the beach from the context's registry); `with_heightmap` takes a heightmap built from the same context. Every
`get()` returns a `WorldGenerator` sharing that one heightmap and filter chain; `climate()` is the heightmap's, and
`terrains()` is the context's registry, the one the heightmap registered its composite terrains into. The tile
builder's `world_factory(&factory)` is Engine's `builder.factory(factory)`. Engine's lazy `context.worldGenerator`
(`new WorldGeneratorFactory(context.copy())`) is `context.world_generator()`: it draws from a new seed at the root and
leaves the context's own cursor alone, as the copy does, but builds on the context itself, since Engine's copy shares
the registry and registering here needs it unshared; it panics, before drawing, if anything already holds the registry,
and is not memoized (keep the factory it returns). Engine's other lazy supplier, `context.cache` (a cached
`TileGenerator` of size 3, border 1, on the default pool), is not ported: only Engine's `WorldLookup` reads it. Its test
replays `WorldGeneratorFactoryOracle.java` (in `.local/terraforged/oracles/world_generator_factory/`) for four settings
variants, one per continent type: the seed cursors after the direct and lazy factories (also on contexts that drew
first), what the factory shares, cells of the factory's, a direct and the lazy heightmap, and the first end-to-end
tiles, `TileGenerator.builder().factory(factory)` regions (plain and batched) and a zoomed region with the real
heightmap and filters, every backing cell compared bit for bit.

The terrain-level layer adds TerraForged's own (the mod's, not Engine's) `TerrainLevels` and `NoiseLevels`
(`terrain_levels.rs`): the world's build bounds in block Y and the noise fractions derived from them. `max_y` is the
exclusive top block and the height every fraction divides by, counted from block 0; `sea_level` is the highest water
block. They are separate from Engine's `Levels`, which TerraForged's terrains build themselves (`ModTerrains`' `new
Levels(63, 255)`, kept as is). The constructor clamps as TerraForged does: `min_y` to -2032..0 and `max_y` to
128..4064 (1.18.2's `DimensionType` limits, the same in 26.3), the sea level to 32..half the `max_y` *argument*
(before `max_y` is clamped, so a small or negative `max_y` gives a lower sea level), then the sea floor and the base
height, with `MathUtil.clamp`'s order when a bound pair is inverted. `copy()` re-clamps, the no-argument
`unconfigured()` keeps Java's `min_y` of +64, and `height`, `floor` and `ceil` use `NoiseUtil.floor` (one lower for
negative whole numbers, -1 for NaN). The defaults are TerraForged's, not vanilla 26.3's: `max_y` 480 (the 26.3
overworld ends at 320), sea level 62 (water up to Y 62, as vanilla's sea level 63), base height 128, sea floor 22,
automatic horizontal frequency `194 / (max_y - sea_level) * scale`. The settings codec is not ported; `getWaterLevel` (it reads
`TerrainData`) came with the next layer. Its test replays `TerrainLevelsOracle.java` (in
`.local/terraforged/oracles/terrain_levels/`), which runs TerraForged's own classes from the mod jar, stubbing only
`DimensionType` and the logger: 36 constructed levels and their copies, the default and no-argument levels,
`calcFrequency` and every transform over signed zeros, subnormals, huge values, infinities and NaN, plus a rounding
sweep of `height`, `floor` and `ceil` (halves and negative halves, negative fractions and whole numbers, the `int`
limits), bit for bit. The constructed levels include a sweep of the constructor's bounds: `max_y` arguments of 0, -1,
`Integer.MIN_VALUE` and `Integer.MAX_VALUE` (a zero or negative one gives a sea level of 0 or below, and the sea floor
then clamps below it), `max_y >> 1` below 32 (an inverted sea-level pair: a requested level above it gets `max_y >>
1`, one below 32 gets 32), each limit's endpoints and one past them, and the sea floor and base height at their
bounds. TerraForged's newer source tree (6dd607eb) builds these levels the same way: its `TerrainLevels`
constructor, `copy()` and transforms, `NoiseLevels` and `MathUtil` compile to the jar's bytecode (only the settings
codec, which is not ported, and javac's string concatenation in `toString` differ;
`.local/terraforged/oracles/terrain_levels/version/compare.sh` checks it).

The terrain-data layer (`terrain_data.rs`, again the mod's classes) adds the per-chunk storage TerraForged's
generator fills: `Index` (`CHUNK` masks x and z into the chunk, so world coordinates of any chunk, negative ones
included, read the right cell; a bordered index offsets and wraps in Java `int` math), `FloatMap` and `ObjectMap`,
`NoiseSample` and `NoiseData` (one chunk's height, base, river noise and terrain with a one-cell border), and
`TerrainData`, which `accept`s a `NoiseData`: heights and base levels scaled by `TerrainLevels`, the clamped
gradient from the four neighbours (the border cells), river noise, terrains, and the extrema with `Math.max`/`min`
(a NaN height makes them NaN; they start at `Float.MAX_VALUE`/`Float.MIN_VALUE`). `TerrainLevels::water_level` is
`getWaterLevel`: a river or lake cell whose river noise is exactly zero fills to its base height, the rest to the
sea level given. Heights are TerraForged's block Y, `floor(noise * max_y)` counted from block 0 with
`TerrainLevels`' `max_y` (480 by default), not clamped to 26.3's -64..320: fitting them into a 26.3 world is the
generator's job. Samples and noise data take the world's `TerrainRegistry`, so the default terrain is that
registry's `none`; unset cells have no terrain (Java's null), and `getWaterLevel` panics on one where Java throws.
`NoiseData` is filled by the newer source's `NoiseGenerator.generate` (`noise_generator.rs`, below). Its test replays
`TerrainDataOracle.java` (in `.local/terraforged/oracles/terrain_data/`), which runs these classes from the mod jar (stubbing
`DimensionType`, the logger and the mod's registry-backed `TerrainType` wrapper of Engine's `none`): indices over
negative and huge coordinates and borders, four chunks (smooth, random, special values with NaN, infinities,
signed zeros and subnormals, and one with unset cells) read with four `TerrainLevels`, every element, height,
gradient and water level, bit for bit.

The terrain-generator layer (`terrain_generator.rs`, the mod's `TerrainGenerator` and `ObjectPool` as in the
1.18.2-0.3.1 jar) turns a noise generator's chunk `NoiseData` into `TerrainData` and answers `getHeight` (height noise
scaled by `TerrainLevels`). `NoiseGenerator` is the part of the mod's `INoiseGenerator` it calls, seed-first as in the
newer source (`height_noise(seed, x, z)`, `generate(seed, chunk_x, chunk_z, consumer)`; `TerrainGenerator::generate`
and `height` take the seed too and only pass it on, as the newer source's do). The jar instead binds the seed into the
noise generator (`with(seed, levels)`) and its calls take coordinates only, so a jar caller passes the seed it bound.
The ported noise generator (`noise_generator.rs`, below) implements the trait. The pool keeps the mod's quirk: `take` always builds a new value
(the polled one is dropped), so every `generate` fills a fresh `TerrainData` and restored data is never reused; its
capacity is `Environment.CORES` (the processor count) and `forCacheSize` uses `NoiseUtil.floor`. Its test replays
`TerrainGeneratorOracle.java` (in `.local/terraforged/oracles/terrain_generator/`), which runs the jar's classes over
a stub noise generator (the same stubs as the terrain-data oracle): pool take/restore sequences, `forCacheSize`, and
heights, noise coordinates and every cell of generated chunks for four `TerrainLevels` over plain and special values,
bit for bit (the stub ignores the seed the test passes, as the jar's has none).

The terrain-blend utilities (`terrain_blend_util.rs`, the mod's `MathUtil.rand(int)`, `WeightMap`,
`Object2FloatCache` and `SpiralIterator` as in the 1.18.2-0.3.1 jar) are the pieces TerraForged's terrain blender
builds on. `random` hashes an `int` to 0..1 in Java `int` math. `WeightMap` picks a value from a 0..1 noise by
cumulative weight bands (values past the end of the weights weigh 1, but only the weights themselves are summed,
from +0 in order, and scaled by 0.99999) and packs a value's band with `PosUtil.packf`; values are compared by identity, and a missing one has band
0. `Object2FloatCache` keeps the mod's preallocated slots: `put` writes slot `size()`, so putting a key again moves
it into the slot the next new key also takes, a full cache panics where Java throws, and a missing key reads NaN.
`SpiralIterator` walks square rings outward in wrapping `int` math, with the mod's quirks (a start radius above the
maximum repeats one position forever; from a negative one, stepping into ring 0 panics where Java divides by zero). Its
test replays `TerrainBlendUtilOracle.java` (in `.local/terraforged/oracles/terrain_blend_util/`),
which runs these classes from the jar: `rand` over special and hashed `int`s, 15 weight maps (NaN, infinities,
signed zeros, subnormals, huge and negative weights, short and long weight arrays) read over 26 noises, a cache
sequence, and 14 spirals with extreme radii and coordinates, bit for bit.

The terrain blender (`terrain_blender.rs`, the mod's `TerrainNoise` and `TerrainBlender` as in the 1.18.2-0.3.1
jar) is the terrain module of the mod's noise generator. A `TerrainNoise` is a terrain, its weight and its noise; a
noise whose minimum is below 5/255 (compared as a `double`) is raised by it and clamped to 0..1. `TerrainBlender`
binds the seed when it is built (cells hashed from `(int) seed + 21491124`, warped by two 3-octave simplex noises
seeded from `(int) seed + 12678`, `int` math wrapping), warps and scales the point into a grid of jittered cells,
and picks the nearest cell's terrain from the weight map by the cell's hash. Points close to a cell border blend in
the nine surrounding cells' noises with squared border weights, each terrain sampled once through the
`Object2FloatCache` (a NaN there is sampled again, as the jar's `isNaN` test does), and the sum is clamped with
`NoiseUtil.clamp`. Where Java throws, this port panics: when no cell's squared distance is below `Float.MAX_VALUE`
(NaN, infinite or huge coordinates leave the closest index at -1) or when the weight map has no terrain for a cell
(NaN weights). Its test
replays `TerrainBlenderOracle.java` (in `.local/terraforged/oracles/terrain_blender/`), which runs the jar's classes
with stubs for Minecraft's `Holder` and the mod's codec classes: 15 terrain noises (signed zeros, NaN, negative,
and values one ulp around the 5/255 bound), then nine blenders (seeds with high bits, `Long.MIN_VALUE`, wrapping
seed sums, negative and tiny scales, zero, negative and huge jitters, NaN, zero and out-of-range blending, NaN and
zero weights) over 72 points (negative, huge, infinite and NaN coordinates), each value, closest pair and terrain,
bit for bit.

The blender's nearest-terrain search (`find_nearest` and `iterator`, the jar's `findNearest`, `iterator` and `find`)
walks `SpiralIterator`'s square rings around the point's warped, scaled cell. `find_nearest` picks the first terrain
noise whose terrain has the asked terrain's name (`String.equals`, as the jar; `None` where it returns null) and its
weight band (`getBand`, by identity); each `next` walks on to the next cell whose hash falls in the band (above the
lower bound, up to the upper one) and returns its jittered point divided by the frequency, `PosUtil.packf`ed. When
the rings run out during a search the result is the jar's `0L` divided the same way (signed zeros), then `None`
(the jar's `0L`). The spiral's quirks stay: ring 0 is empty, a negative start radius panics where Java divides by
zero, and a start radius above the maximum revisits one cell forever, so a search there that misses never returns,
as in Java. The same oracle replays ten blenders (the nine above and one with two noises named "hills") over 13
start points (signed zeros, negative, huge, infinite and NaN coordinates): each `iterator(x, z, 1, 2)` walk, then
for five query terrains (the set's first and last, another instance named "hills", "Hills", and river) and six
radius pairs (including 0..0, -1..3 and `Integer.MAX_VALUE - 1`..`Integer.MAX_VALUE`) four finder steps, bit for bit.

Reseeding (`Module::with_seed`, the jar's `ContextSeedable.withSeed`; `TerrainNoise::with_seed` and
`TerrainBlender::with_seed`) builds a new graph and leaves the old one as it is. The jar writes the module out with
Cereal and reads it back with the seed in the context, so the port rebuilds each node with its Java constructor and
adds the seed's low 32 bits to the octave sources', `CellNoise`'s and `CellEdgeNoise`'s seeds (wrapping like an
`int`); caches start empty. The specs' quirks stay: a `Bias` reads its bias module back as 0, cell sources lose
their displacement (back to 1), `MultiBlend` truncates its blend to an `int`, the children of the combiners
(`Add`, `Multiply`, `Max`, `Min`, `Sub`), of Engine's erosion `Ridge` and of `Line` keep their seeds, a custom
module in a list is dropped and a custom `CellNoise` lookup becomes 0. Where the jar throws (a custom module
elsewhere, `Domain.DIRECT`, `AddWarp` and `CompoundWarp`, whose specs sit under `CumulativeWarp`'s name, and a
`MidPointCurve` or `SCurve`), the port panics. A reseeded terrain noise goes through `TerrainNoise`'s constructor
again: since its raising bias reads back as 0, a raised noise is raised once more. The reseeded blender is
`new TerrainBlender(seed, scale, jitter, blending, noises)`. `ReseedOracle.java`
(`.local/terraforged/oracles/reseed/`) reseeds 71 graphs (every node kind, nested, and each case where the jar
throws or drops a node) with six seeds (high bits, `Long.MIN_VALUE + 5`, `Integer.MAX_VALUE`, -1, 0) through the
jar's own `ContextSeedable` and hashes the bounds and 261 samples of each; `TerrainBlenderOracle.java` reseeds its
nine blenders and prints their noises and values; the tests match both bit for bit.

The blender's seed-first form (`TerrainBlender::try_value_seed_first`, `try_value_with_seed_first`,
`try_iterator_seed_first`, `try_find_nearest_seed_first`, and `Blender`'s `try_*_seed_first`) ports the newer source
tree's `TerrainBlender` (6dd607eb), which takes the world's seed on every call: its noise generator builds the blender
once with the constant seed 45763218 and never reseeds it. Compiled and compared method by method with the jar's
(`.local/terraforged/oracles/terrain_blender/source/version/compare.sh`), the two classes differ only in where the seed comes from:
the jar hashes the cells from the `regionSeed` field (`(int) seed + 21491124`, set in the constructor), the newer
source from the call's `seed + 21491124` (`int` wrap; the constructor's seed is not used there), and passes the
call's seed to the warp's `getX`/`getY` and to every terrain noise (Noise2D 2cb4843: each source adds it to its own
seed, Perlin ignores it). The warp's construction, the cell search, the weights, the `Object2FloatCache` blend with
its NaN re-sample, `find` and the util classes (`WeightMap`, `Object2FloatCache`, `SpiralIterator`, `MathUtil`) are
the same bytecode (`SpiralIterator.PositionFinder` differs only by the newer compiler's null check of its outer
instance). So a blender built with seed 0 and sampled at runtime seed 0 gives the jar's values for seed 0
(where Noise2D 2cb4843's `NoiseUtil.map` agrees with the jar's: no zero or infinite source ranges). The port keeps
the jar's methods as they are and adds the seed-first ones beside them over the same helpers; a terrain noise with a
custom module (no seed-first form) makes them return an error rather than sample it seedless (the first error ends
the blend and is not cached). `TerrainBlenderSourceOracle.java` (`.local/terraforged/oracles/terrain_blender/source/`,
`run.sh`: the newer source's classes on Noise2D 2cb4843 with stubs for `Holder` and the codec classes) runs the jar
oracle's nine blenders over its 72 points at six runtime seeds (0, 17, -91, both `int` extremes and -21491124, the
region seed 0) and its ten searches at three of them, 15,978 lines; the test matches them bit for bit, and another
checks the seed-0 lines against the jar oracle's.

The cave noise primitives (`cave_noise.rs`) port TerraForged's `CaveType`, `NoiseCave` and `UniqueCaveDistributor`.
The jar and 6dd607eb agree except for the runtime seed, and the samplers follow 6dd607eb: `height`, `cavern_size`
and `floor_depth` take the runtime seed (`Seeds.get`, the world seed narrowed to an `int`) and sample their noise
seed-first (`Err` where a node can't be), and the distributor places its cells with the runtime seed while it rolls
their occupancy with its own seed (the parameter shadows the field). At seed 0 the samplers give the jar's
seedless results wherever seed-first and seedless sampling agree (not at zero or infinite source ranges, see the
seed-first sources), and the distributor's seedless sample is the jar's (its own seed for both).
`SeededCaveNoiseOracle.java` (`.local/terraforged/oracles/noise_cave_seeded/`) runs 6dd607eb's own classes on Noise2D
2cb4843: the 8 distributors and 6 caves below at 8 runtime seeds (caves also after 6dd607eb's `withSeed`, which
reseeds these graphs as the jar's does), and both modifiers
over a 20,000-point scan; the test matches it bit for bit, in debug and release. `unique_cave_noise`
and `cave_breach_noise` are 6dd607eb's `NoiseCaveGenerator.createUniqueNoise(500, 0.05)` and
`createBreachNoise(300, 0.7)`, with fixed seeds; the jar derives them from the context seed instead.
`CaveType::for_name` upper-cases like `toUpperCase(Locale.ROOT)` and returns `None` where `valueOf` throws.
`NoiseCave` scales its elevation, shape and floor noise to whole blocks with `NoiseUtil.floor` and Java `int`
wrapping (`rangeY = maxY - minY`; a range of 0 or less gives 0), and `with_seed` reseeds the three modules through
`Module::with_seed` while keeping the cave's own seed. `UniqueCaveDistributor` places jittered cells on staggered
rows and zeroes cells above its density; `module()` makes it a `Module` with `Noise`'s default 0..1 bounds, as
`NoiseCaveGenerator` uses it (`Module::seeded_custom`, a custom module with a seed-first sample too). The carver
is `cave_carver.rs` and the cave decorator `NoiseCaveDecorator` in `decorator.rs` (both below). `CaveNoiseOracle.java`
(`.local/terraforged/oracles/cave_noise/`) runs the jar's own classes on 12 names, 8 distributors (and
`NoiseCaveGenerator`'s clamp, map and warp graph over each) at 217 points (signed zeros, negative, huge, infinite
and NaN), and 6 caves (TerraForged's `mega` cave and `int` wrap, NaN and overflow cases) over 72 block positions
and 8 size modifiers, each reseeded with 6 seeds; the test matches its output bit for bit, in debug and release,
sampling seed-first at seed 0.

The noise caves (`cave_assets.rs`) are TerraForged's five `worldgen/cave` files, bundled byte for byte in
`data/worldgen/cave/` (TerraForged Reimagined a24d450's copies, MIT; 6dd607eb generates the same bytes): the unique
caves `mega` and `mega_deep` and the global caves `synapse_high`, `synapse_low` and `synapse_mid`.
`decode_noise_cave` is `NoiseCave.CODEC` on Minecraft 26.3's DFU: `type` is a string read by `CaveType.forName`;
`seed` (default 0), `size`, `min_y` (default -32) and `max_y` are `Codec.INT` over Gson's `intValue` (fractions
truncate, wider numbers keep their low 32 bits, `null` counts as absent, strings and booleans are errors; 1.19's
DFU, which 6dd607eb ran on, gave an invalid optional field its default instead); the three graphs go through the
terrain asset decoder (`decode_noise`, below), which reads `Abs` with `new Abs` and a `Bias` whose bias is a module
(as in `mega`'s shape) as a bias of 0, as Cereal does. `current_caves()` decodes them in the registry's order
(`mega`, `mega_deep`, `synapse_high`, `synapse_low`, `synapse_mid`: a 26.3 data-pack registry sorts by path, then
namespace; `ModCaves`' order only applies to data generation) and builds `NoiseCaveGenerator.createArray`'s array:
each global cave twice, copy `j` reseeded with `j * 0xFA90C2L` (a `long`; `with_seed` keeps its low 32 bits), 8
caves in all, which the carver (`cave_carver.rs`, below) builds once per world. `CaveAssetsOracle.java`
(`.local/terraforged/oracles/cave_assets/`, `run.sh`) checks the bundled files against both trees, decodes them with
6dd607eb's own `NoiseCave.CODEC`, `DataUtil`, `SuperCodec` and `withSeed` on Noise2D 2cb4843 and 26.3's DFU, runs
`createArray` (copied verbatim) and folds each copy's seed-first height, cavern size and floor depth at 8 runtime
seeds, and decodes 30 edge-case documents; the test matches its output, in debug and release.

The biome viability rules (`biome_viability.rs`) port TerraForged 1.18.2-0.3.1's `worldgen.biome.viability`
package as the jar has it: `Viability.NONE`, the edge, height, multiply, noise, saturation, slope and weighted-sum
rules, `SumViability.Builder`, the two `getFallOff` curves, and the `ViabilityContext` class over a chunk's
`TerrainData` (no biome edges). Heights scale by `maxY / 255`; a height outside the band scores 1, as in the jar;
NaN fails every threshold test; the sum is clamped with `NoiseUtil.clamp` (NaN passes); a sum with fewer weights
than rules panics where Java throws. The context's seed comes from 6dd607eb (the jar has none). `fitness` is the
jar's; `try_fitness`, which the vegetation step samples, is 6dd607eb's: the same rules, but the noise rule reads its
noise seed-first, `getValue(context.seed(), x, z)` (an unsupported node is an `Err`). 6dd607eb's
`ViabilityContext.seed` is never assigned, so the vegetation step's context answers 0, as the jar-shaped
`TerrainViabilityContext` does. The rules' Cereal specs are decoded by `vegetation.rs` (below).
`BiomeViabilityOracle.java` (`.local/terraforged/oracles/biome_viability/`) runs the jar's own classes
on stub contexts (every rule over signed zeros, infinities, NaN, the smallest subnormal and heights at and next
to each threshold, for seven `maxY` values) and on real `TerrainData` chunks through the jar's `ViabilityContext`;
the test matches its 91,275 fitness values bit for bit, in debug and release.

The vegetation configs (`vegetation.rs`) are TerraForged's eight `worldgen/vegetation` files (`trees_copse`,
`trees_hardy`, `trees_hardy_slopes`, `trees_patchy`, `trees_rainforest`, `trees_sparse`, `trees_sparse_rainforest`,
`trees_temperate`), bundled byte for byte in `data/worldgen/vegetation/` as 6dd607eb generates them (seven are the
same in TerraForged Reimagined a24d450; Reimagined's `trees_temperate` weighs its height rule -0.5, not -1), and the
eight `trees/*` biome tags they select, which neither source tree ships, bundled byte for byte from TerraForged's
0.3.1 alpha 2 jar in `data/tags/worldgen/biome/trees/`. `decode_config` is 6dd607eb's `VegetationConfig.CODEC` on
26.3's DFU: `frequency`, `jitter` and `density` are optional `Codec.FLOAT`s (default 1, `Float.parseFloat` on the
text, `null` counts as absent, strings and booleans are errors); `biomes` is `#` + an identifier
(`TagKey.hashedCodec`; a tag that is not bundled has no members, as an unbound tag); `viability` goes through
`ViabilityCodec`'s Cereal specs with their defaults, over the terrain asset decoder's data tree (below: numbers are
rebuilt by DFU first, so `-0.0` reads as 0 there, and a non-number reads as 0). Two differences: a spec name that is
not a viability's is an error (Cereal would try every viability spec whose fields the object has, in hash order),
and a `Sum` with fewer `amounts` than `rules` is an error (Java throws on its first sample). `current_vegetation()`
decodes them in registry order (by id), and `config_for` is `BiomeVegetationManager.getConfig`: the first config
whose tag holds the biome, so taiga (in `trees/hardy` and `trees/hardy_slopes`) takes `trees_hardy`.
`VegetationFeatures::create` splits a biome's vegetal feature ids by path keyword into trees, grass and the rest
(6dd607eb's keywords plus TerraForged Reimagined's `pale_garden_vegetation`, so 26.3's pale oaks are placed as trees:
a departure from 6dd607eb, by Arthur's decision).
26.3 drops a whole tag that names a missing biome, and `trees/patchy` names `terraforged:oak_forest`, which only
the bundled biome provides (a TerraForged world's pack has it); `bind_tags` applies that rule to a world's registry, so
without that biome `trees/patchy` holds nothing and `minecraft:wooded_badlands` has no config.
`VegetationOracle.java` (`.local/terraforged/oracles/vegetation/`, `run.sh`) checks the bundled files against
6dd607eb, Reimagined and the jar, decodes them and 53 edge-case documents with 6dd607eb's own codec on Noise2D
2cb4843 and 26.3's DFU, and folds each noise rule's seed-first samples; the test matches its output bit for bit.
`VegetationFitnessOracle.java` (`.local/terraforged/oracles/vegetation/fitness/`, `run.sh`) samples each bundled
config's 6dd607eb `getFitness` through 6dd607eb's own `ViabilityContext` (seed 0) over a hash-defined terrain at two
`maxY`s; `try_fitness` matches it bit for bit.

The biome overrides (`biome_sampler.rs`) port TerraForged 1.18.2-0.3.1's `ClimateSample` and
`BiomeSampler.getBiomeOverride`, with `ContinentPoints`' thresholds. Continent noise at or below 0.25 gives a deep
ocean, at or below 0.5 a shallow ocean, at or below 0.505 (`BEACH + beachSize`, folded to one float) a beach; past
those bands a river or lake terrain whose river noise is exactly 0 (either signed zero) gives a river, and anything
else keeps the input biome. The climate type picks the variant (cold, frozen, lukewarm or warm oceans, snowy beach,
stony shore, frozen river). NaN fails every band and the river test, as Java's float compares do; terrains count as
rivers or lakes through their category, composites included. `ClimateSample.reset` resets only the `NoiseSample`
part. Minecraft's biome holders don't exist here, so an override is a `BiomeOverride` that names its biome key; the
initial biome pick (`getInitialBiome`), `sampleBiome` and the climate noise come with later layers (below).
`BiomeOverrideOracle.java` (`.local/terraforged/oracles/biome_override/`) runs the jar's own bytecode, with
Minecraft's biome classes and the mod's `BiomeMapManager` stubbed, over 26 continent values (each threshold and its
neighbouring floats, signed zeros, infinities, NaN), every climate type, the 16 built-in terrains and three
composites and 10 river values (signed zeros, subnormals, infinities, NaN); the test matches its 54,340 overrides
and the reset fields exactly, in debug and release.

The continent control points (`continent_points.rs`) port TerraForged 1.18.2-0.3.1's `ContinentPoints` and its
`FalloffPoint` record. The five constants (deep ocean 0.1, shallow ocean 0.25, beach 0.5, coast 0.55, inland 0.6)
are the ones `biome_sampler.rs` already holds, re-exported rather than copied. `terrain_type` gives the world
registry's deep ocean below 0.25, shallow ocean below 0.5, coast below 0.55 and `none` otherwise (NaN included).
`falloff_curve` turns Engine's `ControlPoints` (the same validated points the beach filter and climate use; the
defaults when the settings fail `validate`) into five falloff points, and `falloff` maps continent noise through
them with `MathUtil.map` (noise.rs's `map` then `lerp`); below every point it maps `0..last` onto `0..0.1`. A
zero-width band, such as inland at exactly 1, gives NaN, as in the jar. The continent shape reads the falloff
curve. `ContinentPointsOracle.java` (`.local/terraforged/oracles/continent_points/`,
which reuses the biome override's stubs) runs the jar's own bytecode over the constants, 31 continent values (each
threshold and its neighbouring floats, signed zeros, subnormals, infinities, NaN) and eleven curves (six Engine
settings, including invalid, NaN, subnormal and `-0` ones, and five hand-built curves: empty, one point, a NaN or
`-inf` control point, out of order); the test matches its output bit for bit, in debug and release.

The continent cell primitives (`continent_cell.rs`) port TerraForged 1.18.2-0.3.1's `worldgen.noise.continent.cell`
package: `CellPoint` (a cell's position and noise; `noise()` returns the `noise0` field, as in the jar), `CellShape`
and `CellSource`. A square cell sits at `cell + MathUtil.rand(hash, prime) * jitter` on each axis (noise.rs's `hash`
and `terrain_blend_util.rs`'s `random`, in Java `int` math); `HEXAGON` shifts odd rows by half a cell with half the x
jitter, and its `adjustY` scales y by 1.2 (x is never adjusted). The sources are Perlin (24 gradients, `CURVE3`),
simplex and cubic noise, mapped to 0..1 by `(1 + sample) * 0.5`. The continent shape blends the cells; the cell
generator places them. `ContinentCellOracle.java` (`.local/terraforged/oracles/continent_cell/`, which
reuses the biome override's stubs) runs the jar's own bytecode over 12 hashes, 15 cell coordinates (`int` extremes and
past 2^24), 14 jitters (signed zeros, a subnormal, huge, infinite and NaN) and 27 sample coordinates (the same kinds)
for 6 seeds; the test matches its 177,604 results bit for bit, in debug and release.

The continent configuration (`continent_config.rs`) ports TerraForged 1.18.2-0.3.1's `worldgen.noise.continent.config`
package: `ContinentConfig` with its `Shape`, `Noise` and `Rivers` parts, the mod's own `RiverConfig` (not Engine's)
and `FloatRange`. They are plain structs with the jar's defaults (cell size 400, jitter 0.75, threshold 0.525, base
falloff 0.01..0.25, `SQUARE` cells from `PERLIN` noise, falloffs 1.5 and 1, lake density 0.75), the river and lake
presets, `copy`, and `scale`, which multiplies the three widths but not erosion or the depths; `FloatRange::at` is
`NoiseUtil.lerp`, unclamped. The jar has no codec for them, so none is ported. The seeds stay 0 until the cell
generator and the river pipeline read them (the jar only ever sets `seed0` and `seed1`). The continent shape and the cell
generator read it. `ContinentConfigOracle.java`
(`.local/terraforged/oracles/continent_config/`, which reuses the biome override's stubs) runs the jar's own bytecode
over every default and 26 edge values (signed zeros, subnormals, `int`-precision limits, infinities, NaN) for `at`,
`scale`, `copy` and the presets' `copy`/`scale` chains; the test matches its 89,024 results and every default bit for
bit, in debug and release.

The continent shape (`continent_shape.rs`) ports TerraForged 1.18.2-0.3.1's `ShapeGenerator`. It derives the
threshold and the base falloffs (`threshold + baseFalloffMin`, `threshold + baseFalloffMax`) from the continent
configuration; `sample` blends the 5x5 cells around the nearest one, weighting each by how far past the nearest
distance it lies (blend range: the mean of the two nearest distances times the configured falloff), into the base
noise (the cells' `noise()` mapped onto the base falloffs) and the continent noise (their 0/1 threshold values through
the control points' falloff curve); `value` does the same for the continent noise over 3x3 cells, clamped first (the
jar never calls it). The cells come through a `ContinentCells` trait (the jar's `ContinentGenerator`, ported as the cell
generator below, with the seed-first runtime seed it passes through). Where the cell window would wrap past `i32` (about 2^31 cells
out) the jar overruns its buffer, reads a stale one or loops forever; the port panics there. `ContinentShapeOracle.java`
(`.local/terraforged/oracles/continent_shape/`, a stub `ContinentGenerator` plus the biome override's stubs) runs the
jar's own bytecode over 9 configurations (signed zeros, subnormals, huge, infinite and NaN thresholds, falloffs and
ranges; custom and invalid control points), each derived threshold and its neighbouring floats, the weights over a
grid of edge values, and 6,240 blended outputs per configuration (both cell shapes, four cell tables from plain to
NaN and infinite jitters and noises, nearest cells up to the `int` extremes, NaN, infinite, signed-zero and huge
points); the test matches them, and the cells visited, bit for bit, in debug and release.

The continent cell generator (`continent_cells.rs`) ports the cell half of TerraForged 1.18.2-0.3.1's
`worldgen.noise.continent.ContinentGenerator` (the jar has no separate `CellGenerator`). Each cell sits at its jittered
position (the cell shape over `NoiseUtil.hash2D(seed0, x, y)`); its noise is two octaves of the cell source at 0.1 times
that position (lacunarity 2.75, gain 0.3, seed `seed1 + 6569`), and `noise0` is the first octave. `nearest_cell` is
`getNearestCell`: the strictly nearest of the 3x3 cells around the adjusted point's `NoiseUtil.floor`; where that floor
is `Integer.MAX_VALUE` (points at or past 2^31, at -2^31, infinite) the jar's loop wraps and is empty, so the answer is
cell (0, 0), and the continent shape on top of real cells never reaches its `i32` panics. The jar caches cells in a
lossy cache whose pool always hands out fresh points, so the cache never changes a result. The port's `cell` (`getCell`)
goes through the newer source's cell cache (see the cache wiring below); `compute_cell` and the spawn search compute
directly, as both versions do. The jar has no runtime seed: `ContinentNoise.createContinent` binds the world
seed by drawing `seed0` and `seed1` from the generator context's `Seed`. The port's `seed` parameters are the newer
source's runtime seed, added (Java `int` wrap) to both seeds; 0 gives the jar's cells, and seed `s` over `(a, b)`
equals seed 0 over `(a + s, b + s)`. The river generator is `RiverNetwork`, below.
`ContinentCellsOracle.java` (`.local/terraforged/oracles/continent_cells/`, with a `Mth` stub plus the biome override's
and terrain levels' stubs) runs the jar's own `ContinentGenerator` (its constructor, cache and pool) and
`ShapeGenerator` over 8 seed pairs (`int` extremes and wrapping sample seeds), 10 jitters (signed zeros, a subnormal,
huge, infinite and NaN), 144 cells per table up to the `int` extremes (each read twice, through the cache), 676
nearest-cell searches per generator over NaN, infinite, signed-zero, subnormal, huge, ±2^31 and 2^24 points, and the
shape's blended outputs over the same points, for both cell shapes and all three sources; the test matches its 942,336
results bit for bit, in debug and release.

`world_offset` ports the jar's spawn-offset search, `ContinentGenerator.getWorldOffset`: it walks `SpiralIterator(0, 0,
0, 100000)` (rings 1 to 100,000 around cell (0, 0), each from its `(-r, -r)` corner along +x, +y, -x, then -y; the
origin cell itself is never visited) and returns the jittered position of the first cell that is land
(`getThresholdValue` is not 0: noise at or above the threshold, NaN included) whose spawn neighbourhood (`isValidSpawn`
with radius 3: the ten cells at `dy` 1 or 2 and `dx` -2 to 2) is all land, or `(0, 0)` if there is none (like the jar,
only after walking all ~4·10^10 cells when no land is near the origin). The jar recomputes it on every call
(`ContinentNoise` keeps the answer in a field; the port's `ContinentNoise` keeps one per runtime seed, below), so
`world_offset` caches nothing; the runtime seed shifts both cell seeds as
for every cell. `ContinentSpawnOracle.java` (same folder, `spawn.txt`) runs the jar's `getWorldOffset` for both cell
shapes and all three sources over seed pairs drawn through Engine's `Seed` from `long` world seeds
(`Long.MIN_VALUE`/`MAX_VALUE`, values that wrap `seed1` past `Integer.MAX_VALUE`) and the `int`-extreme seed pairs, NaN
and zero jitters, thresholds 0.525, 0, 0.25, -inf and NaN, and thresholds equal to and one float either side of the
first cell's noise and of its neighbourhood's least noise; it also runs the same loop at radii 0 to 3 over the jar's
`SpiralIterator`, private `computeCell` and `isValidSpawn` (checked against `getWorldOffset`) at every cell noise near
the origin and one float above it, so each answer depends on the search order. The tests match its seed pairs and 45,552
hashed floats bit for bit, in debug and release.

The continent river nodes (`river_nodes.rs`) port TerraForged's own continent rivers, package
`worldgen.noise.continent.river` (not Engine's `rivermap`, ported above as a separate implementation): `RiverNode` (a
segment with heights and radii at both ends; `projection` = `getProjection`, clamped to 0..1 with NaN passing through;
`distance_squared` = `getDistance2`, the squared distance to the point at that projection bowed sideways by the
`displacement` times a Curve3 bump over `NoiseUtil.map(t, 0.05, 0.95, 0.9)`; `distance`; `radial_distance` =
`getDistance(t, d2)`, falling from 1 on the centre line to 0 at the radius; the linear `x`/`y`/`height`/`radius`),
`NodeSample` (distance NaN until reset, `Float.MAX_VALUE` after), `CarverSample` (river and lake samples on the world
registry's river and lake terrains) and `RiverPieces` (a cell's river and lake nodes). These four classes are the same
in the 1.18.2-0.3.1 jar and in the newer source tree. `RiverNodesOracle.java` (`.local/terraforged/oracles/river_nodes/`)
runs the jar's classes over four nodes (one of zero length) at every pair of 20 edge inputs (signed zeros, a subnormal,
huge, infinite and NaN), at every projection and squared distance among them, and with each node field set to each
edge input in turn; the test matches its 44,400 hashed results, the samples' sentinels and the pieces' order bit for
bit, in debug and release. The nearest-node search is the river network's (below).

The continent river carver (`continent_carver.rs`) ports the carving arithmetic of the newer source tree's
`worldgen.noise.continent.river.RiverCarver`. The constructor scales the river and lake configurations by the noise
levels' frequency over the continent scale (`1f / scale` with the `int` scale, so 0 gives infinity) and derives the
blend radius (`min(1, max(both valley maxima) + 0.2)`, which Java never reads). `carve_with_erosion` is `carve` after
its erosion lookup. It multiplies the base noise by the coast modifier (`NoiseUtil.map(continent, COAST, 1, 1 -
COAST)`), then carves the river node and then the lake node, each from the previous base noise. A node inside the
valley lerps the base noise from its level to the sample's base. The carver lowers the height towards the bank level
across the valley, with an eroded U-to-linear falloff that fades out over the valley's last 2.5%, and towards the bed
level inside the banks, where the node's terrain is taken. The river noise falls across the valley and the banks.
Below `BEACH` the river noise is reset to 1. `getBaseNoise`'s -1 ("outside the valley") also skips a node whose carved
base really is -1, as in Java. Version check: compiled from the newer source, `RiverCarver`, `NoiseLevels`,
`NoiseSample`, `ContinentPoints`, the three config classes, `NodeSample` and `CarverSample` give the same bytecode as
the 1.18.2-0.3.1 jar (up to constant-pool layout) except in one place. The source's `carve(int seed, float x, float y,
…)` reads the erosion seed-first (`erosionNoise.getValue(seed, x, y)`, Noise2D); the jar's `carve(float x, float y,
…)` calls `getValue(x, y)`. `RiverCarver::carve(seed, x, y, …)` builds that erosion module (a two-octave ridge at
frequency 128 with the builder's ridge gain 0.975, seed `rivers.seed + 21221` wrapping like an `int`) and reads it
seed-first (`Source::value_seed_first`, below); `carve_with_erosion` is the rest of `carve`, given the erosion value. `ContinentCarverOracle.java` (`.local/terraforged/oracles/continent_carver/`, river nodes' class path
and stubs; the bytecode comparison is `version/compare.sh`) runs the jar's own `carve` with the erosion module swapped
for a constant by reflection. It covers 16 configurations (`int` scales 0, negative and `MAX`; NaN, infinite and
signed-zero widths, depths and erosion; equal and crossed radii) over five noise levels, and a river grid of 766,080
carves. The grid takes each scaled radius with its float neighbours, points across the valley through the erosion
fade, the 0.4/0.6 shape and `COAST`/`BEACH` thresholds with their neighbours, the -1 sentinel, NaN, infinities and
signed zeros. It also covers incoming river noise and river-then-lake precedence. The test matches the outputs (base,
height, river noise, terrain) bit for bit, in debug and release.

The continent river network (`river_network.rs`) ports `computeNodes` of the newer source tree's
`worldgen.noise.continent.river.RiverGenerator`: the river and lake pieces of one cell. An ocean cell (threshold value
0) has none. A land cell scans its neighbours in `DIRS` order, keeping the lowest one at or below its own value (the
last on ties) and taking the rivers of every other land neighbour whose strictly lowest neighbour it is (the inflows);
then it adds its own river to the lowest neighbour, if any. A source cell (no inflow) whose lowest neighbour's value is
at most 0 gets nothing ("chode-rivers" are culled). Each
connection is two segments to the shared border, bent sideways by `MathUtil.rand` draws at `seed + 20107`, `9803` and
`28387` with `MathUtil.hash(seed + 827614, x, y)`, plus a third on to the neighbour when its `noise` field is below the
threshold. A source cell also gets a lake when its hash's draw (`hash + 37171`) is at most `lakeDensity` or its base
noise is below 0.25. All seed and coordinate offsets wrap like Java `int`s. Version check (`version/compare.sh` in
`.local/terraforged/oracles/river_network/`, compiling the newer source's continent package with stubs and comparing
`javap` per method): `MathUtil` and the arithmetic are the same as in the 1.18.2-0.3.1 jar; only the seed differs. The
jar's generator draws from `config.rivers.seed` and reads cells without a runtime seed; the newer source passes its
runtime seed to both and never reads `rivers.seed` (which, in the newer source, only seeds the carver's erosion at
`rivers.seed + 21221`). So the jar computes a newer-source world of runtime seed `s` when given shape seeds
`seed0 + s`, `seed1 + s` and `rivers.seed = s`. `RiverNetworkOracle.java` runs both the jar's and the compiled
source's private `computeNodes` that way (and checks they agree) over both cell shapes and all three noise sources: a
13×13 grid at seed 42, nine runtime seeds near the `int` offsets' wrap points with three shape-seed pairs, cells at
the `int` extremes, and edge lake densities, thresholds and jitters (NaN, infinities, signed zeros). The tests match
the piece counts and the hash of every node's float bits, in order, in debug and release. The tests run the port's
cell cache (cache wiring, below); the oracle bypasses the continent's cell cache: the jar's cache returns evicted cell points to a pool that later cells overwrite, which could
change a cell still in use when two of the 21 cells read share a slot (not in these grids, on a 12-core run).

`RiverNetwork::sample_nodes` ports the nearest-node sampling of the same class: the public `sample(seed, x, y,
NoiseSample)`'s `localRiverSample.get().reset()` and the private `sample(seed, x, y, CarverSample)` it calls, between
the river warp before it and the carver call after it (`RiverNetwork::sample`, below). It takes the nearest cell of the unadjusted point (cell
generator, above), then adjusts the point for the cell shape, and visits the 3×3 cells around that centre, rows
outer, computing each cell's pieces. `sampleNode` keeps, separately for rivers and for lakes, the node whose squared
distance at its projection is strictly below the best so far (starting from `Float.MAX_VALUE`): the first of equal
distances stays and a NaN distance never wins. `recordNode` turns the winner's distance into its square root (`(float)
Math.sqrt`), its radius at the projection into `position` and the shape's base noise of its height there into
`level`. Without a winner the sample is invalidated (distance NaN, the other fields at their reset values). The window
follows Java's `int` loop bounds: it is empty where `centre ± 1` wraps and never ends where its upper bound is
`Integer.MAX_VALUE` (a panic here); the cell generator's centres are (0, 0) for huge points and otherwise stay over a
hundred cells away from the `int` limits, so neither happens with it. Version check: `version/compare.sh` also diffs
`sample`, `sampleNode`, `recordNode` and `getNodes`; `sampleNode` and `recordNode` are the same in the jar and the newer
source, and `sample` and `getNodes` differ only in the seed (the source passes its runtime seed to `getNearestCell` and
`getNodes`), so the seed mapping above holds here too. The newer source's piece cache is keyed by the cell alone, so a
generator called with two runtime seeds can return the other seed's pieces; the port's caches also check the seed
(cache wiring, below). `RiverSamplingOracle.java` (`run-sampling.sh`, same folder) runs the jar's and the compiled
source's private `sample` with both caches bypassed (both always compute) and checks they agree; with the real caches
(`Environment.CORES` fixed at 6, one thread) the output is the same, except on its `shared` lines. It covers a 25×25 quarter-cell grid
at seed 42 over both cell shapes and all three noise sources, the nine runtime seeds with three shape-seed pairs, and
26×26 edge points (NaN, infinities, signed zeros, subnormals, float extremes, ±2^31 and its float neighbours). It also
runs `sampleNode` and `recordNode` on eleven hand-made node lists (equal and mirrored nodes, a zero-length node, NaN
and infinite fields, no node) at 15×15 edge points, folding the winner's index. The tests match the four fields of
both samples and the number of invalid samples, in debug and release; stub continents check the window's `int` bounds.

`RiverNetwork::sample(seed, x, y, sample, carver, terrains)` is the newer source's public `RiverGenerator.sample`:
the river warp, the nearest-node sampling at the warped point, then `RiverCarver::carve` there. The warp is Noise2D
2cb4843's `Domain.warp` of two one-octave simplex sources at frequency 30 (seeds 8657124 and 5123678, read
seed-first) by a constant 0.004: `DomainWarp` maps each 0..1 source to -0.5..0.5 with a `Map` that is `-0.5 + value`
bit for bit, so each axis adds `(value - 0.5) * 0.004f`, both read at the unwarped point (`RiverNetwork::warp`). The
carver is passed in with the world registry its river and lake terrains come from; Java's `RiverGenerator` owns its
carver and a thread-local carver sample. Where `recordNode` maps a node's height through the shape's `getBaseNoise`,
the newer source calls Noise2D 2cb4843's `NoiseUtil.map` (`map_current`, below), which gives 1 where the base falloffs
are equal or infinitely apart; the jar's `map`, which `ShapeGenerator::base_noise` keeps, gives NaN there. The other
`NoiseUtil.map` calls of the river package (`RiverCarver`, `RiverNode`, the network's warps and radii) have constant,
finite, nonzero ranges, where both maps agree. Compared with the jar under the seed mapping above, the source differs
in two more places: its erosion seed is `rivers.seed + 21221 + seed` where the jar's is `rivers.seed + 21221`, and
those degenerate falloffs. `RiverPipelineOracle.java` (`.local/terraforged/oracles/river_pipeline/run.sh`) runs the
source's `RiverGenerator.sample`, compiled against Noise2D 2cb4843, with the caches bypassed (and once more with the real caches, below), and the jar's, which
agrees wherever those differences don't apply. It covers both cell shapes and all three noise sources, each with: a
25×25 quarter-cell grid over the shape's sample at two noise levels, 150 hand-set samples (continent, base, height
and river noise) at 49 points, and 26×26 edge points with their warped points. It also covers eleven runtime seeds
(the `int` extremes and the warp and network offsets' wrap points) with three shape-seed pairs, four degenerate
base-falloff pairs, and six `rivers.seed`s around the erosion offset's wrap. The test matches every sample's four
noises and its terrain (by identity) bit for bit, in debug and release.

The seed-first noise sources (`Source::value_seed_first(seed, x, y)`) port the newer source tree's noise API:
TerraForged's newer source samples with `Module.getValue(int seed, float x, float y)`, which comes from Noise2D commit
2cb4843 (the jar's shaded Noise2D binds the seed when the module is built and samples with `getValue(x, y)`). A
`NoiseSource` samples with its own seed plus the call's seed (`int` wrap), so seed `s` gives the same bits as the jar's
source built with seed `config + s`. `PerlinNoise` and `PerlinNoise2` override that `getValue`, ignore the call's seed and
keep their own. The one other difference: the newer `NoiseUtil.map` returns 1 where `dif >= range` (the jar: `dif >
range`), so a zero range (0 octaves) or an infinite one (a huge gain) gives 1 instead of the jar's NaN; the seed-first
path uses that `map` (`map_current`), and `Source::value`/`value_seeded` keep the jar's. Reseeding (`Module::with_seed`,
above) stays the jar's mechanism: a reseeded source gives the seed-first bits for the same seed where the two maps
agree, except for Perlin and Perlin2 (reseeding changes their seed; seed-first sampling does not). The eight octave sources,
the scalar graph nodes, the domain warps, the five selectors, `Cache`, `Terrace`, `LegacyTerrace`, `AdvancedTerrace`, the curves, `Steps`/`Freq`/`Grad`/`Modulate`, `Boost`, the cell, cell-edge and sine sources, `Line` and Engine's erosion `Ridge` (below) are seed-first; only custom modules are not (their closure has no seed parameter), unless built with `Module::seeded_custom` (the cave distributor). `SeedFirstOracle.java` and `JarSeedOracle.java`
(`.local/terraforged/oracles/seed_first/`, outside Git; `run.sh` compiles Noise2D 2cb4843 from source) fold every
kind over seven configs (0 octaves, negative gain and lacunarity, an infinite range), five config seeds, eight runtime
seeds (both `int` extremes) and ten points (NaN, infinity, -0); the tests match both tables bit for bit in debug and
release, through `value_seed_first` and through the seedless API.

Seed-first noise graphs (`Module::try_value_seed_first(seed, x, y)`) start with Noise2D 2cb4843's scalar nodes: constants,
the octave sources, `Abs`, `Alpha`, `Bias`, `Clamp`, `Invert`, `Map`, `Power`, `Scale`, `Threshold` and the five combiners
(`Add`, `Multiply`, `Max`, `Min`, `Sub`). Each passes the call's seed unchanged to its children (only the octave sources
add their own seed), and their arithmetic is the jar's: in 2cb4843 these classes behave as the jar's apart from the seed
parameter. None of them calls `NoiseUtil.map` (`Map` divides by the source range fixed in its constructor), so the newer
map only shows through the sources (a 0-octave source inside a graph gives 1, not NaN). A custom module (the only node left
unported; later layers below add the rest) returns an error instead of falling back to the seedless `value`, which
would ignore the seed. The jar path
(`value`) and reseeding (`with_seed`) are unchanged. Reseeding gives the same bits only where it seeds the same sources:
not through Perlin/Perlin2, a `Bias`'s bias (reseeding drops it) a combiner's sources (reseeding leaves them
unseeded) or a zero or infinite source range (where the two maps differ). `TerraForgedSeededGraphOracle.java` (15 graphs, four seeds, four points) and `SeededGraphEdgeOracle.java` (23
graphs over zero and infinite source ranges, Invert's out-of-bound branches, a zero-range `Map`, crossed `Clamp` bounds,
`Math.pow` special cases, empty and single-source combiners and deep nesting; seven seeds including both `int` extremes;
nine points with NaN, infinity and -0) run on the 2cb4843 classes (`.local/terraforged/oracles/seed_first/run-graphs.sh`,
outside Git, after `run.sh`); the tests match both bit for bit in debug and release.

Seed-first domain warps (`Domain::try_offset_x_seed_first`/`try_offset_y_seed_first`/`try_x_seed_first`/
`try_y_seed_first`, and the `Warp` module in `Module::try_value_seed_first`) port Noise2D 2cb4843's
`getOffsetX(int seed, float x, float y)` and friends for all six domains (`Domain.DIRECT`, `DomainWarp`,
`DirectionWarp`, `AddWarp`, `CompoundWarp`, `CumulativeWarp`). Each passes the call's seed unchanged to its modules and
nested domains; both axes read the unwarped point; `Warp` samples its source at `(getX, getY)`; `DirectionWarp` uses the
same 4096-entry sine table and `cos(r) = sin(r + 1.5708f)`; `CumulativeWarp` keeps Engine's quirk of returning the second
warp's coordinate as the offset. In 2cb4843 these classes equal the jar's apart from the seed parameter, and none calls
`NoiseUtil.map` (`DomainWarp` maps its axes with a `Map` built at construction), so the newer map shows only through the
sources. An unsupported module anywhere inside returns its error. The seedless `offset_x`/`x` and reseeding are
unchanged (reseeding still panics for `Domain.DIRECT`, `AddWarp` and `CompoundWarp`, whose specs Noise2D registers under
`CumulativeWarp`'s name). `TerraForgedSeededDomainOracle.java` (seven domains, four seeds, four points, five outputs) and
`SeededDomainEdgeOracle.java` (16 domains: Perlin axes, zero- and infinite-range axes, an axis already at -0.5..0.5, a
constant axis, NaN and huge angles, `Direct` inside combinations, a six-deep chain and warps of warps; seven seeds
including both `int` extremes; nine points with NaN, infinity and -0; seven outputs each) run in the same
`run-graphs.sh`; the tests match both bit for bit in debug and release. The river network's and `ContinentNoise`'s
inlined warps equal this generic path bit for bit. The seed/scale shorthands (`Domain.warp(seed, scale, octaves,
strength)`...) are still not ported.

Seed-first selectors (`Base`, `Blend` and `VariableBlend` in `Module::try_value_seed_first`) port Noise2D 2cb4843's
`Selector.getValue(int seed, float x, float y)`: the control is sampled first with the call's seed, then `selectValue`
passes the same seed to the children it evaluates (`VariableBlend` samples its variator after the control). The window
is the jar's, fixed in the constructor from the stored arguments: `Base` returns its source (the control) from
`base.maxValue() + falloff` up and below that blends toward the base, which it reaches at `base.maxValue()` (or returns
at once when `falloff` is not positive, NaN included); `Blend` uses
`max(control min, mid - blendRange / 2)` to `min(control max, mid + blendRange / 2)`, where `mid` lies `midpoint` of the
way through the control's bounds; `VariableBlend` computes `minBlend + variator * maxBlend` per sample and clamps its
window to 0..1. The `Linear`, `Curve3` and `Curve4` interpolations and `NoiseUtil.lerp` are the jar's, and none of these
classes calls `NoiseUtil.map`. As in Java, only the selected child is evaluated outside the window, both inside it; a
NaN control (or a NaN window) falls through to the blend with a NaN alpha (`Base` returns its control instead), and a
zero-width window gives NaN (0/0) when the control sits on it. So an unsupported node in a skipped branch returns its error only at the points that reach it.
`TerraForgedSeededBlendOracle.java` (three selectors, three interpolations, four seeds, four points) and
`SeededBlendEdgeOracle.java` (27 families with the control at and one ulp beside each window edge, NaN, ±inf and ±0;
zero, negative, NaN and infinite blend ranges and falloffs; NaN, ±inf, zero and seeded variators; windows clamped to
0..1; nested selectors; seven seeds including both `int` extremes; six points with NaN, infinity and -0) run in the same
`run-graphs.sh`; the tests match both bit for bit in debug and release.

`Select` and `MultiBlend` follow the same pattern (control first, then the children Java evaluates, all with the call's
seed). `Select` reads its four curve edges (`bound ∓ falloff`) and their ranges from the constructor; a zero (or -0)
falloff takes the hard edge (below `lowerBound` the lower source, else above `upperBound` the upper one, else the lower
source; reversed bounds test the lower bound first, so only controls at or above it reach the upper source), and a NaN
control or falloff falls through to the lower source.
`MultiBlend` keeps the constructor's slots: `NoiseUtil.round(selector * maxIndex)` picks the slot, a zero `blendRange`
returns its source, otherwise a control above (below) the slot blends with the next (previous) slot, alpha clamped to
0..1 by `NoiseUtil.clamp`. These slots come from the constructor's float blend; only reseeding cuts it to an `int`.
A control far enough outside 0..1 to pick a missing slot panics where Java throws. Neither class calls `NoiseUtil.map`.
`TerraForgedSeededSelectionOracle.java` (five graphs, three interpolations, four seeds, four points, plus nine controls
across one `Select` window) and `SeededSelectionEdgeOracle.java` (28 families with the control at and one ulp beside
every curve, slot and rounding edge, NaN, ±inf, ±0 and values outside 0..1; zero, -0, negative, NaN and infinite
falloffs and blends; reversed, equal, overlapping, NaN and infinite bounds; one to four sources; nested selectors; seven
seeds including both `int` extremes; six points with NaN, infinity and -0; Java's exceptions where the port panics) run
in the same `run-graphs.sh`; the tests match both bit for bit in debug and release.

`Cache` (`Module.cache()`) is seed-first too, with one difference from Noise2D 2cb4843 so that a cache never changes a
result. 2cb4843's `Cache` keeps a `ThreadLocal` entry per thread and hits on `x == this.x && y == this.y` alone: the
seed is not part of the key, so a repeated point at another runtime seed returns the first seed's sample (2cb4843 is
seed-unsafe there). The port keys its entry
by the runtime seed and the coordinates' bits, like the seedless path (above): a hit is always the sample the uncached
graph gives, a `-0.0` point does not hit `+0.0`'s entry, and a NaN point can hit its own (where Java always misses and
samples again). Seed-first and seedless samples never read each other's entry (their `NoiseUtil.map`s differ), a
failing child stores nothing, and the entry is still one shared value behind a lock that is only tried (bounded,
threads never wait). `TerraForgedSeededCacheOracle.java` (`.local/terraforged/oracles/seed_first/`, outside Git, run by
`run-graphs.sh`: six cached graphs, shared and nested caches included, five seeds, eight points each sampled twice in a row, on one thread with fresh caches,
plus the coordinate-only hits that return another seed's sample) gives what Java shows deterministically; the tests
match it bit for bit in debug and release, return the uncached sample where 2cb4843 returns another seed's, and check
cached = uncached on 8 threads interleaving five seeds over one cache, and that the seedless `value` and `with_seed`
keep their bits.

`Terrace` (`Module.terrace(ramp, cliff, rampHeight, steps, blendRange)`) is seed-first too: Noise2D 2cb4843's class is
the jar's apart from the seed parameter. The source is sampled with the call's seed, clamped to 0..0.999999 and
indexed with `NoiseUtil.floor(value * steps)` into the step table fixed in the constructor (the same table the seedless
path uses; reseeding rebuilds it from the reseeded source's bounds, which are the same). The last step and samples
outside a step's window return a step value without reading any child; inside the window the ramp and then the cliff
are sampled with the call's seed, and the ramp height only when the sample lies above the ramp. No `NoiseUtil.map` is
involved. A NaN sample or a terrace without steps panics where Java throws; one step gives NaN (`0 * (range / 0)`).
`TerraForgedSeededTerraceOracle.java` (the PR's terrace over seeded Simplex/Perlin2/Simplex2/Perlin sources, four
seeds, four points, plus ten boundary samples) and `SeededTerraceEdgeOracle.java` (24 families with the source at and
one ulp beside every step value, window bound and index edge `k / steps`, plus NaN, ±inf, ±0 and subnormals; 0, 1, 2,
5, 7, 64 and 1000 steps; zero, one, inverted, negative and NaN blend ranges; ramps and cliffs pinned to 0 or 1, NaN
and infinite ramp heights; zero, wide, narrow and infinite source ranges; nested terraces and terraces of seeded
sources; seven seeds including both `int` extremes; six points with NaN, infinity and -0) run in the same
`run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the tests match both bit for bit in debug and
release, check which children each sample reads, and that reseeding gives the same bits over non-Perlin sources.

`LegacyTerrace` (`Module.legacyTerrace(lowerCurve, upperCurve, steps, blendRange)`) is seed-first too: Noise2D
2cb4843's class is the jar's apart from the seed parameter. The source is sampled with the call's seed, clamped to 0..1
and rounded (`NoiseUtil.round(value * maxIndex)`) to an index into the step table fixed in the constructor (shared with
the seedless path). Inside a step's window, and below the first step's window or above the last's, it returns the step value
without reading a curve; below the window it eases down from the previous step through Curve3 scaled by `upperCurve`,
above it up to the next step scaled by `lowerCurve`, each curve sampled with the call's seed. As in Java, a sample
below the first step's window returns that step without checking the upper window (a blend range above 1 inverts
the windows). No `NoiseUtil.map` is involved. A NaN sample rounds to the first step; a legacy terrace without steps
panics where Java throws. `TerraForgedSeededLegacyTerraceOracle.java` (the PR's legacy terrace over seeded
Simplex/Perlin2/Simplex2 sources, four seeds, four points) and `SeededLegacyTerraceEdgeOracle.java` (24 families with
the source at and one ulp beside every step value, window bound and rounding edge `(k + 0.5) / maxIndex`, plus NaN,
±inf, ±0 and subnormals; 0, 1, 2, 5, 7, 64 and 1000 steps; zero, one, inverted, negative and NaN blend ranges; zero,
NaN and infinite curves; zero, wide, narrow and infinite source ranges; nested legacy terraces and legacy terraces of
seeded sources; seven seeds including both `int` extremes; six points with NaN, infinity and -0) run in the same
`run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the tests match both bit for bit in debug and
release, check which curve each sample reads, and that reseeding gives the same bits over non-Perlin sources.

`AdvancedTerrace` (`Module.terrace(modulation, mask, slope, blendMin, blendMax, steps, octaves)`) is seed-first too:
Noise2D 2cb4843's class is the jar's apart from the seed parameter, and one helper serves both paths. The source is
sampled with the call's seed; at or below `blendMin` (not NaN) it is returned without reading another child. Otherwise
the mask is read; a zero mask (either sign) returns the source sample without reading the slope or the modulation.
Then the slope and the modulation are read, in that order, all with the call's seed, and the sample is stepped and
sloped once per octave with `steps * i` steps (Java `int` wrap), modulated by the constructor's `modRange` and blended
in as on the seedless path. No `NoiseUtil.map` is involved, and Java throws nowhere. Java's octave loop never ends for
`octaves == Integer.MAX_VALUE`; the port's stops (as on the seedless path). `TerraForgedSeededAdvancedTerraceOracle.java`
(the PR's advanced terrace over seeded Perlin/Simplex/Ridge/Perlin sources, four seeds, four points) and
`SeededAdvancedTerraceEdgeOracle.java` (30 families with the source at and one ulp beside `blendMin`, `blendMax` and
every step value and rounding edge, plus NaN, ±inf, ±0 and subnormals; masks 1, 0.5, ±0, -1, NaN and infinity; slopes
0, 1, -0.5 and NaN; 0, 1, negative and wrapping step counts; 0, -1, 1 and 3 octaves; equal, inverted, NaN and infinite
blend bounds; zero, infinite and NaN `modRange`; seeded children; nested advanced terraces and advanced terraces of
seeded sources; seven seeds including both `int` extremes; six points with NaN, infinity and -0) run in the same
`run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the tests match both bit for bit in debug and
release, check which children each sample reads, and that reseeding gives the same bits over non-Perlin sources.

The curve modifiers `Curve` (`Module.curve(CurveFunc)`: an `Interpolation`, a `MidPointCurve` or an `SCurve`),
`VariableCurve` (`Module.curve(mid, steepness)` with noise controls) and `PowerCurve` (`Module.powCurve(n)`) are
seed-first too: Noise2D 2cb4843's classes are the jar's apart from the seed parameter (and the `NoiseUtil.map` one of
them calls). The source is sampled with the
call's seed, then a `VariableCurve` reads its midpoint and then its steepness module with the same seed (Java reads
all three, with no early exit); `Curve` and `PowerCurve` read only their source. The curve functions are the seedless
path's (`NoiseUtil.curve` over `NoiseUtil.exp`, `SCurve`'s `(float) Math.pow` through `java_pow`), and `PowerCurve`
keeps the bounds, midpoint and range its constructor fixed from the source's bounds (one helper serves both paths).
`PowerCurve` is the one curve that calls `NoiseUtil.map`, so the seed-first path maps with `map_current`: where the
curve's range is zero (a constant source) or infinite it gives 1, the jar's path NaN. Java throws nowhere here; NaN
samples, controls and parameters give NaN as in Java. Reseeding still panics on a `MidPointCurve` or `SCurve` (Noise2D
2cb4843's Cereal specs cannot read them back either), but seed-first sampling needs no reseeding.
`TerraForgedSeededCurveOracle.java` (the PR's four curves over seeded Perlin/Simplex/Ridge sources, four seeds, four
points) and `SeededCurveEdgeOracle.java` (44 families with the source at and one ulp beside every midpoint,
`NoiseUtil.exp` zero-base point, `SCurve` zero-power point and `PowerCurve` source bound and midpoint, plus NaN, ±inf,
±0, subnormals and ±1e30; zero, negative, infinite and NaN steepness and midpoints; `SCurve`s with a raised, NaN,
infinite or signed-zero `upper`; all three interpolations; NaN, infinite and seeded variable-curve controls; powers
0.8, 2, 0, -1, NaN, inf, 0.5, 1 and 3 over zero, infinite, inverted, NaN and wide bounds; power curves of seeded
Perlin, 0-octave and infinite-range sources; nested curves; seven seeds including both `int` extremes; six points with
NaN, infinity and -0) run in the same `run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the
tests match both bit for bit in debug and release, check which children each sample reads, and that reseeding gives
the same bits over non-Perlin sources.

`Steps` (`Module.steps(steps, slopeMin, slopeMax, curve)`), `Freq` (`Module.freq(x, y)`), `Grad`
(`Module.grad(lower, upper, strength)`) and `Modulate` (`Module.mod(direction, strength)`) are seed-first too: Noise2D
2cb4843's classes are the jar's apart from the seed parameter (and `Steps`'s `NoiseUtil.map`), so both paths share one
helper. Every child is sampled with the call's seed, in Java's order: `Steps` reads its source, slope min, slope max and
step count (a zero or negative slope range rounds the sample down to a step, `(int)` saturating like
Java's); `Freq` reads its x and y frequencies at the original point, then the source at `(x * fx, y * fy)`; `Grad` reads
its source and upper bound and, unless the source sample is above the upper bound, its strength and lower bound (`Math.pow` through
`java_pow`); `Modulate` reads its direction and strength, then the source at the point moved by `NoiseUtil.sin`/`cos` of
`direction * PI2` times the strength. `Steps` is the one that calls `NoiseUtil.map`, so the seed-first path maps with
`map_current`: an infinite slope range gives 1 there, the jar's path NaN. Java throws nowhere here; NaN controls give NaN
as in Java (a `0 * inf` offset moves the point to NaN). `TerraForgedSeededTransformOracle.java`
(the PR's four graphs over seeded Perlin/Simplex/Ridge/Perlin2 sources, four seeds, four points) and
`SeededTransformEdgeOracle.java` (43 families with a fixed value at and one ulp beside every step height, slope-window
edge, gradient bound and quarter turn, plus NaN, ±inf, ±0, subnormals and ±1e30; zero, negative, infinite and NaN slope
ranges; zero, NaN, infinite, negative, fractional and `int`-saturating step counts; equal, inverted, NaN and infinite
gradient bounds; NaN, infinite and power-raising strengths; zero and infinite modulation strengths; seeded and 0-octave
children; a nested graph of all four; seven seeds including both `int` extremes; six points with NaN, infinity and -0)
run in the same `run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the tests match both bit for
bit in debug and release, check which children each sample reads, and that reseeding gives the same bits over
non-Perlin sources.

`Boost` (`Module.boost(n)`) and the cell, cell-edge and sine sources (`Builder.cell()`, `cellEdge()`, `sin()`) are
seed-first too: Noise2D 2cb4843's classes are the jar's apart from the seed parameter (and `NoiseUtil.map`), so both
paths share one helper. `CellNoise` and `CellEdgeNoise` are `NoiseSource`s: they search the 3×3 cells around the
scaled point with their own seed plus the call's (`int` wrap), hashing and jittering the cells as the seedless path
does (`NoiseUtil.floor`, `NoiseUtil.cell`, Java `int` wrap where the window passes `Integer.MAX_VALUE`). `CELL_VALUE`
maps the nearest cell's value from -1..1, `CellEdgeNoise` its edge function from the function's bounds, and `Sin` its
sine from -1..1, all with 2cb4843's `NoiseUtil.map` (`map_current`; these ranges are fixed and finite, so the bits
equal the jar's map). `NOISE_LOOKUP` samples its lookup, unmapped, at the nearest cell's feature point with the cell's
seed (so a seeded lookup adds its own seed on top), and panics where Java throws a `NullPointerException` (no cell
nearer than `Float.MAX_VALUE`: NaN or infinite coordinates or displacements); `DISTANCE` always gives 0. `Sin` is a
`NoiseSource` in 2cb4843: it reads its alpha at the original point with its own seed plus the call's, then scales the
point. The port's `Sin` keeps no seed (Engine's `Sin` spec has none), so it uses the builder default 1337, as a `Sin`
built without `seed(...)` or read back from its spec. `Boost` passes the call's seed to its source (mapped to 0..1 at
construction) and raises it `iterations` times with `NoiseUtil.pow` (`Math.pow`, through `java_pow`). Reseeding gives
the same bits for cells and edges of displacement 1 (reseeding resets it) and boosts of non-Perlin sources.
`TerraForgedSeededSpecialSourceOracle.java` (the PR's five graphs, four seeds, four points) and
`SeededSpecialSourceEdgeOracle.java` (seven families of a fixed value as `Boost`'s source, `Sin`'s alpha or a cell
lookup, at and one ulp beside every table value plus NaN, ±inf, ±0, subnormals and ±1e30; 30 graphs of every cell
function, edge function and distance function with zero, NaN and infinite displacements and frequencies, config seeds
at both `int` extremes, seeded, Perlin, 0-octave and nested lookups and alphas, over seven seeds including both `int`
extremes and 20 points at and beside cell edges, huge, NaN, infinite and signed-zero coordinates; Java's exceptions
where the port panics) run in the same `run-graphs.sh` (`.local/terraforged/oracles/seed_first/`, outside Git); the
tests match both bit for bit in debug and release.

`Line` (`Source.line(...)`) is seed-first too: Noise2D 2cb4843's class is the jar's apart from the seed parameter
(it has no seed of its own and calls no `NoiseUtil.map`), so both paths share one helper and the seedless bits are
unchanged. It passes the call's seed unchanged to its radius and fade modules. As in Java, it reads the fades only
away from the endpoints (an endpoint's width modifier is 0 without them), fade-in before fade-out, then the radius at
every point; the segment distance is the jar's f32 arithmetic (projection clamped with `NoiseUtil.clamp`, no square
root): a zero-length segment samples NaN everywhere (0/0), one whose squared length underflows to 0 samples NaN
where its projection is 0/0 and measures to an endpoint elsewhere (±inf clamps to 0 or 1), and the endpoints sample
NaN (an endpoint's width modifier, and so its radius, is 0). Reseeding differs here: the jar's `Line` spec reads its children back without the seed, so `with_seed`
keeps their seeds while seed-first sampling reaches them. `TerraForgedSeededLineOracle.java` (the PR's line, four
seeds, four points) and `SeededLineEdgeOracle.java` (21 lines: seeded, Perlin and 0-octave children, NaN, infinite,
zero and negative radii and fades, feathers 0, 1 and NaN, zero-length, underflowing and overflowing segments, a nested
line and a wide seeded radius, over seven seeds including both `int` extremes and 38 points: endpoints, projections at,
one ulp beside and past 0 and 1 on and off the segment, NaN, ±inf, ±0, `Float.MAX_VALUE` and subnormal points) run in
the same `run-graphs.sh`; the tests match both bit for bit in debug and release.

Engine's erosion `Ridge` (`Valley`, `ErosionRidge::wrap`) is seed-first too, and with it every node but custom modules
(a custom module's closure has no seed parameter, so it still returns an error, unless it is a `Module::seeded_custom`). There is no Java reference for this
one: Engine 0.3.0's `Ridge.Noise` implements only the jar's `getValue(x, y)` (on the 2cb4843 classes it fails at run time),
and no seed-first Engine is public. The port follows Noise2D 2cb4843's convention for seeded nodes: the
cell lattice (`NoiseUtil.cell`) uses the ridge's own seed plus the call's seed (`int` wrap), and the source is read
with the call's seed (the centre sample first, then each octave's 5x5 neighbourhood through the `-1`-filled cache, as
Engine does). The octave loop, gain, lacunarity, fall-off, segment distance (`(float) Math.sqrt` in f64) and blend
modes are Engine's, shared with the seedless path by one helper; no `NoiseUtil.map` is involved, so the newer map shows
only through the sources. So seed-first at seed `s` equals the jar's ridge built with seed `own + s` over sources built
with their seeds plus `s` wherever those agree (non-Perlin sources with finite, nonzero ranges); a test checks that
against the oracle-tested seedless ridge. Reseeding differs: the `Valley` spec reads its seed and source back without
the context, so `with_seed` leaves the ridge's bits unchanged. `SeededErosionRidgeEdgeOracle.java` (Engine 0.3.0's
`Ridge` transcribed onto the 2cb4843 classes; 31 families: 0, 1, many and negative octaves, zero, negative, NaN and
infinite amplitudes, lacunarities, fall-offs, grid sizes and strengths, a 1e30 lacunarity, Perlin, 0-octave, constant
-1 and NaN sources, a nested ridge and own seeds at both `int` extremes, over seven seeds and 15 points with NaN, ±inf,
±0, `Float.MAX_VALUE`, subnormals and grid-cell edges) runs in the same `run-graphs.sh`; the tests match it bit for bit
in debug and release. The river carver's erosion (#125) is a `RidgeNoise` source, not this node; a test checks that
its inline build equals the generic seed-first graph path.

The bounded concurrent lossy cache (`lossy_cache.rs`, `ConcurrentLossyCache`) ports the newer source tree's
`LossyCache.Concurrent` and its `LossyCache.Stamped` buckets (`com.terraforged.mod.util.storage`, 6dd607eb). The
1.18.2-0.3.1 jar has the same class in `util.map` without the seed (`computeIfAbsent(long, Long2ObjectFunction)`) and
without the source's re-check under the write lock (two threads missing one key both compute it); its slot math and
single-threaded behaviour are the source's. The structure is the source's: `smallestEncompassingPowerOfTwo(concurrency)`
buckets (`Environment.CORES`, Java's `availableProcessors()`, for `concurrent(capacity)`), each with
`smallestEncompassingPowerOfTwo(NoiseUtil.floor((float) capacity / buckets))` slots; a key picks its bucket with
`(int) (key ^ key >>> 16) & 0x7fffffff` and its slot with fastutil's `HashCommon.mix`, and a miss computes
`function(seed, key)` under the bucket's write lock (as Java does), overwrites the slot and passes the replaced value to
the removal listener after the lock is released. It differs from Java in three ways, so that a value read through the
cache always equals the one computed directly, whatever the seeds, threads or core count:

- **Hits need the seed too.** Java's `LongCache` matches the `long` key alone, so a slot computed for one seed answers
  every other seed (the source's river-piece cache, keyed by cell, would hand one world's pieces to another). The port
  stores the seed in the slot and treats a different seed as a miss in the same slot. With one seed, which is how the
  source uses it, the hits, misses, eviction order and listener calls are Java's.
- **No recycling of values in use.** Values are shared (`Arc`); an evicted value stays valid for everyone holding it,
  and the listener gets the evicted `Arc`, so it can take a value back only once no one else holds it. TerraForged's
  cell and river caches hand evicted values to an `ObjectPool`, and `Stamped` calls the listener with the value it read
  before locking even when it then returns another thread's newer value or the function throws, so a value can go back
  to the pool twice or while it is still cached, depending on how threads interleave. In 6dd607eb that is harmless only
  because `ObjectPool.take` always builds a new value; a pool that reused them would alias values still in use. The port
  calls the listener only for values it evicted and ports no pool for these caches. Hits take a read lock rather than
  Java's optimistic read; a miss checks the slot again under the write lock, as Java does when its lock conversion
  fails.
- **Unusable sizes panic at construction**: no buckets or buckets of no slots (Java builds them, then throws
  `ArrayIndexOutOfBoundsException` on the first lookup) and counts past 2^30 (Java's constructor throws). The plain
  single-threaded `LossyCache.of`, which nothing in the source uses, is not ported.

`LossyCacheOracle.java` (`.local/terraforged/oracles/lossy_cache/`, outside Git; `run.sh` compiles the source's
`LossyCache`, `LongCache` and `LockUtil` with an `Environment` stub fixing `CORES` at 6 and runs them interpreted and
JIT, then the jar's seedless class, whose output is the same) checks nine capacity/concurrency pairs (one slot to 256
slots per bucket, 1 to 64 buckets, non-power-of-two sizes): the bucket and slot counts, the bucket and slot of 10 to 10,245 keys each (the `long` extremes, keys `i << 16`
and `i << 32`, negatives), and per pair three single-threaded traces of 4 lookups per key (seeds 0, 12345,
`Integer.MIN_VALUE`) recording every factory call, listener call and result; the `CORES` factory gives the same traces
as concurrency 6. The test replays them bit for bit. Rust tests cover what Java can't show deterministically: 16 threads
on 48 colliding keys and three seeds always get the directly computed value, values held across evictions stay intact,
one key under contention is computed once, a panicking function leaves the slot and the listener alone, and the
listener runs outside the lock.

The cache wiring follows 6dd607eb. `CellGenerator`'s `cell` (`ContinentGenerator.getCell`, used by the nearest-cell
search, the shape's 5×5 blend and the river network) reads a 2,048-cell cache (`CELL_POINT_CACHE_SIZE`), and
`RiverNetwork::nodes` (`RiverGenerator.getNodes`, used by the nearest-node sampling) a 1,024-cell piece cache
(`RIVER_CACHE_SIZE`), both `LossyCache.concurrent` with `Environment.CORES` buckets and keyed by `PosUtil.pack(x,
y)`; `with_concurrency` fixes the bucket count. `compute_cell`, `compute_nodes` and the spawn search stay direct, as in
the source. Two differences, so that the caches never change a result: a hit needs the runtime seed too (the source's
caches hit on the cell alone, so one generator called with two runtime seeds returns the first seed's cells and pieces
for the second), and no cell point or piece list is pooled (the source's pools build a new value on every `take`, so
that changes nothing). Evicted values are shared `Arc`s, so a sample holding pieces never sees them change. Per sample
the caches save most of the work: without them, `sample_nodes` read the nearest-cell search's 9 cells and then
computed the pieces of the 9 cells around it (each a new list, reading the cell, its neighbours and, for land, theirs);
on a 9×9 quarter-cell grid at seed 42 that is 5,810 cell reads for 81 samples, against 895 on a first cached pass and
exactly 9 per sample (all hits, no pieces computed) once the neighbourhood is cached. Checks: the river oracles above
(`run-sampling.sh`, `river_pipeline/run.sh`) also run the source with its real caches, `CORES` fixed at 6 by an
`Environment` stub, on one thread, and get the direct output; their `shared` lines reuse one generator across the
runtime seeds of the `seeds` lines, where the source's output changes (its caches leak one seed into the next) and the
direct one doesn't. The Rust tests run the cached path (6 buckets) against all of these lines, `shared` included
(equal to `seeds`), and compare the cached cells, nearest cells, pieces and samples with a fully direct computation
over 64 and 16 runtime seeds on 8 threads sharing one generator.

`ContinentNoise` (`continent_noise.rs`) ports the newer source's `ContinentNoise` (6dd607eb), the continent module that
joins these layers. Its constructor draws `seed0`, `seed1` (`createContinent`) and the two warp seeds from the context's
`Seed`, in that order, and builds the cell generator, the shape, the river network and the river carver from one
`ContinentConfig` with `continentScale` as its scale. Both samplers scale world coordinates by `1F / continentScale`,
warp them with Noise2D 2cb4843's `Domain.warp` of two seed-first Perlin2 sources (3 octaves, lacunarity 2.2, frequency 3,
gain 0.3) by a constant 0.2 (each axis adds `(value - 0.5) * 0.2f`, as for the river warp), and add the spawn offset;
`sample_continent` then runs the shape's 5×5 blend and sets the terrain from `ContinentPoints.getTerrainType`,
`sample_river` the river pipeline. Terrains come from the context's registry (`context.terrain`), so they are the
world's built-ins. On this path the shape runs as the newer source does: its `ShapeGenerator` and `ContinentPoints`
are the jar's bytecode (with the seed passed down; `version/compare.sh` below checks it), but they call Noise2D
2cb4843's `NoiseUtil.map`, so `ShapeGenerator::sample_current`, `base_noise_current` and `falloff_current` (and
`continent_points::falloff_current`) use `map_current`: 1 instead of NaN where a range is zero or infinite (equal base
falloffs, an inland control point of 1, a zero deep-ocean point under a negative input). `sample`, `base_noise` and
`falloff` keep the jar's `map` for the jar-tested layers. The two versions differ in the spawn offset: the jar computes
`getWorldOffset()` once, in its constructor, and keeps it in a field; the newer source calls `getWorldOffset(seed)` on
every sample, which computes it for the first caller's runtime seed and then returns that one for every seed. The port
computes each runtime seed's offset on its first sample and keeps it (up to 64 seeds, then it starts over), so two
runtime seeds never share an offset; with one seed per world, as TerraForged samples, the result is the source's. The
jar's seedless samplers are not ported (they equal runtime seed 0 where the maps agree). Checks: `version/compare.sh`
(`.local/terraforged/oracles/continent_noise/`, outside Git; `methods.py` from the river-network check) finds
`ContinentPoints` identical and `ShapeGenerator` differing only in the seed, and `ContinentNoise` in the seed and the
offset's storage. `ContinentNoiseOracle.java` (`run.sh`, same folder) runs the compiled source's `ContinentNoise` with
both caches bypassed over six configs (continent scales 400, 3000, 1000, 1, -400 and 0; custom, default, invalid and
inland-1 control points; two `TerrainLevels`; `long` world seeds), each at six runtime seeds (the `int` extremes
included) on an 11×11 grid a little over a third of a cell apart, plus 26×26 edge points (NaN, infinities, signed zeros,
subnormals, float extremes, ±2^31) at runtime seed 0, folding both samples' continent, base, height and river noise
and terrain ids, the seed draws and each seed's offset; the jar's run equals the source's runtime-seed-0 lines except
for the inland-1 config, as expected. Its `shared` lines (one `ContinentNoise` at every runtime seed in turn) show the
source's first-seed offset; the test checks that the port's equal its `fresh` lines (a new one per seed). The jar's
continent-shape and continent-points oracles rerun with Noise2D 2cb4843 first on the class path give the `_current`
variants' expected values (only the zero-range and infinite-range cases change). The tests match all of it bit for bit,
in debug and release; twelve threads sampling one `ContinentNoise` at six runtime seeds at once get the same
lines, and past 64 seeds the offset store starts over without mixing seeds up.

The terrain height sampler (`noise_generator.rs`) ports the newer source tree's `NoiseGenerator` sampling (6dd607eb:
`sample`, `sampleTerrain`, `sampleRiver`, `getOcean`, `getBlend`, `getInland`). A block position is scaled by the noise
levels' frequency, the continent is sampled, and then the height depends on the continent noise. Below `SHALLOW_OCEAN`
(0.25) it is the ocean floor (`Source.simplex(8763214, 64, 3).scale(0.4)`, the runtime seed added with `int` wrap, as
`toDepthNoise`) and the terrain becomes deep ocean. Below `BEACH` (0.5) it lerps from that floor up to sea level and
keeps the continent's terrain. Below `COAST` (0.55) it lerps from sea level up to the land height, and above that it is
the land height: the seed-first terrain blender times 1.2 on the continent's base noise (`toHeightNoise`), with the
blender's terrain. NaN goes inland, as Java's float compares do. Then the rivers are carved. The deep-ocean terrain is
the continent context's registry's. A terrain noise with no seed-first form (a custom module) makes `sample` return an
error rather than sample it seedless. Building the continent and the blender (`createContinentNoise` with
`CONTINENT_OFFSET` 18749560 and its control points, `createLandTerrain` with `TERRAIN_OFFSET` 45763218) is left to the
caller, chunk generation and `find` are below, and erosion comes later. The 1.18.2-0.3.1 jar's `NoiseGenerator` samples
the same way with the seed taken from its constructor: its ocean noise is seeded with `(int) seed` itself (no 8763214 offset),
its blender and continent settings with the world seed, and its sampling methods take no seed. Compared method by
method (`.local/terraforged/oracles/noise_generator/version/compare.sh`), the two differ only in where that seed comes
from. `NoiseGeneratorOracle.java` (`.local/terraforged/oracles/noise_generator/`, `run.sh`) runs the newer source's own
`NoiseGenerator` constructor over Noise2D 2cb4843, with the continent caches bypassed and a fresh generator per runtime
seed. It covers three `TerrainLevels`, a constant terrain set and a four-terrain set (simplex, Perlin, scaled ridge and
billow noises, seeds up to `Integer.MAX_VALUE`), and seven runtime seeds (the `int` extremes, -8763214 and -21491124
included). Each case samples a 17×17 grid that reaches all four height branches (into one reused sample, as chunk generation does), 100 integer edge positions
(`Integer.MIN_VALUE`/`MAX_VALUE` included), and 144 float noise coordinates (signed zeros, subnormals, NaN and
infinity, where Java throws and the port panics). The test matches its 66 folded grid and edge lines bit for bit, in debug and
release.

Chunk generation (`NoiseGenerator::generate(seed, chunk_x, chunk_z, consumer)`, the newer source's `generate(seed,
chunkX, chunkZ, consumer)`) samples the chunk and its one-block border, an 18×18 grid from -1 to 16, `dz` outer and
`dx` inner, at `(chunkX << 4) + dx` in `int` math (so the edge chunks wrap), into one `NoiseData` and hands it to the
consumer once; `height_noise(seed, x, z)` is `getHeightNoise` (one reset sample). As in Java every position is sampled
into the data's own sample with no reset in between, so the river noise carries over from position to position: the
river carver only multiplies it (inside a valley) and only an ocean or beach position resets it to 1, so once a valley
lowers it, it stays low for the following land positions of the chunk and each further valley lowers it again. Java
keeps that `NoiseData` per thread and reuses it for the thread's next chunk, so a chunk's river noise also depends on
which chunk the same thread generated before; the port does not reproduce that (it depends on scheduling): every chunk
starts from a new `NoiseData`, which is Java's first chunk on a thread. A failing sample (a custom terrain noise)
returns an error and the consumer is not called, where Java throws out of the loop. The jar's `generate(chunkX, chunkZ,
consumer)` has the same loop with its constructor's seed (and the same carry-over; `version/compare.sh` also compares
`generate`, `getHeightNoise` and `getNoiseSample`). The generator implements the terrain generator's seed-first
`NoiseGenerator` trait with these two methods; the trait has no error, so a failing chunk or height panics there, as
Java throws. `NoiseChunkOracle.java`
(same folder, `run-chunks.sh`) runs the source's `generate` for two `TerrainLevels`, both terrain sets and four runtime
seeds (the `int` extremes included) over eight chunks (negative ones, ones crossing the `int` wrap, two in river
valleys at seed 0), each on a new thread, hashing every `NoiseData` element and the data's final sample. The test
matches its 128 chunk lines (the hash and the number of cells whose river noise differs from a reset sample's: 56
chunks have some) bit for bit, in debug and release. Run one after another on one thread, up to six of the eight
chunks come out different in Java.

The nearest-terrain search (`NoiseGenerator::find(seed, x, z, min_radius, max_radius, terrain)`, the newer source's
`find`, which TerraForged's `/locateterrain` command calls) returns the block position (`PosUtil.pack` of the
`NoiseUtil.floor`ed coordinates) of the nearest cell of the terrain's region that is on land and away from rivers, or 0
when there is none. A terrain that is not overground (oceans, rivers, lakes) is 0 at once. Otherwise the block position
is scaled by the noise frequency and the blender's seed-first search (`try_find_nearest_seed_first`, above; nothing of
it is duplicated here) walks its spiral of cells; each candidate is skipped when it is the search's miss value (0), then
divided back to blocks and skipped when its continent noise is below `BEACH` (0.5, coast or ocean) or its river noise is
below 0.75 (the source skips this second test for river terrains, but no built-in terrain is both overground and a
river). As in Java, the continent and rivers are sampled at those block coordinates without scaling them by the
frequency again (unlike `sample`), and one sample, reset once, is reused for every candidate: the river carver only
multiplies the river noise and reads the carried height, so a candidate rejected near a river lowers the river noise of
the ones after it. Where the blender has no terrain noise with the terrain's name, Java's `findNearest` returns `null`
and `find` throws a `NullPointerException`; the port returns an error there (and where the blender's warp has no
seed-first form). The jar's `find(x, z, minRadius, maxRadius, terrain)` is the same method with the seed from its
constructor (`version/compare.sh` now compares `find` too: only the seed argument of `findNearest`, `sampleContinent`
and `sampleRiver` differs). A negative minimum radius panics where Java divides by zero in the spiral.
`NoiseFindOracle.java` (same folder, `run-find.sh`) runs the source's `find` for two `TerrainLevels`, both terrain sets
and four runtime seeds (the `int` extremes included) over eight terrains (two not overground, two with no terrain
noise), seven block positions (negative ones and the `int` extremes, where the floored result saturates) and three radius
pairs (a zero minimum included): 2,688 searches, interpreted and JIT-compiled alike. The test matches its 16 folded lines
bit for bit, in debug and release, including the per-line count of searches whose result changes when every candidate
gets a fresh sample (5 to 38 per line), which shows the carried sample is reproduced.

The erosion tile geometry (`erosion.rs`, the mod's `worldgen.noise.erosion.NoiseTileSize`; the newer source's class and
the jar's have the same bytecode) sizes the square of chunks that TerraForged's eroded noise generator erodes together:
`chunk_radius` chunks on each side of the centre chunk (`NoiseGenerator.getNoiseTileSize` uses 2, `DEFAULT` is 1), its
chunk and block offset ranges, the heightmap length, and the relative chunk and block indices
(`chunkIndexOfRel`, `indexOfRel`), all in Java `int` math, so negative or huge radii give Java's wrapped values; it is
separate from Engine's power-of-two tile `Size`. The eroded generator itself (its tile cache, heightmap filling and
erosion) comes later. `NoiseTileSizeOracle.java` (`.local/terraforged/oracles/noise_generator/`, `run-tile-size.sh`)
runs the source's own class and the jar's, interpreted and JIT-compiled alike, over 16 radii (the `int` extremes and
overflowing products included) and 16 coordinates, and replays `ErodedNoiseGenerator`'s chunk-to-tile loops
(`collectNeighbours`, `generateCenterChunk`, `awaitNeighbours`) for six radii and five chunks (negative ones and the
`int` extremes, where the chunk offsets wrap). The test matches its 63 lines bit for bit, in debug and release.

The heightmap smoothing (`erosion.rs`, `HeightmapSmoothing` with `HeightmapSmoothingSettings`) comes from neither
TerraForged version above: it is TerraForged Reimagined 26.3's `worldgen.noise.erosion.HeightmapSmoothing` and
`TerraSettings.Smoothing` (Romoslayer/TerraForged-Reimagined, branch 26.3, a24d450), an unofficial port whose
`ErodedNoiseGenerator` smooths the eroded region heightmap after erosion. It is adapted under Reimagined's MIT licence
(copyright (c) 2020 TerraForged and (c) 2026 Romoslayer), preserved in `LICENSE-TERRAFORGED-REIMAGINED`. The
1.18.2-0.3.1 jar and the newer source (6dd607eb) never smooth there (`run-smoothing.sh` checks both). The algorithm is Engine's `Smoothing` filter (above) on a
bare height array: the same `round(radius + 0.5)` scan radius, `1 - d² / r²` weights and in-place z-then-x passes, but
no erosion mask, no altitude modifier (every cell smooths evenly) and no absent cells. Its settings are Reimagined's:
0 iterations (off: TerraForged 0.3.x never smoothed), radius 1.75, rate 0.85, unlike Engine's 1, 1.8 and 0.9. Cells
closer than the scan radius to the edge never move; a map shorter than `length²` panics where Java throws.
`HeightmapSmoothingOracle.java` (`.local/terraforged/oracles/noise_generator/`, `run-smoothing.sh`) compiles
Reimagined's own class and settings and runs them, interpreted and JIT-compiled alike, over 24 settings rows (off,
active, zero, negative, NaN, infinite and subnormal radii and rates) × 3 maps (smooth, random, and with NaN, infinities,
-0 and subnormals) × 8 lengths up to Reimagined's 256, and over the heights of real `NoiseData` from the newer
source's `NoiseGenerator.generate` (both terrain sets, two `TerrainLevels`, two runtime seeds, four chunks). The tests
match its 25 settings digests and 8 real-data digests bit for bit, in debug and release.

The erosion brushes (`erosion.rs`, `ErosionBrushes` with `ErosionFilterSettings`) are the radius-7 brush tables of the
mod's hydraulic `ErosionFilter`, in TerraForged Reimagined 26.3's layout (a24d450, MIT, as above): one normalized disc
of offsets relative to the droplet's cell, shared by the interior, plus a clipped, re-weighted copy for each cell
within the disc's reach (6) of an edge. The 1.18.2-0.3.1 jar and 6dd607eb store one absolute table per cell (Engine's
`Erosion.initBrushes` at radius 7, ported flat as `terraforged_filters::ErosionBrushes`), but every cell gets the
same points, order and weight bits in all three trees. The separate type is kept because the per-cell layout grows
with the map's area (~110 MB for Reimagined's 256x256 region map, ~6 MB here). `ErosionFilterSettings` defaults are
Reimagined's `TerraSettings.Erosion` (350 droplets, lifetime 25, volume and velocity 0.7, rates 0.5); the jar and
6dd607eb set only the 350 droplets on Engine's settings, so their droplet lifetime is 12. Map sizes use Java `int`
math (a square that wraps negative panics where Java throws). `ErosionBrushOracle.java`
(`.local/terraforged/oracles/erosion_brushes/`, `run.sh`) compiles each tree's own `ErosionFilter` and digests every
cell's brush for 21 map sizes (negative, zero, wrapping and 256), interpreted and JIT-compiled alike; all three
trees agree, and the test matches the digests bit for bit, in debug and release, and checks every cell against
Engine's port.

The hydraulic erosion filter (`erosion.rs`, `ErosionFilter`) is TerraForged Reimagined's droplet simulation over a
flat heightmap: one droplet per chunk per iteration, started from a `FastRandom` seeded with `NoiseUtil.seed` of the
world chunk and of the seed and iteration, clamped (as an `int`) one block inside the map, then moved, eroded through
the brushes and deposited bilinearly in Java's f32 order (`(float) Math.sqrt` as f64). `apply_tile` is the
`NoiseTileSize` form that 6dd607eb and the 1.18.2-0.3.1 jar also have (the jar takes the seed in its constructor);
`apply_region` is Reimagined's region-wide form (`chunksAcross` chunks over a `mapSize` map). Java's `FastRandom`
argument is not taken: each chunk reseeds both of its fields, so a fresh generator per chunk draws the same numbers.
It is not Engine's `Erosion` (#69), a different model over cells (inertia 0.05 instead of 0.005, capacity factor 4
instead of 7, minimum capacity 0.01 instead of 0.008, gravity 3 instead of 2.5, 1 % evaporation instead of 35 %, no
0.4 height falloff, a float start clamp, and NaN lengths and speeds zeroed instead of stopping or carrying the
droplet), so the two stay separate. Maps too small for a droplet's cell panic where Java throws.
`ErosionFilterOracle.java` (`.local/terraforged/oracles/erosion_filter/`, `run.sh`) runs each tree's own filter on
synthetic maps (value noise, a ramp, flat, and NaN/infinite/negative cells) for six settings sets (the jar's lifetime
12, Reimagined's 25, small, odd, zero and negative counts), five seeds and wrapping chunk coordinates, tiles of radius
0 to 2 and regions up to Reimagined's 256-block map, interpreted and JIT-compiled; 6dd607eb's and the jar's tiles
equal Reimagined's, and the tests match every map bit for bit, in debug and release.

The eroded-region layout and cache (`erosion.rs`, `ErosionRegionLayout` and `ErosionRegionCache`) are TerraForged
Reimagined's `ErodedNoiseGenerator` (a24d450, MIT, as above): erosion runs once per square of 8x8 chunks over a
256x256 map with 4 chunks of margin on every side, and each chunk or block reads its slice at `64 + (floorMod(chunk, 8)
<< 4)` (+ `block & 15`), in Java `int` math. The 1.18.2-0.3.1 jar and 6dd607eb have no regions (they erode a
`NoiseTileSize` tile per chunk). The cache is `getRegion`'s: keyed by `PosUtil.pack(regionX, regionZ)`, 16 regions
dropped least recently used first, emptied when the seed changes (so a hit always has the caller's seed), one build
per region on the first caller's thread with no lock held while the other callers of that region wait, and a failed
build removed before its waiters fail, so the next call builds again. A cache belongs to one generator (Java: a field
of `ErodedNoiseGenerator`), so the height source and settings a region also depends on are fixed. Engine's
`LossyCache`, `Cache` and `CacheEntry` don't fit: they build under the map's lock or into lossy buckets. The region
build is below; reading chunks out of the regions (`generate`) comes later. `ErosionRegionOracle.java` (`.local/terraforged/oracles/erosion_region/`,
`run.sh`) runs Reimagined's own `ErodedNoiseGenerator` over a stub height source whose maps hold their own indices
(erosion and smoothing off) through a 46-call trace (hits, evictions, a touched entry, seed changes, a failed build,
negative and `int`-limit chunks and seeds), block offsets and key packing; the Rust test replays it line for line.
Other tests check that cached regions equal uncached builds across four seeds on eight threads, and that a failed
build's waiters fail and the next call rebuilds.

The eroded region build (`eroded_noise_generator.rs`, `ErodedNoiseGenerator`) is TerraForged Reimagined's
`ErodedNoiseGenerator.buildRegion` and `erodedHeightNoise` (a24d450, MIT, as above); the 1.18.2-0.3.1 jar and 6dd607eb
erode one `NoiseTileSize` tile per chunk instead. A region's 256x256 map holds the raw terrain height
(`NoiseGenerator::sample_terrain`, without rivers, one sample reused for the whole map as in Java) of every block from
its origin chunk (`region * 8 - 4`, Java `int` math), rows then columns, at `block * frequency`; then the region-wide
`ErosionFilter::apply_region` runs over its 16x16 chunks from that origin chunk with the runtime seed, then
`HeightmapSmoothing` with Reimagined's settings (off by default). Regions are built through the region cache above.
`eroded_height_noise` is the column height the world is built to: the raw terrain sample, its height replaced by the
cached eroded one, then the rivers carved. Reimagined's `NoiseGenerator.sampleTerrain` and `sampleRiver` are
6dd607eb's, which the port's `NoiseGenerator` follows. A sample that fails (a custom module has no seed-first form) is
an `Err` where Java throws: the build is not cached, its waiters get the same `Err`, and the next call builds again.
`ErodedRegionOracle.java` (`.local/terraforged/oracles/erosion_region/e2e/`, `run.sh`) compiles Reimagined's own
`ErodedNoiseGenerator`, `ErosionFilter` and `HeightmapSmoothing` and links them to the newer source's real
`NoiseGenerator` (the setup of `oracles/noise_generator/`); it runs the real `buildRegion` over six regions (two
levels, both terrain sets, four seeds, negative regions, smoothing on and off, the jar's droplet lifetime),
interpreted and JIT-compiled alike. The test matches every map bit for bit (a digest of all 65536 floats) and four
cached column heights per region, in debug and release; others check that cached heights equal uncached builds on
eight threads, and that failed builds are errors and not cached.

Chunk generation (`ErodedNoiseGenerator::generate`, also the seed-first `NoiseGenerator` trait's `generate`) is
Reimagined's `ErodedNoiseGenerator.generate` (a24d450, MIT, as above): it reads the chunk's eroded region (building it
if needed), then fills the chunk and its one-block border (`dz` then `dx`, -1..16, Java `int` math) with each
position's raw terrain sample, its height replaced by the region's (from the chunk's offset in the region's map; a
border position reads the region's own margin, not the neighbouring region, as Java does, so it can differ from the
neighbouring chunk's edge), then the rivers carved. Java samples each position into its own `NoiseSample` of a pooled
`NoiseResource`: a new resource starts every position from the defaults, while a reused one carries the river noise
over from the same position of the chunk that used it before, so Java's output depends on which chunks shared a
resource (the oracle's `pool` lines show it: chunk (25, 16) right after (24, 16) on one resource differs from a fresh
fill). The port starts every position from the defaults and fills a new `NoiseData` per call, so a chunk never
depends on the chunks or threads before it; the river noise starts at `RiverStart::InPlay`'s 0 (what 6dd607eb's
reused samples settle at in play, Arthur's choice) or, with `sparse_forests`, at `RiverStart::Fresh`'s 1 (Java's new
resource). A failed region build is an `Err` (the trait panics) where
Java throws out of `generate`; a later failure (a failing sample, or a panic, the consumer's included) is logged and the
chunk left unfilled, as Java's `catch (Throwable)` does. The trait's `height_noise` and `levels`, and `find`, stay on the
un-eroded generator, as in Reimagined. `ErodedChunkOracle.java` (same folder, `run-chunks.sh`) runs Reimagined's real
`generate` (its resource pool cleared before every chunk, so the test uses `RiverStart::Fresh`) over twelve chunks in three settings groups (seeds 0,
`i32::MIN` and `i32::MAX`; negative chunks, chunks on both sides of region boundaries, region corners, river chunks, and
chunks whose block coordinates wrap at the `int` limits), interpreted and JIT-compiled alike; the test matches every
element of each chunk's `NoiseData` bit for bit, in debug and release. Others check that cached chunks equal uncached
builds on eight threads and in any order, that a failed region build is an `Err`, and that a panicking consumer is
caught and logged.

The terrain cache (`terrain_cache.rs`, `TerrainCache`) is TerraForged Reimagined's `worldgen.terrain.TerrainCache`
(a24d450, MIT, as above: 6dd607eb's class plus `getErodedHeightNoise`; the 1.18.2-0.3.1 jar's keys by chunk position
alone and has no seed parameters). The first `get_async`, `get_now` or `hint` of a runtime seed and chunk submits one
`TerrainGenerator::generate` task to the cache's thread pool (Engine's async `CacheEntry` on the port's `ThreadPool`,
sized like Minecraft's background executor, `clamp(cores - 1, 1, 255)` threads, in place of Java's
`Util.backgroundExecutor()`); every later caller of the same seed and chunk shares that task until `remove` (Java's
`drop`, called once the chunk is decorated) takes it out, waits for it and gives its data back to the generator's pool
once the last reader lets go. As in Java there is no capacity, expiry or eviction, the seed is part of every key (a hit
never returns another seed's chunk), `get_if_ready` never starts a task, a panicked generation panics every reader
(Java's `join` throws) and is not rerun until removed, and `height`, `eroded_height_noise`, `noise_sample` and `sample`
are passed straight to the generators, uncached. The map's lock covers only the lookup and the submission, never the
generation, so it never nests with the region, cell or river caches. Java's pooled keys and values and
`combineAsync` are not ported. `TerrainCacheOracle.java` (`.local/terraforged/oracles/terrain_cache/`, `run.sh`)
compiles Reimagined's real `TerrainCache` against a recording generator stub on one FIFO task thread and traces
readiness, sharing, removal, regeneration, seeds and `int`-limit chunks, interpreted and JIT-compiled alike; the test
replays the trace on one task thread. Others check that cached chunks equal uncached ones bit for bit for four seeds
on eight threads, that a running task is shared and reported not ready, and that a failed generation is not rerun.
An entry's `TerrainData` is one shared, mutable value, as Java's single object is: handles read it with `read()` and
write it with `write()` (an `RwLock`, so a write serializes with every reader and every handle sees it), and the
entry keeps a copy of its generated heights, which `write_generated()` puts back before it hands out the data.

The generator queries (`generator_facade.rs`, `TerraForgedGenerator`) are TerraForged Reimagined's
`worldgen.Generator` (a24d450, MIT, as above) over its terrain cache: `createState`'s seed capture (`Seeds.get`, the
level seed's low 32 bits, kept beside the raw level seed), `getChunkData`/`getChunkDataAsync`, the cache hints and
drop (each with the seed its Java hook passes), `getMinY`, `getSeaLevel`, `getGenDepth`, `getBaseHeight` and
`getBaseColumn` (see below). Heights count from block 0, as TerraForged's levels do; `getGenDepth` is `levels.maxY` (480 by
default, the exclusive top, not `maxY - minY`), and Reimagined fits that by widening the overworld dimension type to
`-64..960`. A TerraForged world does the same (its own data pack carries that dimension type; see the world type
below), so nothing is rescaled. `getBaseHeight` (the structure height query) follows Reimagined, Arthur's choice for #210: it reads the
un-eroded sample's base (water) level and the ERODED height (`getErodedHeightNoise`, erosion and rivers: the height the
chunk is filled to), the surface heightmaps returning the higher plus one, the ocean-floor ones the height plus one.
That is Reimagined's fix for floating structures; TerraForged 6dd607eb reads the raw sample's height there, so its
structures float or sink wherever erosion moved the ground. `getBaseColumn` reads the raw height in both trees; the
port reads the eroded one there too (a departure from both), so structures placed by column (ruined portals, nether
fossils, land checks) and by height see the same ground, the filled one. Both trees also build the column as
`new NoiseColumn(height, states)` with stone in `states[0..height]`, so it starts at the height instead of the bottom of
the world: its stone runs from the height up to about twice the height, and column-placed structures float. The port
departs from both there too (Arthur's decision for #211): `base_column(x, z, level_min_y, level_height)` has 26.3's
`NoiseColumn` shape, as vanilla's `NoiseBasedChunkGenerator.getBaseColumn` builds it: stone from the level's bottom up
to the eroded height, water above it up to the surface (the higher of the height and the base level), air above, all
clipped to the level as the fill clips the terrain, so it is exactly the filled ground. A sample that fails (a custom
module has no seed-first form) is an `Err` the caller gets, never a made-up height; Java's throws on negative column
heights have no counterpart (the column is simply empty below the level). `GeneratorQueryOracle.java`
(`.local/terraforged/oracles/generator_facade/`, `run.sh`, after `oracles/erosion_region/e2e/run.sh`) runs the query
bodies (copied, since `Generator` needs Minecraft's `ChunkGenerator`) on Reimagined's own `TerrainLevels`,
`NoiseLevels` and `ErodedNoiseGenerator` over the newer source's `NoiseGenerator`, interpreted and JIT-compiled alike:
the query math over signed zeros, subnormals, huge values, infinities and NaN for three level sets, `Seeds.get`, and
thirteen end-to-end positions (four seeds, negative blocks, `int` limits, the world border); the tests match them bit
for bit in debug and release. `oracles/generator_facade/eroded_column/` (`run.sh`, after `../run.sh`) reruns the
end-to-end positions and adds each one's column from the eroded height, in Java's shape; the test reads the same height
and surface back from the port's `base_column`. A test fills chunks and checks that both queries name the filled ground
in every column, the column block for block; another checks the column's clipping over the query-math noises. 6dd607eb's raw-height queries stay checkable with
`oracles/generator_facade/6dd607eb/` (`extract.py` copies its `getBaseHeight` and `getBaseColumn` verbatim); the query
math and `Seeds.get` lines equal Reimagined's.

The chunk fill (`chunk_fill.rs`, `fill_chunk`) is `ChunkUtil.fillChunk` then `ChunkUtil.primeHeightmaps`, as
TerraForged Reimagined's `Generator.fillFromNoise` calls them (a24d450, MIT, as above; the fill logic is the same in
6dd607eb and the 1.18.2-0.3.1 jar, Reimagined's is the 26.3 form). It writes a Horizon `TerrainChunk` from a chunk's
`TerrainData`: whole stone sections below the section of the chunk's lowest height, then, in the sections up to
the one holding `max(getMaxBase, getMax)`, stone up to each column's height and water above it up to the column's
water level (`TerrainLevels::water_level`: the base height in a still river or lake, the sea level elsewhere).
As in Java, water stops at that highest section's top even when the sea level is higher (an all-ocean chunk whose
heights and base levels stay below the sea level's section), and a NaN height, which makes the chunk's lowest
height -1, fills stone below section -16. OCEAN_FLOOR_WG takes each column's height and WORLD_SURFACE_WG
`max(sea level, height)`, ignoring river water levels (Java's `primeHeightmaps` does the same).

The terrain stage hand-off (`TerraForgedGenerator::fill_from_noise`) is Reimagined's `buildTerrain` up to its fill:
`terrainCache().combineAsync(EXECUTOR, seed, chunk, ..)` with the captured seed (`Seeds.get` of the level seed;
6dd607eb passes `Seeds.get(state)`), which takes the cached `TerrainData` at the chunk's own position and runs
`fillFromNoise`'s `fillChunk` and `primeHeightmaps` with the generator's sea level. It returns the cache entry's data
for the later steps; like Java it neither hints nor drops the entry (`drop_chunk`, after decoration, does). Java runs
the fill as a continuation on its executor; the port waits for the cache task on the calling thread and fills there.
The chunk's own build range is what the fill clips to (below). A region build that fails panics where Java's
future fails; a later sample failure leaves the data unfilled and the chunk is filled from it, as in Java.

`TerraForgedGenerator::fill_from_noise_with_structures` is the rest of 6dd607eb's `fillFromNoise` continuation:
the same fill, then `ChunkUtil.buildStructureTerrain` (`StructureTerrain::modify_chunk`, below) with the default
states of `minecraft:stone` and `minecraft:air`. The caller builds the `StructureTerrain` with
`StructureTerrain::from_starts` over the generator's `references(..)` for the chunk (the beardifier's list), as
Java's constructor reads them from the `StructureManager`. The structure edits write their OCEAN_FLOOR_WG heights into
the cache entry's own `TerrainData`, so the later stages that read the entry (surface, carvers, decoration, each at
the chunk's own position, until `drop_chunk`) see the structure-adjusted heights, as they read Java's one object. The
fill and the edits run under one write guard: no reader sees a half-adjusted chunk. Both fills start from the
entry's generated heights (`write_generated`). Java's second run of the stage for the same entry, which happens only
if the chunk is generated again before its entry is dropped, would fill from the heights the first run's
structures left, so its output depends on when the chunk was generated again; the port doesn't reproduce that: a
rerun gives the first run's blocks and heights. The facade tests check that the stage equals `fill_chunk` then
`modify_chunk` on a freshly generated entry, that every handle sees the edited heights with the extrema unchanged,
that a rerun of the structure stage (also four at once on four threads) leaves the same chunk and heights, and
that a plain fill after a structure stage gives the plain blocks and heights again. Each step has its own Java oracle (the terrain stage
oracle and `ChunkAdapterOracle`, below); the composition is Java's three calls in order, so it has no oracle of its
own. Heights the structure edits write come from the clipped chunk (its OCEAN_FLOOR_WG), while unedited columns
keep TerraForged's unclipped heights; with terrain above the build range the two differ, which TerraForged's taller
overworld (y −64 to 959, below) settles. The surface step, the carver step (`cave_carver.rs`) and the registration
that lets Horizon call the stage (`world_preset.rs`, `generator_preset.rs`) come later, below. Deep Caves and Strata
are TerraForged Reimagined's (`DeepCaves`, `StrataDecorator`): 6dd607eb has neither, so they are not ported.

The surface erosion pass (`surface.rs`, `apply`) is TerraForged 6dd607eb's `worldgen.biome.surface.Surface.apply`
with `findSolid` (MIT, as above). On columns at or above sea level whose gradient (normalised by `55 * genDepth /
255`) is at least 0.6, it replaces the erodible top blocks with the first non-erodible state found strictly above
`max(0, y - 20)` (void air below the build range counts as non-erodible, as `ProtoChunk.getBlockState` reads it).
Writes go through `set_block_worldgen`, so only the chunk stage's heightmaps follow them, as with 26.3's
`ProtoChunk.setBlockState`: at TERRAIN, where the surface step runs, OCEAN_FLOOR_WG keeps its pre-erosion heights.
The caller supplies which states are erodible (Java's `isErodible`: `#minecraft:dirt` or `#minecraft:snow`). With
26.3's tags that leaves grass, podzol, mycelium, moss and mud uneroded (`#minecraft:dirt` held them before 26.x);
TerraForged Reimagined switches to `#minecraft:substrate_overworld` to keep them, and the bindings below follow it. The cached terrain data is only read. Snow postprocessing (`applyPost`, below) is ported; the
call from the surface decorator is a later layer. The test runs `SurfaceErosionOracle`
(6dd607eb's three methods, copied verbatim, over real 26.3 ProtoChunks, heightmaps and block tags): 160 generated
chunks over three build ranges, four persisted statuses, primed and unprimed heightmaps, three gen depths and NaN
gradients, comparing gradients, every block and all six heightmaps. Some of them put steep, all-erodible soil at the
bottom of a 16..80 chunk, where the search runs into void air and copies it up the column, as Java does.

The water edge smoothing pass (`surface.rs`, `smooth_water`) is TerraForged 6dd607eb's `Surface.smoothWater` with
`shouldSmooth`, `isSmoothable` and `sameChunk` (MIT, as above; the 1.18.2-0.3.1 jar and TerraForged Reimagined 26.3 have
the same logic), which `BiomeGenerator.decorate` runs after features and cave decoration. A river or lake column whose
river value is exactly 0 (`-0.0` counts, NaN doesn't) and whose MOTION_BLOCKING_NO_LEAVES top is source water becomes
level-2 water when any block at that height in the radius-6 disc around it is air (rows from -6, centre skipped, first
hit wins). Blocks of the chunk itself are read from it; the others come from a caller-supplied region lookup in world
coordinates (Java's `WorldGenLevel`), whose errors stop the pass. Writes go through `set_block_worldgen`, and a missing
MOTION_BLOCKING_NO_LEAVES is primed only when the first eligible column reads it (`ChunkAccess.getHeight`). The caller
supplies the water and level-2 water state ids; the region wiring is a later layer. The test runs `WaterSmoothingOracle`
(the four methods, copied verbatim, over real 26.3 ProtoChunks and heightmaps, with Engine 0.3.0's terrain types and a
region proxy over the eight neighbouring ProtoChunks): 128 generated 3x3 chunk grids over three build ranges, four
persisted statuses and three priming states, with flowing water, leaves over water, kelp, cave air and signed-zero, NaN
and non-zero river values, comparing every block, all six heightmaps (absent ones too) and every region read in order.
The oracle lives outside Git, next to the erosion one, in the local TerraForged oracle folder
(`.local/terraforged/oracles/surface_erosion/`, whose `run.sh` builds both). A separate test covers block coordinates
that wrap past `i32::MAX` at the far chunk edge, as Java's int arithmetic does.

The snow pass (`surface.rs`, `apply_post`) is TerraForged 6dd607eb's `Surface.applyPost` with `smoothSnow` and
`erodeSnow` (MIT, as above; the 1.18.2-0.3.1 jar has the same bytecode, and TerraForged Reimagined 26.3 only adds two
switches that default to on). `BiomeGenerator.decorate` runs it right after water smoothing. For each column, `y` is one
above the MOTION_BLOCKING_NO_LEAVES height, and the gradient is normalised by `70 * genDepth / 255`. On gentle slopes
(below 0.625), a snow layer at `y` gets `1 + floor(7.9999 * (height - floor(height)))` layers from TerraForged's
unclipped height. `NoiseUtil.floor` takes 1 off a negative whole number, so -60.0 gets 8 layers where +60.0 gets 1, as
in Java. On steeper slopes (and NaN), an air block at `y` moves the test one block down. A `#minecraft:snow` block there
becomes air, and the erodible blocks under it, strictly above `max(y - 15, 0)` (absolute 0, not the build range's
bottom), become stone. The snow-layer test (`instanceof SnowLayerBlock`, in 26.3 only `minecraft:snow`), the `layers`
value, air and stone come from the chunk's own block registry. The caller supplies `#minecraft:snow` and `isErodible`
(the same predicate as erosion's). 26.3's `#minecraft:snow` is snow, snow block and powder snow, unchanged from 1.18.2.
In 26.3 neither snow layers nor powder snow block motion, so `y` lands on them. A layer count outside 1..8 (NaN,
infinite or huge heights) returns `InvalidState` where Java's `setValue` throws (an `IllegalArgumentException`, or an
`ArrayIndexOutOfBoundsException` for a wrapped `Integer.MIN_VALUE`); the columns already written stay written. Writes
go through `set_block_worldgen`, and MOTION_BLOCKING_NO_LEAVES is primed when the first column reads it. The test runs
`SnowPostOracle` (the methods copied verbatim, with the TerrainData stub's height map and `TerrainLevels.getHeight`,
over real 26.3 ProtoChunks, heightmaps and block tags; same folder and `run.sh`): 192 generated chunks over three build
ranges, four persisted statuses, three priming states and three gen depths, with snow layers, snow and powder snow,
snow on leaves, cave air and water caps, full columns, columns near y 0 and NaN or infinite heights (6 of them throw).
It compares every block and all six heightmaps, and also checks Java's SnowLayerBlocks and tag contents.

The surface bindings (`surface_bindings.rs`, `SurfaceBindings`) resolve what the three passes take from the caller,
once per world, from the 26.3 block registry and its bound block tags (nested tags flattened): `#minecraft:snow`,
the erodible tags, and the source and level-2 water states (`minecraft:water` with `level=0` and `level=2`, as
`smoothWater`'s `LEVEL == 0` test and `setValue(LEVEL, 2)` give them). Air, stone and the snow layers come from the
chunk's own registry inside the passes. By default (`SurfaceBindings::new`) a state is erodible when it is in
`#minecraft:substrate_overworld` or `#minecraft:snow`, TerraForged Reimagined a24d450's `isErodible` (MIT, as
above). That departs on purpose from 6dd607eb's literal `#minecraft:dirt` or `#minecraft:snow`: 26.x shrank
`#minecraft:dirt` to dirt, coarse dirt and rooted dirt and moved grass, podzol, mycelium, moss and mud into
`#minecraft:substrate_overworld`, so the literal tag would leave grassy slopes uneroded. 26.3's `substrate_overworld`
is dirt, coarse dirt, rooted dirt, mud, muddy mangrove roots, moss block, pale moss block, grass block, podzol and
mycelium. `SurfaceBindings::with_erodible_tags` takes any tag list (`JAVA_ERODIBLE_TAGS`
gives 6dd607eb's); the passes themselves still take the predicate, and their oracles test Java's literal tags. A
missing tag, water block or water level is an error when the bindings are built. The call from the surface decorator
and the region lookup for water smoothing are a later layer.

The preliminary surface-height cache (`surface_height_cache.rs`, `initialize`, and the facade's
`initialize_surface_cache`) is TerraForged 6dd607eb's `NoiseChunkUtil.initChunk` and `initSurfaceCache` (MIT, as
above), which its surface decorator runs before the vanilla surface rules. If the cache is empty, it stores the
chunk's height at each of its 16 quart columns (`TerrainData.getHeight`, read dz before dx) under its block column
(`ColumnPos.asLong`), then stores the lowest of the 16 at every other quart column of the surrounding 3x3 chunk area.
The facade asks for the chunk's terrain with the captured seed, as Java's `getChunkDataAsync(Seeds.get(state), ..)`
does. It reads the shared entry's current heights, so after the structure-aware terrain stage it sees the structure
edits, as Java's surface stage reads the `TerrainData` its `fillFromNoise` edited. A cache that is already filled is
left alone. The heights are TerraForged levels and are not clipped to the build range, so the world-height decision
covers them too. The 1.18.2-0.3.1 jar differs: it reads all 256 columns, keys by quart coordinates (`ChunkPos.asLong` of the quart position),
and pads the area with the minimum of all 256. TerraForged Reimagined drops the cache, because 26.3's `NoiseChunk`
has none. The port keeps 6dd607eb's block-column keys, which match the block positions Horizon's preliminary-surface
sampler receives. Wiring it into Horizon's surface rules is a later layer. The oracle runs 6dd607eb's own methods
(extracted from the source) with `ColumnPos` and `QuartPos` stand-ins that match 26.3's bytecode. It compares the
whole 144-entry cache at the origin, a negative chunk and the `int` corners, the order of the reads, and the
filled-cache case. It is kept outside Git, in `.local/terraforged/oracles/surface_height_cache/`.

The structure terrain columns (`structure_terrain.rs`, `StructureTerrain`) are TerraForged 6dd607eb's
`worldgen.terrain.StructureTerrain.modify` and its helpers (MIT, as above), the per-column part of `fillFromNoise`'s
`buildStructureTerrain`. Given the chunk's rigid structure pieces, sorted by their boxes' `minY` (stable, as
fastutil's sort), each column raises the ground towards every piece standing above it until a piece covering the
column is found, with Java's `max(4, 20 - length)` blend radius and `pow(alpha, 2 - alpha)` falloff, then fills
stone from the column's OCEAN_FLOOR_WG height up to the raised level and carves air through the highest covering
piece, from its floor (box `minY` plus a pool piece's ground-level delta) to its roof; when the highest floor of all
the pieces, or the column's new surface, is more than 5 blocks above the roof, the roof rises by half that depth
times an ellipse falloff over the box (`NoiseUtil.round`). `modify` returns the new OCEAN_FLOOR_WG height
when it changed the column, the value Java writes into `TerrainData`'s height map. `modify_chunk` is
`buildStructureTerrain`'s column loop over a Horizon proto-chunk (dz, then dx, from the chunk's minimum block): it
reads OCEAN_FLOOR_WG as `ChunkAccess.getHeight` does (priming it if missing), writes through `set_block_worldgen`
(26.3's `ProtoChunk.setBlockState`: writes outside the build range are dropped, and each write updates the heightmaps
of the chunk's persisted status, so before TERRAIN, where `fillFromNoise` runs, the next read sees it; from TERRAIN on
26.3 updates the final heightmaps instead and OCEAN_FLOOR_WG keeps its old heights, as in Java), and stores each
returned height in `TerrainData`'s height map as Java does (`Index.CHUNK`, the `int` widened to float; the
min/max/max-base extrema are left as they were). `StructureTerrain::from_starts` is the constructor's piece
collection: given the starts referencing the chunk, in the order 26.3's `startsForStructure` returns them (the
generator's `references`, which the beardifier reads too), it keeps the BEARD_BOX and BEARD_THIN structures' pieces
whose box is within 20 blocks of the chunk (`isCloseToChunk(pos, 20)`, Java `int` wrap), drops pool pieces whose
element is not RIGID (the empty element is terrain-matching), takes a pool piece's ground-level delta (0 for other
pieces) and sorts by `minY`, stable. Within a structure 26.3 visits the starts in its references' `LongOpenHashSet`
order (which `references` reproduces); across structures in a `HashMap`'s identity-hash order, so Java does not fix
the order of tied-`minY` pieces from different structures either. The terrain stage calls `modify_chunk` (above);
building the pieces with `from_starts` from the generator's references is the caller's job. Reimagined a24d450
changes the blend radius (`clamp(length * 0.75, 8, 24)`, collection radius 32) and skips buried structures; the port keeps 6dd607eb's. `StructureTerrainOracle.java`
(`.local/terraforged/oracles/structure_terrain/`, `run.sh`) compiles 6dd607eb's `StructureTerrain.java` unchanged, with
fastutil's `ObjectArrayList` and Noise2D's `NoiseUtil`, over stub chunks and runs `buildStructureTerrain`'s column loop
on 96 generated chunks (negative chunk positions, tied `minY`s, pool ground deltas, surfaces near both build limits);
the test matches its ordered block writes and height-map writes in debug and release, interpreted and JIT-compiled
alike. `ChunkAdapterOracle.java` (`.local/terraforged/oracles/structure_terrain/chunk/`, `run.sh`) runs the same
`StructureTerrain.java` (its constructor given the collected pieces, and `setBlockState`'s 1.19 `false` changed to
26.3's int flags as Reimagined does; `patch.py` prints the diff) over real 26.3 `ProtoChunk`s and `Heightmap`s with the
jar's block tags, on 96 chunks at the BIOMES, EMPTY, TERRAIN and FEATURES statuses, with and without primed WG
heightmaps, with water over the stone; `modify_chunk` matches its blocks, all six heightmaps and the height map.
`SelectionOracle.java` (`.local/terraforged/oracles/structure_terrain/selection/`, `run.sh`) runs 6dd607eb's unchanged
constructor (only 26.3's `startsForStructure(x, z, ..)` call form and `setBlockState` flags patched;
`patch.py` prints the diff) through a real 26.3 `StructureManager` over `ProtoChunk` references and starts, with
`PoolElementStructurePiece`s of single and empty pool elements and plain pieces, on 128 chunks (margin edges at -20/-21
and +35/+36, chunk positions where `x << 4` and the margin wrap, empty starts, tied `minY`s); `from_starts` matches the
collected pieces in order, given Java's visit order.

**World height.** TerraForged's heights count from block 0 up to `levels.maxY` (480 by default), and Reimagined
widens the overworld to `-64..960` so that its terrain always fits. Horizon 26.3's overworld is `-64..320`, and the
fill does not rescale anything: it clips to the chunk's build range (`chunk.min_y()` to `min_y + height`). Java
would throw instead (`ArrayIndexOutOfBoundsException`: the section loop reads a section outside the chunk, for
terrain whose highest section starts at or above the top, or whose lowest is below the bottom). In the clipped
chunk the blocks are exactly the window of the fill Java makes in a chunk tall enough for the terrain: terrain above
y 319 is cut off at y 319 (a flat stone top; water reaches the top only where the chunk's highest section
does), and a column below the bottom gets no stone but still its water. Heightmap heights above the top are stored as the top (first
available `min_y + height`, 320; Java stores a higher one where its bit storage holds it), heights below the
bottom leave the map at `min_y`. Heights at the `int` extremes (infinite or NaN noise) neither wrap nor panic, and
Horizon's chunks themselves stay within -2032..2032. Whether the overworld is widened, or TerraForged's levels
scaled into 26.3's range: a TerraForged world widens the overworld to `-64..960` as Reimagined does (see the world
type below), so the clipping only matters in a 26.3-high overworld.

`ChunkFillOracle.java` (`.local/terraforged/oracles/chunk_fill/`, `run.sh`) runs Reimagined's `fillChunk`,
`fillSection` and `primeHeightmaps`, copied onto a stub chunk, with 26.3's `Heightmap.update` and its bit-storage
range check transcribed from the 26.3 jar, interpreted and with `-Xcomp`: sixteen generated chunks (vanilla,
Reimagined's `-64..1024`, Horizon's tallest -2032..2032 and one-section chunks; river columns; sea levels below,
inside and above the terrain; NaN lowest heights; terrain at, one above and one below the build range, and well
outside it). It fills each in a `-4096..4096` chunk and digests the case's build range; the test matches every
digest, the five cases where Java throws in the case's own chunk included.

`TerrainStageOracle.java` (`.local/terraforged/oracles/generator_facade/`, `run-stage.sh`, after `run.sh`) runs the
hand-off end to end: Reimagined's real `TerrainCache.combineAsync`, `TerrainGenerator`, `TerrainData`,
`TerrainLevels` and `ErodedNoiseGenerator` over the newer source's `NoiseGenerator`, with `fillChunk` and
`primeHeightmaps` copied onto the chunk-fill oracle's stub chunk (`oracles/chunk_fill/ChunkStage.java`), interpreted
and JIT-compiled alike. Seven chunks over three level sets and four seeds (negative chunks, the world border) are filled in
the `-64..320`, `-64..480`, `-64..960`, `0..256` and `128..192` build ranges (terrain above the top of one and
below the bottom of the other); the test matches every block and both heightmaps of
each, bit for bit, including the ranges where Java's own chunk throws.

The biome chunk fill (`biome_chunk_fill.rs`, `fill_noise_biomes`) is TerraForged 6dd607eb's
`ChunkUtil.fillNoiseBiomes` with its `BiomeBuffer2D`, as `Generator.createBiomes` runs it (MIT). For the chunk's 4x4
quart columns (`QuartPos.fromBlock` of the chunk's first block, Java `int` math), `dz` then `dx`, it asks the biome
source once per column at quart y -1 (`getNoiseBiome(x, -1, z)`), then fills every section of the chunk's build range
from that one 4x4 layer, so each column has the same biome at every height. There is no cave layer: TerraForged's
cave biomes come from its noise-cave carver, which asks the cave biome sampler below. (TerraForged Reimagined adds a
second, cave layer for the sections below y 0; this port follows 6dd607eb.) A source
error comes back before any section is written, as Java's exception leaves the chunk untouched. Like TerraForged it
fills the sections directly, so 26.3's `ChunkAccess.fillBiomesFromNoise` decorations (blending, below-zero retrogen of
upgraded chunks) do not apply. The source is the caller's closure; `TerraForgedGenerator::create_biomes` (below)
passes TerraForged's surface biome sampler at the captured seed. The mapping onto Horizon's biome registry comes with
a later layer.

`BiomeChunkFillOracle.java` (`.local/terraforged/oracles/biome_chunk_fill/6dd607eb/`, `run.sh`) extracts 6dd607eb's
`fillNoiseBiomes` verbatim from `ChunkUtil.java` and runs it with 6dd607eb's real `BiomeBuffer2D` over a stub chunk
whose sections fill as 1.19's `LevelChunkSection.fillBiomesFromNoise` does (6dd607eb's target; its exclusive
`getMaxSection` covers the same sections as 26.3's inclusive `getMaxSectionY`), interpreted and with `-Xcomp`. A
stand-in biome source gives hashed ids per seed and column. 63 chunks (nine positions, the `int` limits included,
seven build ranges from `128..192` to `-2032..2032`, six seeds) are compared on the source's call trace (`y`, `x`,
`z` of every call) and every cell of every section; failures at four calls and a reused resource are compared too.
The tests match all of them.

The cave biome sampler (`cave_biome_sampler.rs`, `CaveBiomeSampler`) is TerraForged 6dd607eb's `CaveBiomeSampler`
(MIT): one simplex noise (`Noise.singleSimplex`, seed + 124897 with Java `int` wrap, frequency `1F / scale`, mapped to
`(1 + n) * 0.5` and clamped to 0..1) picks from an equal-weight `WeightMap` of the GLOBAL pool (`terraforged:cave`) or
the UNIQUE pool. `CaveBiomeSampler::new` keeps the two pools independent, as 6dd607eb does: an empty pool stays empty
and its lookups are `None` (Java's `WeightMap.getValue` returns null). 6dd607eb's own UNIQUE pool is every registry
biome (its `UNDERGROUND` filter is commented out), and its biome source never returns a cave biome: only its noise-cave
carver (`CarverChunk`) asks the sampler.

TerraForged Reimagined's additions to it (a coverage mask read by its cave biome layer, and an empty UNIQUE pool
falling back to GLOBAL) are not ported: the chunk biome fill above has no cave layer.

The oracle lives outside Git in `.local/terraforged/oracles/cave_biome_sampler/`. `6dd607eb/run.sh` compiles
6dd607eb's own `CaveBiomeSampler`, `WeightMap` and `MathUtil` with Noise2D's `Noise` and `NoiseUtil` from source over
stub registries and runs them interpreted and with `-Xcomp`: 8 scales (`0` and the `int` limits included), 6
registries (empty, one biome with and without `terraforged:cave`, up to nine biomes), 15 seeds and 310 positions,
223,200 samples of the choice bits and the GLOBAL and UNIQUE choices. (`run.sh` there is the earlier oracle of
Reimagined's classes, mask and fallback included, which no test reads now.) The tests match every digest in debug and
release builds.

The initial biome (`initial_biome`, `biome_sampler.rs`) is TerraForged 6dd607eb's `BiomeSampler.getInitialBiome`
(MIT): the climate type's pool (`getBiomeMap()`) picks with `WeightMap.getValue(biomeNoise)`, and a missing or empty
pool gives `minecraft:plains`. It reads nothing else: Reimagined a24d450's wet and dry pools, highland swap,
mountain-usage roll, slope gate and snow check are not followed. Where Java's `getValue` returns null (a noise past the
last band, NaN, or a pool whose weights are all zero) it gives `None`; Java hands that null on as the sampled biome
unless `getBiomeOverride` (below) replaces it, and fails later on it.

The current climate noise (`CurrentClimateNoise`, `current_climate.rs`) ports TerraForged 6dd607eb's `ClimateNoise`
(MIT), the climate sampler of the newer biome sampler. It is separate from Engine's `ClimateModule` (the jar's climate
stage: continent mask, macro cell noise, a 3x3 search), but reuses its `Moisture` and `Temperature` (their seed-first
forms, with Noise2D 2cb4843's `NoiseUtil.map`), the climate settings (with `RangeValue`'s clamped bounds) and the warp
builder. The constructor draws from Engine 0.3.0's `Seed`, whose `next()` returns `seed++`: the module seed (never
read), the moisture noise and two warps from the moisture offset seed, two temperature warps from the temperature
offset seed (both `new Seed(root + offset)`), then the two biome-warp seeds, so a seed at `s` ends at `s + 3`. A sample
warps the point by the biome warp (simplex2, 3 octaves, strength `biomeWarpStrength * 0.75`); finds the nearest and
second-nearest of the 4x4 jittered square cells around it (jitter 0.8); takes `biomeNoise` from that cell's hash,
`biomeEdgeNoise = 1 - sqrt(d1 / d2)`, and moisture and temperature at the cell's point; and then the climate type
from Engine's `BiomeType.get`, with Java's temporary cold steppe and steppe to grassland swap. Every noise gets the
runtime seed. `macroNoiseSize` and `biomeEdgeShape` are not read. `ClimateSettings::terraforged()` is the climate of
`NoiseGenerator.createContinentNoise`: Engine's defaults (temperature scale 6, moisture scale 6 with seed offset 0)
with biome size 220, falloffs 2 and 1 and biases 0.1 and -0.05. Engine's `BiomeType.get` throws for an index outside
0..255, and the port panics there; only NaN climate bounds get a value out of 0..1.
TerraForged Reimagined a24d450's `ClimateNoise` (an extra biome-edge warp, macro-cell grouping, a pre-increment `Seed`,
its `TerraSettings.Climate` defaults and a `BiomeType` that clamps its indices) is not followed: the port used it until
#204.

`ClimateNoiseOracle.java` (`.local/terraforged/oracles/climate_noise/`, `run.sh`) compiles 6dd607eb's own
`ClimateNoise`, `ClimateSample`, `NoiseSample`, `MathUtil` and `CellShape` from source over Engine 0.3.0, Noise2D 2cb4843
and Engine's seed-first `Moisture` and `Temperature` (its `cp.sh` class path is shared by the biome oracles below),
interpreted and with `-Xcomp`: `createContinentNoise`'s settings with and without the continent's four seed draws,
Engine's plain defaults, and 13 settings variants (macro sizes and edge warps set to show they are ignored, `int`-edge
root seeds and seed offsets, swapped, out-of-range, narrow and NaN climate ranges, falloffs 0 to 10) with up to seven
seed draws before the constructor, each sampled at 8 runtime seeds and 181 positions (zeros of both signs, fractions,
2^24, 1e10, infinities, NaN, subnormals and a signed grid): the seed draws, the samples that throw (1,050 of the NaN
variant) and a fold of every sample's bits match, as does `BiomeType.get` over 0..1; so do the samples of four
degenerate biome sizes (0, whose infinite frequency gives NaN moisture of either sign, -1, -220 and 1), in debug and
release builds.

The biome sampler bridge (`CurrentBiomeSampler`, `current_biome_sampler.rs`) ports TerraForged 6dd607eb's
`IBiomeSampler.Sampler` (MIT; Reimagined a24d450's is the same): it scales block coordinates by the noise levels'
frequency and fills a fresh climate sample from the continent noise, its rivers and then the current climate noise,
all at the runtime seed; `getShape` and `sample` run the climate alone (`sample` keeps the caller's terrain fields).
The climate is built from the continent's context, so it draws its seeds after the continent's four. Both count
with Engine 0.3.0's post-increment `Seed`: root `s` gives the continent `s..s+3` and the climate `s+4..s+6`, as
6dd607eb does. Java draws the climate's seeds from the context it shares with the
continent; here they come from a copy in the same state, so a second sampler over one generator gets the same climate
(TerraForged builds one per world). The surface biome sampler and the chunk biome hook (below) build on it.
`.local/terraforged/oracles/current_biome_sampler/` (`run.sh`) runs 6dd607eb's real `Sampler` and `ClimateNoise`
over the newer source's real `NoiseGenerator` in one JVM (until #204 the climate half ran Reimagined's): the terrain
and climate fields, `getShape`, `sample` and the context seed's next draw after the sampler; three levels variants,
two terrain sets, six runtime seeds and 202 block positions (`int` edges and a signed grid) match bit for bit in debug
and release builds.
The current surface override is 6dd607eb's `getBiomeOverride`, the same `biome_override` as the jar's
(`biome_sampler.rs`): its `beachSize` is 0.005 (`BEACH_SIZE`), so the beach band is continent noise in (0.5, 0.505]
(`BEACH + beachSize` folds to the float 0.505f, the same `<=` in `float`), and (0.505, 0.55] keeps the input biome or
becomes a river. TerraForged Reimagined a24d450 sets `beachSize = COAST - BEACH` (0.05), which makes every column up to
the coast marker a beach; that is not followed (it was the current path's band from #174 until #201).
`SourceBiomeOverrideOracle.java` (`.local/terraforged/oracles/biome_override/6dd607eb/`, `run.sh`) compiles
6dd607eb's own `BiomeSampler`, `ClimateSample`, `NoiseSample` and `ContinentPoints` from source against Engine 0.3.0
and runs them interpreted and with `-Xcomp` over 30 continent values (each threshold and its neighbouring floats, the
floats next to 0.55 and 0.5 + 0.0499 and 0.5 + 0.0501 included), every climate type, Engine's 16 built-in terrains and
10 river values; the test matches its 52,800 overrides and the beach size's and folded bound's bits, in debug and release.

The surface biome sampler (`CurrentSurfaceBiomeSampler`, `current_surface_biomes.rs`) is TerraForged 6dd607eb's
`BiomeSampler.sampleBiome` (MIT): the biome sampler bridge's climate sample (6dd607eb's climate noise, above), then
the initial biome from one pool, then the surface
override (`biome_override`, with the 0.005 beach band), which wins. It takes block coordinates and the runtime seed and
reads no height noise. A column whose pool has no biome and no override is an `Err` where Java's `sampleBiome`
returns null. The pools and the override biomes come from the caller, sharing one `Arc` per biome; `Source`'s
per-column cache is not ported; `create_biomes` (below) fills chunks with it.
`.local/terraforged/oracles/current_surface_biomes/` (`run.sh`) records, in a 6dd607eb JVM, what `sampleBiome` reads
from the real `Sampler` and newer-source `NoiseGenerator` (the continent and rivers of 7,272 columns: three levels
variants, two terrain sets, six runtime seeds, `int`-edge and grid positions), then runs 6dd607eb's real
`BiomeSampler`, `Sampler` and `ClimateNoise` (on the climate oracle's Engine 0.3.0 class path; Reimagined's until
#204), on that record, interpreted and with `-Xcomp`, for five pool configurations (some climates with empty pools, so plains
appears, or with all-zero weights, so Java returns null and the port an `Err`); any height-noise read fails the run. The test matches every biome name, in debug and release builds.

The chunk biome hook (`TerraForgedGenerator::create_biomes`, `generator_facade.rs`) is TerraForged 6dd607eb's
`Generator.createBiomes` (MIT; Reimagined's is the same apart from its cave layer): it hints the chunk to the terrain
cache with the captured seed (`Seeds.get` of the level seed), then runs the biome chunk fill at that same seed
(`Source.withSeed` keeps `(int) levelSeed`). `Source.getNoiseBiome` ignores `y`: its `compute` calls the surface
biome sampler at block `x << 2`, `z << 2` of the quart column (Java `int` wrap), so every height of the column gets
that biome, from 6dd607eb's `BiomeSampler.sampleBiome` (above). It is built
once per world and shared by every chunk, as `Source`'s sampler is (`surface_biome_sampler` builds it over the
generator's own un-eroded noise; a sampler over another generator is refused). Java runs the fill on its executor, the
port on the calling thread. A failing surface sample (a pool with no biome for a column's noise, where Java's sampler
returns null) is an `Err` before any section is written, after the hint. `Source`'s per-column `LossyCache` is not
ported: it caches the same values, and `createBiomes` asks each quart column once. The biome names onto Horizon's
registry and the registration of the hook are later layers.

`ChunkBiomeOracle.java` (`.local/terraforged/oracles/chunk_biomes/6dd607eb/`, `run.sh`, after `../run.sh`, which
needs `oracles/noise_generator/run.sh`) runs it end to end in two JVMs, as the surface biome oracle does: 6dd607eb's
real `Sampler` and the newer source's `NoiseGenerator` record what the surface sampler reads at every quart column of
17 chunks (negative, far and `int`-limit chunks); then 6dd607eb's `fillNoiseBiomes` and `Source` members, copied
verbatim (its `LossyCache` included), with its real `BiomeBuffer2D`, `BiomeSampler`, `Sampler` and `ClimateNoise`
(Engine 0.3.0, as above), fill stub
1.19 chunk sections from that record, interpreted and with `-Xcomp`: three levels variants, two terrain sets, six
runtime seeds (the low halves of level seeds with high bits set), the four pool configurations of the surface biome
oracle, and six build ranges from `128..192` to `-2032..2032`. The test matches every cell of every section (612
chunks per configuration) in debug and release builds.

The biome registry pools (`RegistryBiomeMap`, `biome_registry.rs`, with `BiomePools` in `biome_map.rs`) are
TerraForged 6dd607eb's `BiomeMapManager` with `BiomeUtil`'s classification (MIT). A registry entry is plain data here
(`RegistryBiome`: its key, base temperature, downfall, precipitation and tags); `from_generator_data` reads Horizon's
loaded biome registry and its resolved tags; the live biome bindings below tie the pools to Horizon's biome ids. How
a biome, a mod's included, ends up in a pool:

1. It must be in the overworld list: tagged `minecraft:is_overworld`, or named by a climate file. A mod biome with
   neither is never placed.
2. If climate files name it (`data/terraforged/worldgen/climate/<climate>.json`, `{"namespace:path": weight}`, a bare
   or `:`-prefixed path meaning `minecraft:`), it goes into each of those climates at that weight and nowhere else,
   ahead of the climate's other biomes. A file's biomes enter the pool in `ClimateType`'s hash-map order (fastutil's
   `Object2FloatOpenHashMap` over `Identifier.hashCode`), not file order. A name missing from the registry is an
   error (Java's `getHolderOrThrow` throws in the constructor); the port reports the first one in the climate
   registry's order, which 26.3 loads by `Identifier` (`terraforged:alpine` first), each file in its hash-map order.
   TerraForged ships all eleven files empty (`{}`), so by default every biome is placed automatically.
3. Otherwise it is placed at weight 1 in the first climate whose rule applies: `is_badlands` or
   `has_structure/village_desert` gives desert; `is_savanna` savanna; `is_jungle` tropical rainforest; `is_taiga`
   tundra if it snows, else taiga; `is_forest` tundra if it snows, temperate rainforest at a downfall of 0.8 or more,
   else temperate forest. **Anything else is grassland**, whatever its temperature and downfall: that is where an
   untagged mod biome (most carry none of these tags) goes. Ocean, river, beach and cave biomes are not filtered out
   (they sit in grassland with the plains); with Horizon's `corrected_biomes` setting, Reimagined's
   `isClimateBiome` keeps them out ([Settings](#settings)). Biomes are placed in `BIOME_SORTER` order: path first, then namespace
   (`terralith:alpine_grove` comes before `minecraft:badlands`, and `a:plains` before `minecraft:plains`).
4. Every climate has a pool, and an empty one stays empty: no climate borrows another's biomes (`getInitialBiome`
   gives plains for it). With 26.3's biomes and no climate files, cold steppe, steppe and alpine are empty.

The registry's iteration order does not change the result: the overworld list is sorted and the files are hash maps.
"It snows" needs a 26.3 stand-in: 1.18's biomes stored their precipitation, 26.3's keep only `has_precipitation` and
decide snow per position, so the port takes TerraForged Reimagined a24d450's position-free `BiomeUtil.getPrecipitation`
(MIT, as above): precipitation and a base temperature below 0.15, vanilla's own threshold. Every list and pool holds
the same `Arc` per biome. Not ported: climate files named after no climate type (Java adds their biomes to the
overworld list, and throws on an unknown one, but gives them no pool), and the codec's own checks (an invalid key or a
non-number weight throws in Java). The pipeline decodes no climate files: it builds the pools with none
(`pipeline.rs` passes `&[]` to `LiveBiomeBindings::from_generator_data_with`), the same pools as TerraForged's eleven
empty files, so a data pack's climate files (`data/terraforged/worldgen/climate/*.json`) are not read. Reimagined's version of the manager (its `terraforged:non_surface`/`wetland`/`highland` tags,
cave and water filters, climate lookup for untagged biomes, `fillEmptyClimates` borrowing and dry and highland pools)
is not followed; 6dd607eb's sampler reads none of it.

`BiomeMapManagerOracle.java` (`.local/terraforged/oracles/biome_registry/6dd607eb/`, `run.sh`) compiles 6dd607eb's own
`BiomeMapManager`, `BiomeUtil`, `ClimateType` (climate files go through its codec), `LazyCodec`, `WeightMap` and
`MathUtil`, renamed to 26.3's registry API by `patch.py` (nothing else changes; `patch.diff` lists every edit), and
runs them interpreted and with `-Xcomp` over real 26.3 `MappedRegistry<Biome>`s: the server jar's 67 biome files and
its biome tags (bound through 26.3's `TagLoader`), with no climate files and with TerraForged's eleven empty ones;
those plus 16 mod-like biomes (untagged, a `cave/` path, forest, taiga, jungle and ocean tags, one outside the
overworld, `a:plains`), registered forwards and backwards; with climate files (bare, `:`-prefixed, ignored,
non-overworld and untagged keys, and a 30-entry file that makes the hash map grow); four sets of unknown keys; and
3,000 random registries and climate files (registered in random order). The tests (the vanilla ones through
`from_generator_data` over Horizon's prepared bundle, skipped without it) match the overworld list, every pool's
biomes, order and weight bands, and the unknown key reported, in debug and release builds.

The live biome bindings (`LiveBiomeBindings`, `live_biomes.rs`) bind what TerraForged Reimagined a24d450 looks up in the
biome registry to Horizon's numeric biome ids (MIT, as above): the pools above, `minecraft:plains` (`getInitialBiome`'s
fallback for an empty pool), and the 13 ocean, beach, shore and river biomes `getBiomeOverride` returns. They hold no cave biome sampler: the chunk biome fill has no cave layer (TerraForged's noise-cave
carver, a later layer, builds its own). `from_generator_data`
reads the same loaded registry (`GeneratorData`) the generator uses, so the ids are the ones Horizon's generator writes
into chunks and maps by name for saves and the network; a mod's biomes in that data are bound like vanilla's. The
registry does not change while a world is loaded, so the bindings are built once per world. Keys match by namespace
and path exactly. A missing key is an error (first a climate file's, as the pools above), and no other biome stands in for it. Reimagined calls `getOrThrow` when a
column first needs the biome and throws there; the port checks all 14 keys up front and refuses the registry. Every
list, pool and lookup holds the registry entry's own `Arc`. `surface_sampler` and `create_biomes` run the chunk biome
hook above over these bindings, with the climate pools (`getBiomeMap()`) as 6dd607eb's sampler reads them.

`LiveBiomesOracle.java` (`.local/terraforged/oracles/live_biomes/no_cave/`, `run.sh`) compiles Reimagined's own
`BiomeMapManager` and `BiomeUtil` against the biome registry oracle's stubs. It runs them
interpreted and with `-Xcomp`, each registry listed in 26.3's `Identifier` order, over: 26.3's biomes; those plus 8
mod-like ones (`terraforged:cave`, `cave/` and `_caves` paths, a tagged cave biome, look-alikes that are not); that set
without each of the 14 keys in turn; and 2,000 random registries. It prints the 14 bound biomes with their base
temperatures (or the first key whose lookup throws). The tests match every line (the vanilla ones
through `from_generator_data`, skipped without the bundle), in debug and release builds.

There are no terrain settings at generation time. TerraForged 6dd607eb's `GeneratorPreset` hands the terrain-noise
registry's entries (`RegistryKey.entries`, registry order) to its `NoiseGenerator` unchanged, and `createLandTerrain`
builds the terrain blender with fixed values (seed `TERRAIN_OFFSET`, scale 800, jitter 0.8, blending 0.4; the
preset layer wires it). The vertical scale and fancy mountains are Engine `TerrainSettings` that `ModTerrains` used when
it generated the noise files (above), so they are already in the bundled noises, and the weights are in the files too.
TerraForged Reimagined adds a Terrain settings page whose `GeneratorPreset.applyTerrainOverrides` reweights and rescales
terrains, widens the mountains by a mountain width (3.5 by default) and drops the mountain ridges when fancy mountains
are off. That pass is not followed, so the default terrain is 6dd607eb's (Reimagined's defaults would stretch dales,
dolomites, the mountains and torridonian).

The terrain assets (`decode_terrain_asset`, `terrain_asset.rs`) are TerraForged Reimagined a24d450's
`TerrainNoise.CODEC` and `NoiseCodec.CODEC` (MIT, as above), reading a `worldgen/terrain_noise/*.json` file as
Minecraft 26.3 loads it (strict JSON). `type` must be a string and is parsed like `Identifier.read` (no namespace, or
a leading `:`, means `minecraft`); the caller's `resolve` gets the full id and does the registry lookup, which is the
next layer's job (inline `TerrainType` objects are not supported). `weight` is a JSON number read with
`Float.parseFloat`. `noise` goes through Cereal as `SuperCodec` runs it: DFU's `JsonOps.convertTo` first rebuilds every
number as a `long`, `float` or `double` (Gson's `BigDecimal` limits apply), then each spec reads its fields with
Java's `intValue`/`floatValue`/`doubleValue`; a missing or non-numeric scalar takes the spec's default (a non-number
reads as 0), enums must match a constant exactly, a missing child is an error, a combiner skips non-object entries,
and JSON `null` anywhere in the graph is an error. Nodes are built with their Java constructors, which never skip a
node as `Module`'s shortcut methods do (`Module::new_alpha`, `new_bias`, `new_boost`, `new_clamp`, `new_map`,
`new_power`, `new_scale` in `noise.rs`, shared with reseeding). The specs' quirks are kept: `Bias` reads its bias as a
float, so the module bias Reimagined's own assets write reads as 0 (every bias in the 14 shipped assets is dropped,
as in Java); `CellEdge` ignores `distance` (always 1); `Boost` runs at least one iteration. Where Java's constructor
throws (a negative octave count for every source but `Cubic` and `CellEdge`, more than 30 for `Ridge` and `Billow`,
negative terrace steps), decoding fails; where Java builds a graph that throws on every sample (`SimplexRidge` above 30
octaves, a terrace with no steps), decoding fails too. Supported are the nodes the 14 Reimagined assets use (the eight
octave sources, `CellEdge`, `Const`, `Alpha`, `Bias`, `Boost`, `Clamp`, `Invert`, `Map`, `Pow`, `PowCurve`, `Scale`,
`Warp` with `DomainWarp`/`DirectionWarp`, `Blend`, `Terrace`, `AdvTerrace`, `Steps` with an `Interpolation` curve)
plus `Min` and `Sub`, and Engine 0.3.0's erosion `Valley` (`Ridge.create`: seed 1337, one octave, strength 1, grid
size 100, amplitude 0.5, lacunarity 2.25, fall-off 0.75 and blend `CONSTANT` by default, read with `asInt`/`asFloat`,
then `new Ridge(...)` and `wrap(source)`; zero or negative octaves sample NaN and the octave count is not capped,
as in Java). TerraForged 6dd607eb's
`NoiseCodec` registers `Valley` and its generated mountain assets use it; Reimagined a24d450's `NoiseCodec` dropped
that registration (its comment calls it `Ridge`), so Reimagined itself rejects a `Valley` node: the port follows
6dd607eb here. The other specs Reimagined registers (`Cell`, `Sin`, `Line`, `Rand`, `Abs`, `Cache`, `Curve`,
`Freq`, `Grad`, `LegacyTerrace`, `Modulate`, `Threshold`, `VariCurve`, `Base`, `MultiBlend`, `Select`, `VariBlend`,
`CumulativeWarp`, and the `MidPointCurve`/`SCurve` curves) decode in Java but return an "unsupported" error here.
`TerrainAssetOracle.java` (`.local/terraforged/oracles/terrain_asset/`, `run.sh`) compiles Reimagined's Cereal, noise
package, `DataUtil`, `SuperCodec`, `NoiseCodec` and `TerrainNoise` from source against Minecraft 26.3's server jar
(`StrictJsonParser`, `Identifier`, DFU, Gson), runs 181 cases (defaults, number forms and limits, enum and identifier
errors, constructor edge cases, 28 with `Valley`) and the 14 assets interpreted and JIT-compiled, and prints each decoded id, weight,
bounds and a hash of 294 seed-first samples over seven seeds (`sync.py` copies its cases and output into the tests).
The tests match every line, reading "err" where Java decodes a graph that only throws; the asset lines are checked
when Reimagined's sources are unpacked locally (the assets are not copied into this repository). For `Valley` it also
compiles Engine 0.3.0's `Ridge` (spec and factory unchanged; Engine's sampling has only the jar's seedless
`getValue(x, y)`, so it samples seed-first with the convention of the seed-first erosion `Ridge` above: lattice seed = own +
call seed) and registers it as 6dd607eb does, and checks 6dd607eb's three generated `mountains_*.json` assets, which
use it (when TerraForged's sources are unpacked locally).

`terrain_asset_registry.rs` loads the `terrain_type` and `terrain_noise` registries from (registry key, JSON) pairs,
after TerraForged Reimagined a24d450's asset `TerrainType` (`DIRECT`, `forName`, its engine `TerrainType.get` and
`getOrCreate`) and `RegistryKey.entries`. Keys are read as 26.3 `Identifier`s (no namespace means
`minecraft`); an invalid or repeated key is an error. Entries are decoded in registry order, `Identifier.compareTo`
(path, then namespace), so the terrain ids they get do not depend on the input order, and the noises come back in
that order. A terrain type's `parent` names a registered terrain by its exact name, else `none` (not an error, as in
Java); its `name` returns the terrain already registered under it (keeping that terrain's own parent) or registers a
new one delegating to `parent`. A terrain noise whose `type` is not in the loaded types is an error. Differences from Java:

- Minecraft 26.3 decodes a registry's files in parallel, so in Java a parent that is itself a terrain-type asset
  resolves only if its file happened to be decoded first. Here a parent must sort before the types naming it.
  Reimagined's own assets only name built-in parents.
- The registry is Engine 0.3.0's (the engine TerraForged 6dd607eb links, so the port follows it), not Reimagined's
  rewritten engine. Engine's names compare ignoring case, so a
  name that differs only in case from a registered terrain (Reimagined registers `Hills` beside `hills`) is an
  error. Its built-ins differ from Reimagined's: Engine has `ocean` (Reimagined: `shallow_ocean`) and
  `mountain_chain`, `volcano` and `volcano_pipe`, which Reimagined lacks (so its `ocean`, `mountain_chain`, `volcano`
  and `volcano_pipe` assets make new terrains under `none` there, and keep the built-ins here). Engine's `badlands`
  is a flatland and `plateau` a lowland; Reimagined makes `badlands` a lowland and `plateau` a highland.
- On an error the types decoded before it stay registered (Java fails the whole load): discard the registry.

`TerrainAssetRegistryOracle.java` (`.local/terraforged/oracles/terrain_asset_registry/`, `run.sh`) compiles
Reimagined's asset and engine `TerrainType` from source against Minecraft 26.3's server jar (strict JSON, DFU,
`Identifier`) and runs 31 terrain-type cases in one registry (built-in, custom, unknown and case-variant parents,
existing names, duplicate JSON keys, wrong field types) and 4 key-order cases, interpreted and JIT-compiled; the
tests match every line except the two case-variant names, which are errors here (the key-order lines also print
Reimagined's `entryIds`, which nothing here uses, so the tests compare only the order).

`current_terrain_resources.rs` bundles TerraForged Reimagined a24d450's 18 `terrain_type` files
(`src/main/resources/default/data/terraforged/worldgen/terrain_type/`, MIT) byte for byte in `data/worldgen/terrain_type/`;
horizon-mixins stages a bundle's `data/` folder under `native-data/terraforged/`, and `data.rs`'s `terraforged_data!`
embeds them in the build, so no TerraForged checkout is needed at runtime. `decode_current_terrain_types(&mut registry)`
decodes them under their `terraforged:` registry keys into the world's terrain registry (before it is shared); a second
call registers nothing new. The generator (`generator_preset.rs`) decodes them into `TerrainRegistry::new()`, Engine
0.3.0's built-ins, the registry TerraForged 6dd607eb links and generated these 18 files from. Reimagined's rewritten
engine has other built-ins, so 12 of the 18 types come out as in Reimagined and 6 differ (Reimagined vs Engine):
`ocean`, `mountain_chain`, `volcano` and `volcano_pipe` stay Engine's built-ins (Reimagined: new terrains under
`none`), and `badlands` and `plateau` keep Engine's categories. `CurrentTerrainResourcesOracle.java`
(`.local/terraforged/oracles/current_terrain_resources/`, `run.sh`) decodes Reimagined's own files with its asset and
engine `TerrainType` (registry order and three shuffled orders give the same result, interpreted and JIT-compiled) and
checks the bundled copies are byte-identical; the test matches its 12 agreeing lines and pins the 6 differences.

It also bundles all fourteen `terrain_noise` files, byte for byte in `data/worldgen/terrain_noise/`. Eleven are Reimagined's
(same folder in Reimagined, `terrain_noise` instead of `terrain_type`, MIT): the flatlands `plains` and `steppe`, the
hills `dales`, `hills_1`, `hills_2` and `torridonian`, the mountain ridges `mountains_ridge_1` and `mountains_ridge_2`,
`dolomites`, `plateau` and `badlands` (6dd607eb generates the same bytes in `generated/.../worldgen/terrain/noise/`). The mountains
`mountains_1`, `mountains_2` and `mountains_3` are TerraForged 6dd607eb's generated files (MIT): Reimagined's copies
differ (they replace the `Valley` erosion node, which Reimagined's codec cannot read, with other nodes), and the port
follows the newer source. `decode_current_terrain_noises(&types)` decodes them with #185's decoder, their `type`
(`terraforged:flats`, `terraforged:hills`, `terraforged:mountains`, `terraforged:torridonian`, a child of `hills`, or
`terraforged:dolomites`, a child of `mountains`, `terraforged:plateau` or `terraforged:badlands`) resolved in the terrain types
`decode_current_terrain_types` returned (so the terrains are the world registry's), into a `TerrainNoiseAssets` in
registry order (`badlands`, `dales`, `dolomites`, `hills_1`, `hills_2`, `mountains_1`, `mountains_2`,
`mountains_3`, `mountains_ridge_1`, `mountains_ridge_2`, `plains`, `plateau`, `steppe`, `torridonian`), the order the
noise generator's blender takes them in. The generator decodes them (`generator_preset.rs`).
`CurrentTerrainNoisesOracle.java` (same oracle folder, `run.sh`) decodes each bundled file from the tree it comes from
(6dd607eb's three mountains, Reimagined's copies of the rest) with Reimagined's real `NoiseCodec`, plus Engine 0.3.0's
`Valley` registered as 6dd607eb does, and `TerrainNoise` into its terrain-type registry, and checks each bundled file
is byte-identical to its source (and that the three 6dd607eb files are not Reimagined's); the test matches its lines
(terrain, registry identity, weight, raised bounds and a hash of 294 seed-first samples over 7 seeds), except for
two terrain columns (the registry difference above): `plateau`'s terrain is Engine 0.3.0's lowland built-in here and
Reimagined's highland there, and `badlands`' is Engine's flatland here and Reimagined's lowland there. Lowland and
highland have the same predicates in both engines (overground only). Flatland adds `isFlat`, and Engine's `badlands` also
has an erosion modifier of 0.3 (Reimagined's: 1). Of the ported code only Engine's composite terrains read `isFlat` and
only Engine's erosion filters' `Modifier` reads the modifier, and only on the Engine 0.3.0 tile path. The generator
path (terrain blender, eroded noise, chunk fill, biomes) reads neither, so the bundled noises generate the same
heights either way. The noise, weight and bounds match, so the test pins the two terrain columns as known
Reimagined-vs-Engine differences: the port keeps Engine 0.3.0's registry, as 6dd607eb links it.

`world_preset.rs` adds TerraForged's `terraforged:normal` world preset to the generator's `world_presets` bundle (a
RETURN hook on `WorldgenOverrides::apply`; Horizon reads no data-pack world presets, so the bundled one is used). Its file,
`data/worldgen/world_preset/normal.json`, is TerraForged 6dd607eb's generated preset byte for byte (TerraForged
Reimagined ships the same bytes, MIT): an overworld with a `terraforged:generator` and its `levels`, and vanilla's nether
and end. `pipeline.rs` overwrites `OverworldGenerator::for_world` and `for_dimension` (horizon-mixins cannot cancel a
function returning `Self`, so a HEAD hook is not possible): a dimension of that preset whose level stem uses
`terraforged:generator` gets a `TerraForgedPipeline`. The stem is picked as Horizon's `build` and 26.3's
`WorldDimensions.bake` pick it, a data pack's over the preset's, so a pack's own overworld keeps its generator. Every
other preset, dimension and stem goes through Horizon's own `build` unchanged.

The pipeline generates with TerraForged 6dd607eb's `Generator`. `generator_preset.rs` reads the stem's `levels` as
`TerrainLevels.CODEC` does (absent: `TerrainLevels.DEFAULT`; out-of-range or mistyped fields are errors, as 26.3's
strict `optionalFieldOf` makes them) and builds `GeneratorPreset.build`'s noise: the 14 bundled terrain noises in
registry order, `createLandTerrain`'s blender (`TerrainBlender(45763218, 800, 0.8, 0.4)`), `createContinentNoise`'s
continent (`ClimateSettings::terraforged()`, continent scale 400, its control points) and the erosion stage. Terrain
types, continent and blender share one terrain registry, Engine 0.3.0's built-ins (`TerrainRegistry::new()`), as
6dd607eb links Engine 0.3.0.
The erosion stage is TerraForged Reimagined's region erosion run with 6dd607eb's `withErosion` settings: Engine
0.3.0's `FilterSettings.Erosion` (droplet lifetime 12, volume and velocity 0.7, erosion and deposit rates 0.5) with
350 droplets per chunk, checked against the jar (`.local/terraforged/oracles/generator_preset/`), not Reimagined's
defaults (lifetime 25); Reimagined's heightmap smoothing stays off, as 6dd607eb has none. Each chunk then runs the statuses in 26.3's order:
`createBiomes` (TerraForged's surface biome per quart column, at every height, from the registry pools; it hints
the terrain cache for that chunk only, and the 3x3 neighbours the surface rules read are sampled again without a
hint), `fillFromNoise` (stone and water from the terrain cache, `primeHeightmaps`' WG heightmaps) and
`buildSurface`: the vanilla overworld noise settings' surface rule (`VanillaGen`) with the preliminary surface
heights from `NoiseChunkUtil.initSurfaceCache` and 26.3's `WorldGenerationContext(generator, level)` (sea level 62),
then `Surface.apply` (gen depth 480, `#minecraft:substrate_overworld` and `#minecraft:snow` erodible, as Reimagined chose). Decoration is TerraForged's (`decorator.rs`, below);
after it the chunk's terrain-cache entry is dropped, as `applyBiomeDecoration` does. `getSeaLevel()` is
TerraForged's 62 and `Source.getNoiseBiome` answers biome queries. Structures are placed by 26.3's structure placement
over TerraForged's queries (`structure_queries.rs`: `getBaseHeight`, `getBaseColumn` and `Source.getNoiseBiome` over
the overworld biome list, `possibleBiomes`), and the terrain stage is `fill_from_noise_with_structures` over
`StructureTerrain::from_starts` of the chunk's references (the beardifier's list); the vanilla half runs without
structures. `/locate biome` is 26.3's `BiomeSource.findClosestBiome3d` (TerraForged's `Source` does not override
it) over `possibleBiomes` (the overworld biome list) and `getNoiseBiome`: the same spiral, step and radius, and since
TerraForged's noise biome ignores `y`, a column answers at the first sample height (Java samples the other heights in
vain, with the same result). It reads only the continent, river and climate noise, so a search adds no terrain-cache
entry and evicts no erosion region (a cave biome's search, below, builds the regions of the columns it checks). A column whose pool has no biome matches nothing; Java's `Set.contains(null)`
throws out of the command there instead. Beyond TerraForged (its search never finds them), the biomes only caves hold
(`terraforged:cave`, unique caves' biomes outside the climate pools: the End's and the Nether's
without `corrected_biomes`, the cave biomes with it) are looked for in the same spiral, after each
column's noise biome: the caves' noise first (`NoiseCaveGenerator.carve`'s height, cavern and floor
and the cave biome sample), and only where a cave would carve a wanted biome, the column's
un-eroded height (no erosion region) and then the eroded height the chunk is filled to (which
builds that column's erosion region). The answer is the lowest carved block of the lowest quart
`carve` leaves that biome in, more than 16 blocks below the surface `getSurface` gives with the river
breach shut (its deepest, so the quart is written whatever the river noise). Other columns of the same
quart and `CarverChunk`'s one-quart biome cache are not replayed, so near a cave biome's border the
quart can hold a neighbour's choice. A biome the climate pools hold is looked for only as a noise
biome, as before, so searches for surface biomes cost what they did. Like 26.3's own search, it runs on the
server thread: the cave biomes' noise goes first (one simplex per cave), so a whole 6,400-block spiral's cave part
(cave noise, and un-eroded heights where a wanted cave carves) takes about 0.03–1.3 s in an optimized build at
seed 777, against 0.37 s for its noise biomes, and a found cave adds one erosion region (up to about 1.5 s; the region the answer's chunks need anyway). Structures' own biome searches (`findBiomeHorizontal` for strongholds,
biome checks) already go through `Source.getNoiseBiome` in `structure_queries.rs`. Distant terrain (Horizon's far tiles, `lod_grid` and `surface_sample`) reads the same continent, river and
climate noise: each column is `getBaseColumn`'s shape from the UN-ERODED height (`far_heights`), with the biome the
chunk stores at its surface block and the vanilla surface rule over TerraForged's generation context and preliminary
height painting its top eight blocks (two with Reduced far detail, the `approximate` flag); it too adds no terrain-cache entry and builds no erosion region. With `eroded_far_terrain` the finest tiles (`lod_grid` with cells of up to 4 blocks) read the ERODED height where the column's region is built, in the pipeline's own region cache (`ErosionRegionCache::peek`: no build, no wait, no change to the access order) or in the world's far cache (`FarErosion`, shared with the noise by every pipeline of the world, with its own thread and 256 regions of their 128x128 heights), and queue the region otherwise; such a column is exactly `base_column`'s, the filled ground (`far_eroded_heights`). A TerraForged world's overworld is TerraForged's taller one (`-64..960`, the
world type below), so the default levels (`max_y` 480) always fit; nothing is rescaled, and in a 26.3-high overworld
(the generator without the world's pack) the fill clips terrain to the chunk (cut flat at y 319, which the default
levels seldom reach: the highest un-eroded heights within 32768 blocks of the origin are about 310 to 350), and the
preliminary surface heights are clipped the same way. The vanilla surface rules write over the whole level, as
`SurfaceSystem.buildSurface` walks each column down from its own top, not only over the vanilla noise range. The preliminary heights hold per 4x4
column cell, as the cache stores them; 26.3's surface context reads one value per column. `getBaseHeight` is clipped
the same way (to the level's `min_y ..= min_y + height`, where the clipped fill's heightmaps end), so a structure is
placed where the filled chunk has its ground; `getBaseColumn` is clipped to the same range, from the level's bottom, and
the structure checks read it over the whole level (the taller overworld keeps the vanilla noise settings' `-64..320`).
A test gives the overworld the taller dimension type and checks that a chunk under a peak keeps its ground above 320
(the same chunk in a 26.3-high overworld is cut flat at 319) and that `getBaseHeight` and `getBaseColumn` reach it. A
test generates a chunk
twice in one world and once in a second world, checks the blocks and biomes are equal, the blocks differ from the
normal overworld, and every quart's biome is `getNoiseBiome`'s except where a noise cave carved it.

After the surface the pipeline carves TerraForged's noise caves (`cave_carver.rs`: 6dd607eb's
`NoiseCaveGenerator.carve`, `NoiseCaveCarver` and `CarverChunk`). 6dd607eb's `applyCarvers` replaces the vanilla
carvers, so none run. 26.3 has a single `ChunkGenerator.buildTerrain` for fill, surface and carvers (TerraForged
Reimagined runs them in that order too), on a chunk whose persisted status is still BIOMES, so the carver's writes
update the WG heightmaps and later caves read the lowered OCEAN_FLOOR_WG. The cave array (`current_caves()`) and
`CaveBiomeSampler(800, …)` are built once per world: GLOBAL caves draw `terraforged:cave` (from the world's pack; a
registry without it leaves their pool empty) and UNIQUE caves every registered biome in 26.3's registry order (6dd607eb's UNDERGROUND
filter is commented out; `corrected_biomes` takes Reimagined's `isNonSurface` biomes instead). Each cave, in array order, goes through the chunk's 256 columns with the `(int)` level
seed: the surface is OCEAN_FLOOR_WG's first free block minus 2, raised by 9 off the sea-level band and lowered by
`NoiseUtil.floor(16 * mask)` (mask = 1 - breach noise × river), the cavern size takes the unique-cave noise for
UNIQUE caves, and `MathUtil.clamp` bounds top and bottom; every non-fluid block in between becomes air through
`set_block_worldgen`, and quarts more than 16 blocks below the surface take the cave's biome. `CarverChunk.getBiome`'s
single cache is shared by every cave and resamples whenever it holds null. Differences: where the pool is empty
Java writes a null biome into the chunk (which fails later in Java); the port keeps the quart's biome and leaves the
null out of the cave's biome list. Blocks and quarts outside the build range are skipped (Java would throw for a
quart below it, which only a generator `min_y` under the chunk's reaches). Each chunk's CarverChunk (its caves' biome
lists) is kept until the chunk is decorated, as `NoiseCaveGenerator.cache` keeps it. `CaveCarverOracle.java`
(`.local/terraforged/oracles/cave_carver/`, `run.sh`; needs `../seed_first/run.sh` first) runs 6dd607eb's own
classes (patched mechanically, the diff in `patch.diff`) with the bundled caves on real 26.3 ProtoChunks, heightmaps
and biome containers: 48 scenarios (build ranges, generator `min_y` and sea level, BIOMES and TERRAIN statuses,
primed or unprimed heightmaps, seeds, chunks where unique caves form, caves in reverse registry order, with and
without `terraforged:cave`, river maps, fluid pockets); the test matches its blocks, biomes, heightmaps and per-cave
biome lists (including a `carve = false` replay) bit for bit, in debug and release. Another test generates four chunks
with unique caves twice in one world and once in another, checks the blocks and biomes are equal, and that cave biomes were written (each in a carved quart).
TerraForged's decoration (`decorator.rs`, spliced into Horizon's decoration module) is 6dd607eb's
`BiomeGenerator.decorate`: `FeatureDecorator`, then `NoiseCaveGenerator.decorate`, `Surface.smoothWater` and
`Surface.applyPost`, all while the chunk's terrain-cache entry is held; the entry is dropped after them. Unlike
vanilla's sorted, chunk-wide feature order, `FeatureDecorator` decorates the whole chunk with the one biome at its
lowest corner (`SectionPos.of(chunkPos, level.getMinSection()).origin()`, read through the biome zoom) and a legacy
`WorldgenRandom` seeded with `setDecorationSeed(levelSeed, x, z)`. `VanillaDecorator` runs the steps before the
vegetal one (0 to 8) and after it (10): a step past the biome's last is skipped, structures included; otherwise each
structure registered for the step gets `setFeatureSeed(seed, i, step)` and places its starts in the chunk, then the
biome's features of that step get `setFeatureSeed(seed, structures + i, step)`, where `structures` counts every
structure registered for the step (the same whether or not the world places structures). The vegetal step is
6dd607eb's `PositionSampler` (below). For each noise cave, in array order, `NoiseCaveDecorator` places every
biome the cave passed through (its CarverChunk list, kept from the chunk's carving) at the chunk's corner and the
cave's start height, from `LOCAL_MODIFICATIONS` on, with a fresh legacy random per cave and `setDecorationSeed` per
biome (no structure offset). Every placement is `placeWithBiomeCheck` in 26.3's `WorldGenerationContext(generator,
level)` (sea level 62). Because the unique caves give quarts any registered biome (the nether's and the end's too),
the decoration knows every registered biome's features, as `BiomeFilter` reads `biome.getGenerationSettings()`.
Differences: Java orders a step's structures by `MappedRegistry.entrySet()`, a `HashMap` of identity-hashed
`ResourceKey`s, so the order (and each structure's seed) changes from run to run; the port uses registry order, as
vanilla's `applyBiomeDecoration` does. A chunk carved by an earlier run has no CarverChunk, and its caves are replayed
without writing (`getPostCarveChunk`), as in Java; the replay reads the carved chunk's OCEAN_FLOOR_WG, so where a
cave breached the surface its lists can differ from the first run's, as in Java. A cave whose pool is empty
records no biome (only GLOBAL caves in a registry without `terraforged:cave`, e.g. a TerraForged world whose pack
was disabled; Java always has it, and would record a null and fail on it). `DecoratorSeedOracle.java`
(`.local/terraforged/oracles/decorator/`, `run.sh`) runs the three classes' seeding loops (copied verbatim, with three
draws in place of each placement) on 26.3's own `WorldgenRandom`, `LegacyRandomSource` and `SectionPos`, for 6 seeds,
6 chunks (extreme coordinates included), 4 minimum heights, 4 step tables and 3 structure tables; the test matches its
seeds and draws through the decorator's own ordering. Tests decorate a chunk with unique caves twice in one world and
once in another (the same blocks, biomes, block-entity positions and entity counts), with its CarverChunk taken out and with a
replay, and check every biome's features and the per-step structure counts.
The vegetal step (`PositionSampler`, in `decorator.rs`) uses `BiomeVegetationManager`'s per-biome vegetation, built
once per world: the first bundled config whose tag (bound as above) holds the biome, and `VegetationFeatures` over the
biome's `VEGETAL_DECORATION` list, with a configured biome's trees unwrapped (their biome, count and square
modifiers dropped). `populate` reads every column's top block (`OCEAN_FLOOR_WG`) and biome, and the column's fitness
under that biome's config (1 without one), noting the biomes in the order met. Each biome then runs with feature
seeds `setFeatureSeed(seed, offset, 9)`, where `offset` adds the biome's index and each success: a river biome
(`#minecraft:is_river`) places nothing; a biome without a config places its trees and grass once at the chunk's
corner (above sea level); a configured one places every tree at each point of a grid of `frequency` cells (rows
squashed by `2 / sqrt(3)`, odd rows shifted half a cell, each point jittered by its cell's `hash2D((int) seed, px,
pz)`) that falls in the chunk, above sea level, in that biome, and whose fitness is not under `(1 - density) *
rand(hash)`; then its grass at the corner at y 0, in at least two passes (`2 + 2 + floor(4 * river) - floor(5 *
height)` at the chunk's centre, so 2 wherever that height is 2 or more). Last, the origin biome's other vegetal features place at the origin.
Differences: where a grid bound saturates at `i32::MAX` (a frequency of 1 or more at the last chunks before x 2^31;
the bundled configs' 0.15 to 0.35 never do), Java's `px <= maxX; px++` loop never ends; the port's stops.
`PositionSamplerOracle.java` (`.local/terraforged/oracles/position_sampler/`, `run.sh`) compiles 6dd607eb's own
`PositionSampler`, `SamplerContext` and `BiomeList` over stub worlds, on 26.3's `WorldgenRandom`: per seed (6) and
chunk (6, extreme coordinates included), every placement's feature, unwrapping, position and draws over four biomes
(no config, configured, a river, and 11 grid configs) and four grass-pass counts, and every grid cell's hash and
point. A test drives the port's sampler over the same stubs and matches it bit for bit; another decorates a wooded
region twice in one world and once in another and checks the blocks match and hold tree logs.
`client_world_type.rs` (a module of horizon-client's `save.rs`) offers the preset on the Create World screen. 26.3's
world tab cycles through the `#minecraft:normal` world-preset tag: vanilla's five presets, then the
`terraforged:normal` entry that TerraForged's tag file adds (both trees ship the same file; Horizon reads no
world-preset tags, so the order is written into the cycle). The button therefore goes Single Biome, TerraForged,
Normal. Horizon's world types are a fixed enum, so TerraForged is held beside it as Normal plus a flag. The button
shows `generator.terraforged.normal` (vanilla's `toLanguageKey("generator")`), whose text in TerraForged's
`en_us.json` is "TerraForged" (6dd607eb; Reimagined has the same entry). Customize stays off, as 6dd607eb has no
preset editor. Creating the world (`WorldOptions::world_gen` arms the `write_world_gen_settings_with` that takes its
result) saves vanilla's `world_gen_settings` with the preset's TerraForged overworld stem. Every other write (a world
opened without settings, a legacy world, a dedicated server's new world of a vanilla `level-type`) stays vanilla. The new world also gets its own
data pack, `datapacks/terraforged`, holding TerraForged's taller overworld dimension type (TerraForged Reimagined's
built-in `dimension_type/overworld.json`, bundled unchanged; 6dd607eb's has the same range in 1.18's format): from y
-64, 1024 blocks and logical height. Horizon enables a world's packs unless `datapacks.json` disables them, so the
generator's build range, the level shapes, saving and the synced `dimension_type` registry (LAN and multiplayer
clients get the patched height) follow it as for any pack, and the world reopens with the same height. TerraForged's
built-in pack also holds its worldgen registries and its world preset; the port bundles those, so only the dimension
type and TerraForged's two biomes are copied. The biomes are 6dd607eb's `ModBiomes` (`mod_biomes.rs`), built at
runtime from the player's own 26.3 data as `ModBiome.create` builds them, so the mod ships no vanilla biome file:
`terraforged:cave` is 26.3's dripstone caves with only large dripstone and pointed dripstone and no carvers,
`terraforged:oak_forest` is 26.3's plains unchanged (the parent's climate, effects, spawns and generation are kept).
The parents are read from the resources folder the game reads vanilla's biomes from (a HEAD hook on Horizon's
`Biomes::load` remembers it); a parent that can't be read leaves its biome out, with a warning. The game's biome table, the synced registry and the commands take them through a
RETURN hook on Horizon's `world_biomes::native_biome_files`, for TerraForged worlds only (and, for a client of another
server, as far as that server's registry names them); the generator reads them from the world's pack, so GLOBAL caves
get `terraforged:cave` and the UNIQUE pool (every registered biome) holds both. A TerraForged world made before the
pack held the biomes (or before it had a pack) gets them in its generator too: a HEAD hook on Horizon's
`WorldgenOverrides::finish` (`mod_biomes.rs`, `join_table_biomes`) adds each of TerraForged's biomes the world's biome
table holds and its packs lack to the packs' worldgen entries, built from the 26.3 parents as the table's copy is, so
the table and the generator always agree: GLOBAL caves get `terraforged:cave` and the UNIQUE pool holds both, in the
chunks such a world generates from now on (its saved chunks keep their biomes). Nothing is written to the world and
its pack stays as it is (Re-Create writes the current one); a pack's own file for a biome is kept; a new world's
entries are unchanged, and a vanilla world's table holds neither biome, so its packs gain none. This fix is the port's
own, not parity: TerraForged copies its pack only as a world is created, so Java worlds of older mod versions lack its
newer files the same way.
TerraForged's `DatapackHook.injectDatapack` copies its built-in pack into every world the Create screen makes,
whatever the preset; here only TerraForged worlds get it (Arthur's choice), so vanilla world types keep 26.3's
overworld. Worlds made before this keep their height; disabling the pack gives the overworld 26.3's height again. The screen copies the packs its Data Packs screen showed into the world first: a `datapacks/terraforged`
folder holding exactly this pack (Re-Create's copy) is kept, and any other (the player's own pack of that name) is left
alone while TerraForged's pack takes the next free name (`terraforged-1`, ...), which Horizon enables as a new pack above
the player's, as TerraForged selects its pack last. Horizon reads the type's build range, sky light and cloud height
(TerraForged's 300): its other fields (the sky, fog and cloud colours) are not used. A world whose saved overworld
generator is `terraforged:generator` reloads as `terraforged:normal` (`WorldGen::preset`), and Re-Create picks
TerraForged again. The world screens' decoration preload (`GeneratorData::prepare_decoration`) prepares the two
decorations the pipeline builds (the stem has no biome source): the vanilla overworld's, for its vanilla half, and
TerraForged's own over every biome. As TerraForged's `CreateWorldScreen.init` hook (`DatapackHook.selectPreset`)
cycles the button to its preset, a fresh screen opens on TerraForged (`reset`, at `WorldOptions::default`), so one
press goes to Normal; Re-Create opens on the world's own type. Differences: that hook runs at every `init` of 1.19's
screen, so in TerraForged a Re-Created vanilla world and a resized screen go back to TerraForged; here only a fresh
screen starts there, and the player's choice stays. A dedicated server's new world is TerraForged's when
server.properties says `level-type=terraforged:normal` (`horizon-server`'s `server.toml`: under `[properties]`), as on a
Java server with the mod, where `level-type` names any world preset and `DatapackHook` adds the built-in pack: Horizon's
`native_level_type_world_gen` hook (before the server reads `level-type` itself) writes it as the Create screen does,
with the stem, the pack and the server's own settings file, and the world then opens, registers `/locateterrain` and `/tf`,
and syncs its taller overworld as any TerraForged world. Other level types, unknown ones included, are left to Horizon. A game built without this bundle opens a TerraForged world as a Normal one
and generates new chunks with vanilla terrain. With the bundle applied, Horizon's own test
`create_world_tabs_follow_vanilla_and_their_choices_reach_the_action` fails, because the screen opens on TerraForged
and the cycle has a sixth preset where the test expects vanilla's five,
and so does `world_biomes::tests::no_native_biomes_without_a_mod`, because a client of another server is offered
TerraForged's biomes.

Boats (`boat.rs`, plus an OVERWRITE of Horizon's `boat_status`, which is `AbstractBoat.getStatus` with `isUnderwater`
inlined) follow TerraForged 6dd607eb's `MixinBoat` (RETURN of `isUnderwater`) and `BoatHook.floatTheBoat`. When a
player's boat is found under water, its floating height under the water surface above it (`getWaterLevelAbove() -
height + 0.101`) is computed. If that height is no more than 0.8 above the boat, the boat moves halfway there (all the
way once within 0.01), and the status checks go on as if it were not under water. So a ridden boat climbs a river's
stepped water instead of sinking. The arithmetic is Java's: the water level and the subtraction are floats, the rest
doubles. `getWaterLevelAbove` is the hook's own copy of vanilla's bytecode, because Horizon's copy returns a double and
ignores `lastYd`. The copy reads the boat's vertical motion as `lastYd`; vanilla keeps the motion from before the last
move's collisions, so the two differ only right after a vertical collision. TerraForged's mixin works in every world; here the hook works only in
TerraForged worlds (Arthur's choice), and other worlds keep vanilla's boats exactly. A window playing its own world on
its server thread (single player, or a LAN host) checks the world's saved overworld generator
(`terraforged:generator`) once, when it joins the world (a HEAD hook on `server_packs_world_pack`). A client of another
server (a LAN guest, a dedicated server) isn't told the generator. It uses the synced overworld dimension type: TerraForged's
pack gives it min y −64, height 1024 and logical height 1024, and a Horizon server syncs only that shape and the cloud height over
vanilla's type (TerraForged's colours never reach the client). So any other server whose packs give the
overworld exactly that shape also floats boats, and a TerraForged world without the pack (made before it existed,
or with it disabled) does not. The deprecated in-frame path keeps vanilla boats. The hook runs where a ridden boat's physics run: on the rider's client (single
player on its server thread, LAN guests and dedicated-server players alike), which sends the boat's position to the
server. A server never moves a player's boat itself. TerraForged's server also floats it, but the client's next move
overwrites that position. Boats nobody rides, or that only mobs ride, are unchanged. The tests compare against a Java
transcription in `.local/terraforged/oracles/boat_hook/` (out of Git).

Commands (`commands.rs`, on Horizon's native command hooks) follow 6dd607eb's `TFCommands`, for operators (level 2):
`/locateterrain <terrain> [radius]`, `/tf export structures` and `/tf regen <radius>`. They are registered only when the open world is a
TerraForged one (its saved overworld generator), so other worlds keep vanilla's command tree; TerraForged registers them
in every world. `/locateterrain` finds the player's level's generator as `GeneratorPreset.getGenerator` does: each
TerraForged pipeline records its generator by level seed and dimension (`live_generator`, which answers with the
newest pipeline that built its own noise, the level's, before the far-decoration ones sharing it), so the Nether and the End
answer nothing, as in Java. The terrain is looked up with `TerrainType.get` (ignoring case), then `NoiseGenerator.find`
searches from the player's block with the radius and `min(100, radius + 50)`; the answer (`Found terrain: … Distance: ….
Teleport`, `Invalid terrain: …`, `Unable to locate terrain: …`) uses TerraForged's literal texts and colours, and the
teleport link runs `/tp x y z` at `getFirstFreeHeight` (the eroded base height, Reimagined's choice taken for
structures). Differences: the search runs on the server thread, but that height builds an erosion region (0.25–1.2 s
optimized), so it runs on its own thread and the answer reaches the player at a later server tick; a radius above 100
makes Java's spiral stick on its first cell and never end (unless that cell matches), and here it answers "Unable to
locate"; Horizon's native commands have no quotable `string` argument or suggestions, so the terrain is a `word` and
nothing is suggested; the radius' minimum is checked when the command runs. `/tf export structures` writes nothing
(TerraForged's export is commented out) and only answers "Exported structure settings".

`/tf preview [size] [zoom]` (`preview.rs`) is Horizon's in-game form of 6dd607eb's `Previewer` window, a developer tool
that only `ContinentPreview.main` opens; like the others it is for operators and only in TerraForged worlds. It draws a `size`×`size` picture (16 to 4096 pixels, default 400) at `zoom`
blocks per pixel (at least 0.0001, as the window's mouse wheel allows; default 1), centred on the player, and writes it to
`terraforged-preview.png` in the world folder. Pixel `(x, y)` samples `posX - (w >> 1) * zoom + x * zoom` (Java's float
order), coloured by `ColorUtil.getBiomeColor(sample, 0.2f)`: deep blue to the shallow ocean, light blue to the beach and
on rivers, else the climate type's `biomes.txt` colour shaded by the biome noise (Java's unmasked int channels kept).
The answer gives the window's corner label (the area in blocks). Unlike `ContinentPreview`, which builds a generator of
its own at the fixed seed 273651 on `new TerrainLevels()`, the picture is of the player's world: the level's live
generator (never a second one), at its runtime seed, through the world's `IBiomeSampler.Sampler` at each pixel's block
(`NoiseUtil.floor`). `ContinentPreview` samples the climate at the raw coordinates and the continent at
`floor(x) * frequency`; that only lines up at its frequency of 1, and the bundled preset's is 194/418. Rendering runs on
its own thread over all processors but one, one preview at a time (about 0.5 s for 400×400, 42 s for 4096×4096 in an
optimized build; the server keeps ticking), and the answer reaches the player at a later server tick. The tests
compare colours and whole pictures against 6dd607eb's `ColorUtil` and the real `Sampler`, in
`.local/terraforged/oracles/preview/` (out of Git).

The Create World screen's map (`create_preview.rs`, over `terraforged_preview::new_world_map`) is Horizon's: the 1.18.2-0.3.1
jar and 6dd607eb show nothing on that screen, and TerraForged Reimagined's `PreviewWidget` (160 pixels over 38400 blocks
at its first zoom, relief shading and seven render modes) belongs to its settings screen, which is not ported. With
TerraForged as the world type, the World tab draws `/tf preview`'s picture of the world the screen would make: 64×64
pixels at 64 blocks per pixel centred on 0, 0 (around spawn: the continent's spawn offset puts a valid spawn cell
there), shown up to 128 GUI pixels under the seed box and hidden on screens too short for 32. The seed is the text
Create World turns into the level seed (a number, else the text's Java `hashCode`, trimmed), and the generator is built
as the world's pipeline builds it: the preset's levels with `height_scale`, the settings the new world gets (Re-Create's
source world's, else `config/terraforged/settings.json`), and `Seeds.get` of the level seed. Its colours read only the
climate, continent and river noise, so no setting changes the map (the tests check every setting). A blank seed shows
"the map shows once a seed is typed". The picture renders on a thread of its own, never the screen's, in well under a second:
one render at a time for the latest seed, and a render for a seed already replaced, or for a screen no
longer drawing the map, stops at its next pixel (TerraForged Reimagined's `PreviewWidget` takes the latest request the
same way). While it renders, the previous seed's picture stays, dimmed. It goes to the screen through a world-list icon
slot, which the world list fills again each frame it draws, so it holds no texture of its own. In game, seed 777's map
equals `/tf preview 64 64` taken from 0.0, 0.0 pixel for pixel (from a centred `/tp 0 ~ 0`, `NoiseUtil.floor` takes the
row below on the negative side, as Java does).

`/tf regen <radius>` follows 6dd607eb's `Regenerator.regenerateChunks`: the square of chunks within the radius of the
player's chunk (`(int) x >> 4`, Java's truncating cast) is deleted from the save (`chunkMap.write(pos, null)` = a delete
on the region writer's queue, behind every earlier write, which loads see at once) and from memory (columns, proto
chunks, block entities, scheduled ticks), and the level's own streaming and generator generate it again; players in the
level forget the old chunks and are sent the new ones. A regenerated chunk whose neighbours were regenerated too is the
same as in a new world of that seed (checked in game), except where trees of neighbouring chunks collide: those depend
on the order chunks are decorated in (see above; exact restoration is issue #257). It answers "Deleting chunks", "Regenerating
chunks" and "Regen complete!" to the player and the operators (`sendSuccess(.., true)`). Differences:
- The area's entities and POIs are kept, as in Java, where only the `region/` records are deleted and loaded
  entities stay in the entity manager until their chunks are back. Here the area's loaded entities (mobs, items,
  vehicles, item frames, armor stands...) are saved to `entities/` and leave the level, as when their chunks
  unload, and the chunks generated in their place load them again with their UUIDs and positions (Horizon's
  `Persistent::finish` reads a generated chunk's saved entities, as vanilla's entity storage is separate from the
  chunk's), as do the entities saved in the area's unloaded chunks. A mob can come back inside the new terrain,
  as in Java. POIs stay in `poi/`; a new chunk's save keeps only the POIs its blocks hold (a bed that is gone
  loses its POI), as vanilla revalidates them. Players are not entities there and stay where they are, as in
  Java.
- The radius is at most 32 (vanilla's largest view distance); TerraForged's `integer(1)` has no maximum, and a large
  radius holds its server thread until the deletes are queued.
- Java decorates every regenerated chunk, writing its features into the neighbours outside the area through its
  `WorldGenRegion`. Horizon's `Streaming::regenerate` decorates the outermost ring too, but against copies of the kept
  neighbours generated again (without their own decoration, their neighbours' or the players' changes), and writes
  into a kept chunk only where it still holds the block it was generated with; block entities of those writes are not
  carried over. The copies' terrain-cache entries and carver records are dropped when their edge decoration comes back
  (a HEAD hook on `Streaming::receive_decorated_edge` calls `release_finished`), as a chunk's own decoration drops
  its own. What the kept chunks' decoration once spread into the area (leaves of trees rooted next door), which Java
  loses, is replayed: `Streaming::regenerate` decorates each kept neighbour that had decorated again, on a worker,
  against copies of the columns outside the area, before the regenerated chunks beside it decorate, and keeps only
  its writes into the area.
- Nothing is deleted while a load or generation of the area is still running, or a decoration or save the flush waits
  for did not finish in 30 seconds: it answers "Regen failed" instead, as it does when the region writer has stopped.
  Unsaved changes in the area are dropped with it.

## Status: development only

These layers only add the noise functions, Engine's terrain types, its world levels, its settings, its landforms, its cell model, its terrain populators, its terrain regions, its volcano populator, its standard terrain provider, its heightmap blenders, its position utilities, its simple and advanced continent generators, the fancy continents (island geometry and generator), Engine's river configuration, its river warps, its river geometry, its river carver, its lakes, its wetlands, its river networks, its river maps, its base river generator, the fancy river generator, its river caches, the simple river generator, the continents' river caches, the heightmap's river stage, the heightmap cache (with its tile path), the river-map reuse, the world-erosion cache, the smoothing filter, the steepness filter, the beach detection filter, the hydraulic erosion filter (with its settings, random stream and brushes), the world filter chain, the tile sizes, the tiles' cells with their filter region, the tiles' chunk views, the tile heightmap fill (direct, through the heightmap cache, zoomed, and zoomed through the cache), the tile visitors, the tile generator, its lazy tiles, its batched and striped generation, its builder, the tile cache's entries, the expiring cache, the async cache entries, the tile cache, its cached chunks, tile disposal, the async tile factory, the climate primitives, the biome climate types, the climate settings, the climate module, the climate stage, the continent factory, the generator context, the heightmap, the world generator factory, TerraForged's terrain levels, its terrain data, its terrain generator, its terrain-blend utilities, its terrain blender with its nearest-terrain search (and the newer source's seed-first blender), noise-graph reseeding, the cave noise primitives, the biome viability rules, the biome overrides, the continent control points, the continent cell primitives, the continent configuration, the continent shape blending, the continent cell generator with its spawn-offset search, the continent river nodes, the continent river carver's arithmetic, the continent river networks with their nearest-node sampling and their warped, carved river sampling, the newer source's seed-first noise sources, scalar noise graphs, domain warps, selectors, graph caches, terraces, legacy terraces, advanced terraces, curves, transforms (steps, frequency, gradient and modulation), special sources (boost, cell, cell edge and sine) and lines, and its bounded concurrent lossy cache with the continent's cell and
river-piece caches on it, the continent noise module that joins the continent layers, the newer source's terrain height sampler with its bordered chunk generation and its nearest-terrain search, its biome chunk fill, cave biome sampler, initial biome selection, surface biome sampler, chunk biome hook and biome registry pools, the erosion tile geometry, and TerraForged Reimagined's heightmap smoothing, erosion brushes, hydraulic erosion filter, eroded-region cache, eroded region build, eroded chunk generation, terrain cache, generator queries, chunk fill, the terrain stage hand-off to it, the current climate noise, the biome sampler bridge, the live biome bindings, the terrain asset decoder, the terrain-type and terrain-noise registries, the bundled terrain types and all 14 bundled terrain noises, the bundled noise caves and their reseeded cave array, and 6dd607eb's structure terrain columns with their
chunk adapter, piece selection and structure-aware terrain stage, 6dd607eb's surface erosion and water edge smoothing passes, and its preliminary surface-height cache, and the `terraforged:normal` world preset, whose pipeline generates
TerraForged's terrain, biomes, surfaces, noise caves and structures (placed on Reimagined's eroded heights), and the Create World screen's TerraForged world type with TerraForged's taller overworld, and its `/locateterrain`, `/tf export structures`, `/tf regen` and `/tf preview` commands. A game built with the bundle offers it, with TerraForged's vegetation, but it is still for development (distant tiles show the un-eroded ground unless `eroded_far_terrain` is on); install instructions come once it is.

## Development

`noise.rs` and `engine.rs` are ordinary Rust, each self-contained, and can be tested without Minecraft resources. CI
runs the same commands for both files, in debug and with `-O` (`.github/workflows/terraforged.yml`):

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
rustc --edition 2024 --test native-mods/terraforged/engine.rs -o /tmp/terraforged-engine-tests
/tmp/terraforged-engine-tests
```

The mixin pins Rusticraft commit `a446cdd041b4d411919d7181a3313c3bf6490a79` (#288's merge; `horizon_revision` in `horizon-mixin.toml`), a `main` commit rather than a Nightly:
no Nightly is close enough (the nearest earlier one, `nightly-source/2026-10-31-d91cab6`, is 1941 commits older and
`generator.rs` differs by 73 lines). The pin moves to a Nightly when the series is rebased onto one.

Translation inputs are TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes. The small Java programs that printed them run against that jar, so they stay
out of Git, in the maintainer's `.local/terraforged/oracles/` folder.
