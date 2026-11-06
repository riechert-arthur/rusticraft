# Third-party notices

## TerraForged

`native-mods/terraforged` translates
[TerraForged](https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44)
and its Engine 0.3.0/Noise2D dependencies by TerraForged. The unavailable dependency source was
recovered from bytecode shipped in TerraForged's official 0.3.1 alpha 2 jar. The code is licensed
under MIT; its notice is preserved in `native-mods/terraforged/LICENSE-TERRAFORGED`.
The terrain-noise decoder's `Valley` spec in `native-mods/terraforged/terrain_asset.rs` is Engine 0.3.0's erosion
`Ridge.spec()` and `Ridge.create`, as TerraForged's `NoiseCodec` registers it (TerraForged Reimagined, credited below,
no longer registers it).
The cave biome sampler in `native-mods/terraforged/cave_biome_sampler.rs` (choice noise, `WeightMap` pools, no
fallback) follows TerraForged's own `CaveBiomeSampler` and `WeightMap`. The biome chunk fill in
`native-mods/terraforged/biome_chunk_fill.rs` and the chunk biome hook (`create_biomes` in
`native-mods/terraforged/generator_facade.rs`) follow TerraForged's own `ChunkUtil.fillNoiseBiomes`, `BiomeBuffer2D`,
`Generator.createBiomes` and `Source`'s `withSeed`, `getNoiseBiome` and `compute` (one biome layer, no cave layer).
TerraForged's generated mountain terrain-noise data files `mountains_1.json`, `mountains_2.json` and `mountains_3.json`
(`generated/resources/default/data/terraforged/worldgen/terrain/noise/`, which use that `Valley` node; TerraForged
Reimagined's copies differ) are bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_noise/`.
The cave asset decoder and cave array in `native-mods/terraforged/cave_assets.rs` follow TerraForged's own
`NoiseCave.CODEC` and `NoiseCaveGenerator.createArray`. The noise-cave carver in
`native-mods/terraforged/cave_carver.rs` and its pipeline stage follow TerraForged's own `NoiseCaveGenerator.carve`,
`NoiseCaveCarver`, `CarverChunk` and `BiomeGenerator.carve`.
The biome override and initial biome (`biome_override` and `initial_biome` in
`native-mods/terraforged/biome_sampler.rs`) and the surface biome sampler in `current_surface_biomes.rs` follow
TerraForged's own `BiomeSampler.getBiomeOverride` (with its 0.005 beach band), `getInitialBiome` (one climate pool,
plains for a missing or empty one) and `sampleBiome`. The biome registry pools in `native-mods/terraforged/biome_registry.rs`
and `biome_map.rs` follow TerraForged's own `BiomeMapManager` (`getOverworldBiomes`, `getWeightsMap`, `getBiomeWeights`,
`buildBiomeMap`), `BiomeUtil` (`getOverworldBiomes`, `BIOME_SORTER`, `getType`) and `ClimateType`'s weights.
The climate noise in `native-mods/terraforged/current_climate.rs` and the biome sampler bridge in
`current_biome_sampler.rs` follow TerraForged's own `ClimateNoise` (Engine's post-increment `Seed`, no edge warp or
macro grouping) and `IBiomeSampler.Sampler` with `createClimate`, with `NoiseGenerator.createContinentNoise`'s climate
settings.
The generator in `native-mods/terraforged/generator_preset.rs` and `pipeline.rs` follows TerraForged's own
`GeneratorPreset.build`, `TerrainLevels.CODEC`, `NoiseGenerator.createLandTerrain` and `createContinentNoise`, and
`Generator`'s `createBiomes`, `fillFromNoise`, `buildSurface` (`SurfaceDecorator`, `NoiseChunkUtil`), the terrain-cache
drop in `applyBiomeDecoration`, `getSeaLevel` and `Source.getNoiseBiome`, over Engine 0.3.0's terrain-type registry as
TerraForged links it.
The decoration in `native-mods/terraforged/decorator.rs` follows TerraForged's own `BiomeGenerator.decorate`:
`FeatureDecorator` and `VanillaDecorator` (the per-biome feature steps, their seeding order and the structure pieces
around the vegetal step), and `NoiseCaveGenerator.decorate` with `NoiseCaveDecorator` (each noise cave's biomes'
features), then `Surface.smoothWater` and `Surface.applyPost`.
The world type in `native-mods/terraforged/client_world_type.rs` follows TerraForged's own `DatapackHook.injectDatapack`
and `DataPackExporter.createWorldDatapack` for TerraForged worlds: each gets a copy of TerraForged's built-in pack.
The boat hook in `native-mods/terraforged/boat.rs` follows TerraForged's own `MixinBoat` and `BoatHook.floatTheBoat`.
The vegetation decoder in `native-mods/terraforged/vegetation.rs` follows TerraForged's own `VegetationConfig.CODEC`,
`ViabilityCodec` and its viability specs, `BiomeVegetationManager.getConfig` and `VegetationFeatures.create`. The
vegetal step in `native-mods/terraforged/decorator.rs` follows TerraForged's own `PositionSampler` (with
`SamplerContext` and `BiomeList`), `BiomeVegetationManager.getVegetation` and `VegetationFeatures.unwrap`, and the
viability rules' seed-first noise sampling in `biome_viability.rs` follows its `NoiseViability` and
`ViabilityContext`.
TerraForged's generated vegetation data file `trees_temperate.json`
(`generated/resources/default/data/terraforged/worldgen/vegetation/`; TerraForged Reimagined's copy differs) is bundled
unchanged in `native-mods/terraforged/data/worldgen/vegetation/`, and the eight `trees/*` biome tags `copses.json`,
`hardy.json`, `hardy_slopes.json`, `patchy.json`, `rainforest.json`, `sparse.json`, `sparse_rainforest.json` and
`temperate.json`, which neither source tree ships, are bundled unchanged from TerraForged's official 0.3.1 alpha 2 jar
(`default/data/terraforged/tags/worldgen/biome/trees/`, MIT) in `native-mods/terraforged/data/tags/worldgen/biome/trees/`.

The seed-first source API additionally follows
[Noise2D 2cb48431148a321f74319d7ef0ea61b26bf8e59c](https://github.com/TerraForged/Noise2D/tree/2cb48431148a321f74319d7ef0ea61b26bf8e59c),
MIT, copyright (c) 2018 dags and (c) 2020 TerraForged. Its license is preserved in
`native-mods/terraforged/LICENSE-NOISE2D`. This is the seed-first API used by the current
TerraForged source, distinct from the older shaded dependency's seed-replacement API.

The heightmap smoothing, the erosion brushes and filter and the eroded-region cache in
`native-mods/terraforged/erosion.rs` (`HeightmapSmoothing`, `HeightmapSmoothingSettings`, `ErosionBrushes`,
`ErosionFilterSettings`, `ErosionFilter`, `ErosionRegionLayout`, `ErosionRegionCache`), the eroded region build in
`native-mods/terraforged/eroded_noise_generator.rs` (`ErodedNoiseGenerator`), the terrain cache, the generator queries, the chunk fill and the biome classification's precipitation rule in `terrain_cache.rs`, `generator_facade.rs`, `chunk_fill.rs`, `biome_registry.rs`, `live_biomes.rs`, `terrain_asset.rs`, `terrain_asset_registry.rs`, `current_terrain_resources.rs`, `surface_bindings.rs` and `vegetation.rs` are adapted from
[TerraForged Reimagined a24d4503064780790e975cd17495866ae13eef11](https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11)
(`HeightmapSmoothing` and `TerraSettings.Smoothing`; `ErosionFilter`'s erosion-brush layout, its
droplet simulation with the region-wide `apply`, and `TerraSettings.Erosion`; `ErodedNoiseGenerator`'s region
layout, `getRegion` region cache, `buildRegion`, `erodedHeightNoise`, `generate` and its `INoiseGenerator` delegations, `getNoiseSample` and `sample` included; `TerrainCache` with `getErodedHeightNoise`; and `Generator`'s seed capture (`createState`, `Seeds.get`), chunk-data, hint and drop calls, level queries, `getBaseHeight` (its fix for floating structures: the eroded height, `getErodedHeightNoise`, which the port also uses for `getBaseColumn`), `getBaseColumn` and `buildTerrain`'s `combineAsync` hand-off to `fillFromNoise`; `ChunkUtil.fillChunk`, `fillSection` and `primeHeightmaps` as `Generator.fillFromNoise` calls them; and `Surface.isErodible`'s `#minecraft:substrate_overworld` erodible tag, the default of `surface_bindings.rs`; and `VegetationFeatures.TREE_KEYWORDS`' `pale_garden_vegetation` keyword, which `vegetation.rs` adds to TerraForged's tree keywords; and `BiomeUtil.getPrecipitation`, the position-free 26.3 snow rule that stands in for 1.18's `Biome.getPrecipitation()` in the biome classification; and the registry lookups of `getInitialBiome`'s plains fallback and `getBiomeOverride` (`BiomeMapManager.get`); and `TerrainNoise.CODEC` and `NoiseCodec.CODEC` with `SuperCodec`, `DataUtil.toData`, Cereal's spec reading and the noise specs `NoiseSpec` registers; and the asset `TerrainType` (`DIRECT`, `forName`) with its engine `TerrainType.get` and `getOrCreate`, and `RegistryKey.entries`, loading the terrain-type and terrain-noise registries; and its 18 `terrain_type` data files, bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_type/`; and its flatland, hill, mountain, plateau and badlands `terrain_noise` data files `plains.json`, `steppe.json`, `dales.json`, `hills_1.json`, `hills_2.json`, `mountains_ridge_1.json`, `mountains_ridge_2.json`, `torridonian.json`, `dolomites.json`, `plateau.json` and `badlands.json`, bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_noise/`; and its five noise-cave data files `mega.json`, `mega_deep.json`, `synapse_high.json`, `synapse_low.json` and `synapse_mid.json` (TerraForged's generated copies are the same), bundled unchanged in `native-mods/terraforged/data/worldgen/cave/`; and its `terraforged:normal` world preset `normal.json` (TerraForged's generated copy is the same), bundled unchanged in `native-mods/terraforged/data/worldgen/world_preset/`; and its taller `minecraft:overworld` dimension type `overworld.json` (-64..960; TerraForged's generated copy has the same range in 1.18's format), bundled unchanged in `native-mods/terraforged/data/dimension_type/` and written into each new TerraForged world's own data pack; and its seven vegetation data files `trees_copse.json`, `trees_hardy.json`, `trees_hardy_slopes.json`, `trees_patchy.json`, `trees_rainforest.json`, `trees_sparse.json` and `trees_sparse_rainforest.json` (TerraForged's generated copies are the same), bundled unchanged in `native-mods/terraforged/data/worldgen/vegetation/`), an unofficial port of TerraForged to Minecraft 26.3,
MIT, copyright (c) 2020 TerraForged and (c) 2026 Romoslayer. Its license is preserved in
`native-mods/terraforged/LICENSE-TERRAFORGED-REIMAGINED`.

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with upstream fixes applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124),
[gfx-rs/wgpu#10714](https://github.com/gfx-rs/wgpu/pull/10714)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## naga

`third_party/naga` is [naga](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs developers, with
fixes to its GLSL frontend that shader packs need, each applied in the commit that needed it. It is licensed under MIT or Apache-2.0; both licenses
are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Horizon's own, drawn on
the same grid.

## AMD FidelityFX SDK (FSR 3.1 frame generation)

`crates/horizon-client-render/src/gfx/frame_gen/optical_flow.wgsl` and `interpolation.wgsl` are
ports to WGSL of the optical flow and frame interpolation shaders of the
[AMD FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 1.1.4
(`sdk/include/FidelityFX/gpu/{opticalflow,frameinterpolation}`, at commit
[c6efa6bf7f20](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/c6efa6bf7f2027b3ec94f28578bb5965eabb9e55)),
and `optical_flow.rs` follows the SDK's `sdk/src/components/opticalflow/ffx_opticalflow.cpp`
(its passes, their order and their dispatch sizes). Copyright (C) 2024 Advanced Micro Devices,
Inc., under the MIT license, whose text heads each file. AMD and FidelityFX are AMD's
trademarks; Horizon is not affiliated with AMD.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with.

- `launcher/assets/logos/quilt.png` is the Quilt mini icon (`brand/128png/quilt_mini_icon_transparent.png`)
  from [QuiltMC/art](https://github.com/QuiltMC/art), by the Quilt Project, dedicated to the public
  domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
- `launcher/assets/logos/neoforge.png` is NeoForged's icon (`art/neoforged_icon_16x16.png`) from
  [neoforged/.github](https://github.com/neoforged/.github/tree/main/art), © the NeoForged team,
  created by Ridanisaurus, under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). It is
  unmodified (the launcher scales it up by whole pixels).
- Fabric's and Forge's icons are not shipped: neither publishes a license for its logo. The launcher
  fetches them from their own sites when an instance needs them
  ([fabric-loader's icon](https://github.com/FabricMC/fabric-loader/blob/master/src/main/resources/assets/fabricloader/icon.png),
  [files.minecraftforge.net's](https://files.minecraftforge.net/)).

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Horizon.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `launcher/src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Horizon is not endorsed by Microsoft.
