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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450): the registry
// lookups of `BiomeSampler.getInitialBiome` (`getBiomes().getOrThrow(Biomes.PLAINS)`) and `getBiomeOverride`
// (`BiomeMapManager.get`), `Source`'s `new CaveBiomeSampler(800, manager)` with its registry pools, and
// `getBaseTemperature` as the samplers read it. MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026
// Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    super::{
        biome::BiomeId,
        biome_chunk::{BiomeChunk, BiomeFillError},
    },
    GeneratorData,
    terraforged_biome_registry::{
        ClimateWeight, RegistryBiome, RegistryBiomeMap, identifier, identifier_order,
        registry_entries,
    },
    terraforged_biome_sampler::{BiomeOverride, WetlandSettings},
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_current_surface_biomes::CurrentSurfaceBiomeSampler,
    terraforged_generator_facade::TerraForgedGenerator,
};
use horizon_core::position::ChunkPos;
use std::{fmt, sync::Arc};

/// `Biomes.PLAINS`: `getInitialBiome`'s fallback when the climate's pool is empty.
const PLAINS: &str = "minecraft:plains";
/// `ModBiomes.CAVE`: the cave sampler's GLOBAL pool, when the registry has it.
const CAVE: &str = "terraforged:cave";
/// `Source`'s `new CaveBiomeSampler(800, manager)`.
const CAVE_SCALE: i32 = 800;
/// Every biome `getBiomeOverride` can return, in `BiomeOverride` order; [`BiomeOverride::key`] names each.
const OVERRIDES: [BiomeOverride; 13] = [
    BiomeOverride::DeepColdOcean,
    BiomeOverride::DeepFrozenOcean,
    BiomeOverride::DeepLukewarmOcean,
    BiomeOverride::DeepOcean,
    BiomeOverride::ColdOcean,
    BiomeOverride::FrozenOcean,
    BiomeOverride::WarmOcean,
    BiomeOverride::Ocean,
    BiomeOverride::SnowyBeach,
    BiomeOverride::StonyShore,
    BiomeOverride::Beach,
    BiomeOverride::FrozenRiver,
    BiomeOverride::River,
];

/// A biome TerraForged looks up by key (`getOrThrow`) that the registry lacks: Java throws when the lookup runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingBiome(pub Arc<str>);

impl fmt::Display for MissingBiome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "missing TerraForged biome {}", self.0)
    }
}

impl std::error::Error for MissingBiome {}

/// TerraForged's biome pools, its plains fallback, its 13 override biomes and its cave pools, bound to the numeric
/// biome ids of one loaded registry ([`GeneratorData`]). Those ids are the ones Horizon's own generator writes into
/// chunks; the client maps them by name to the world's biome table for saves and the network (`vanillagen`'s
/// `biome_map`), so a mod biome in the data (a data pack's, or a native mod's) is bound like a vanilla one. The
/// registry does not change while a world is loaded (data-pack reloads leave worldgen registries alone, as in
/// vanilla): build the bindings once per [`GeneratorData`], from the same instance the generator uses.
///
/// Every list, pool and lookup hands out the registry entry's own `Arc`: one per biome.
pub struct LiveBiomeBindings {
    pub map: RegistryBiomeMap<BiomeId>,
    fallback: Arc<BiomeId>,
    overrides: Vec<Arc<BiomeId>>,
    temperatures: Vec<f32>,
    cave: CaveBiomeSampler<BiomeId>,
}

// Chunk workers share one set of bindings (and the surface sampler built over it).
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<LiveBiomeBindings>();
    send_sync::<CurrentSurfaceBiomeSampler<'static, BiomeId>>();
};

impl LiveBiomeBindings {
    /// [`Self::bind`] over Horizon's loaded biome registry and its resolved tags.
    pub fn from_generator_data(
        data: &GeneratorData,
        configured: &[ClimateWeight],
    ) -> Result<Self, MissingBiome> {
        Self::bind(&registry_entries(data), configured)
    }

    /// `new BiomeMapManager(access)` over `entries` (see [`RegistryBiomeMap::build`]), the lookups by key and
    /// `new CaveBiomeSampler(800, manager)`. Entry `i` must hold `BiomeId(i)` (as [`GeneratorData`]'s do; panics
    /// otherwise). Keys match by namespace and path exactly (`Identifier.equals`).
    ///
    /// `Err` with the first of plains and the 13 override biomes (in [`BiomeOverride`] order) the registry lacks. Java
    /// resolves each with `getOrThrow` when a column first needs it and throws there (failing that chunk); the port
    /// checks them all here, once, so a registry without them is refused before any chunk instead of partway
    /// through generation. No biome ever stands in for a missing one.
    ///
    /// The cave sampler's GLOBAL pool is `terraforged:cave` if registered; UNIQUE is every `isNonSurface` biome in
    /// 26.3's registry order: `RegistryDataLoader` registers data-pack entries sorted by `Identifier` (path, then
    /// namespace), not in [`GeneratorData`]'s name order, so the entries are re-sorted that way.
    pub fn bind(
        entries: &[RegistryBiome<BiomeId>],
        configured: &[ClimateWeight],
    ) -> Result<Self, MissingBiome> {
        for (index, entry) in entries.iter().enumerate() {
            assert_eq!(
                *entry.value,
                BiomeId(index as u32),
                "live biome bindings: entry {index} ({}) holds another id",
                entry.name
            );
        }
        let find = |name: &str| {
            entries
                .iter()
                .find(|entry| identifier(&entry.name) == identifier(name))
        };
        let required = |name: &str| {
            find(name)
                .map(|entry| Arc::clone(&entry.value))
                .ok_or_else(|| MissingBiome(Arc::from(name)))
        };
        let fallback = required(PLAINS)?;
        let overrides = OVERRIDES
            .iter()
            .map(|replacement| required(replacement.key().unwrap()))
            .collect::<Result<_, _>>()?;
        let global = find(CAVE)
            .map(|entry| Arc::clone(&entry.value))
            .into_iter()
            .collect();
        let mut special: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is_non_surface())
            .collect();
        special.sort_by(|left, right| identifier_order(&left.name, &right.name));
        let special = special
            .into_iter()
            .map(|entry| Arc::clone(&entry.value))
            .collect();
        Ok(Self {
            map: RegistryBiomeMap::build(entries, configured),
            fallback,
            overrides,
            temperatures: entries.iter().map(|entry| entry.temperature).collect(),
            cave: CaveBiomeSampler::new(CAVE_SCALE, global, special),
        })
    }

    /// `minecraft:plains`, `getInitialBiome`'s fallback.
    pub fn fallback(&self) -> Arc<BiomeId> {
        Arc::clone(&self.fallback)
    }

    /// `new CaveBiomeSampler(800, manager)` over this registry.
    pub const fn cave(&self) -> &CaveBiomeSampler<BiomeId> {
        &self.cave
    }

    /// `biome.value().getBaseTemperature()`. Panics for an id outside this registry.
    pub fn base_temperature(&self, biome: &BiomeId) -> f32 {
        self.temperatures[biome.0 as usize]
    }

    /// `BiomeMapManager.get(key)` for an override. Panics on [`BiomeOverride::Input`], which keeps the sampled
    /// biome and is never looked up (the samplers return it themselves).
    pub fn override_biome(&self, replacement: BiomeOverride) -> Arc<BiomeId> {
        let index = OVERRIDES
            .iter()
            .position(|known| *known == replacement)
            .expect("BiomeOverride::Input is not a registry biome");
        Arc::clone(&self.overrides[index])
    }

    /// [`TerraForgedGenerator::surface_biome_sampler`] over these pools and this plains: build ONE per world.
    /// `wetlands` and `mountain_usage` are Reimagined's `TerraSettings.rivers.wetlands` and
    /// `miscellaneous.mountainBiomeUsage`; its defaults are `WetlandSettings::default()` and
    /// `MiscellaneousSettings::default().mountain_biome_usage` (`terraforged_current_settings`).
    pub fn surface_sampler<'a>(
        &'a self,
        generator: &'a TerraForgedGenerator,
        wetlands: WetlandSettings,
        mountain_usage: f32,
    ) -> CurrentSurfaceBiomeSampler<'a, BiomeId> {
        generator.surface_biome_sampler(
            &self.map.wet,
            &self.map.dry,
            &self.map.highland,
            &self.fallback,
            wetlands,
            mountain_usage,
        )
    }

    /// [`TerraForgedGenerator::create_biomes`] with this registry's cave sampler, base temperatures and override
    /// biomes. `surface` must come from [`Self::surface_sampler`] on the same bindings and generator.
    pub fn create_biomes(
        &self,
        generator: &TerraForgedGenerator,
        chunk: &mut BiomeChunk<BiomeId>,
        position: ChunkPos,
        surface: &CurrentSurfaceBiomeSampler<'_, BiomeId>,
    ) -> Result<(), BiomeFillError<&'static str>> {
        generator.create_biomes(
            chunk,
            position,
            surface,
            &self.cave,
            |biome| self.base_temperature(biome),
            |replacement| self.override_biome(replacement),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_cave_noise::CaveType,
        terraforged_current_surface_biomes::tests::WETLANDS,
        terraforged_eroded_noise_generator::ErodedNoiseGenerator,
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::tests::generator,
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;

    // ---- LiveBiomesOracle (`.local/terraforged/oracles/live_biomes/`): Reimagined's real lookups and cave pools ----

    /// The oracle's `H` lines: (section, lines, FNV-1a 64 of the section's lines).
    const JAVA: [(&str, usize, u64); 4] = [
        ("vanilla", 4, 0xc4a4_bfd2_e20b_5d0e),
        ("modded", 4, 0x93e8_eae9_5ed9_2271),
        ("missing", 28, 0x7737_106f_92a1_f5db),
        ("random", 5200, 0x5f86_a071_5f62_fea4),
    ];
    const RANDOM_CASES: u64 = 2000;
    const OW: &str = "minecraft:is_overworld";
    const NON_SURFACE: &str = "terraforged:non_surface";

    #[derive(Default)]
    struct Section(usize, u64);
    impl Section {
        fn emit(&mut self, line: String) {
            if self.1 == 0 {
                self.1 = 0xcbf2_9ce4_8422_2325;
            }
            for byte in line.bytes().chain([b'\n']) {
                self.1 = (self.1 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.0 += 1;
        }
        /// The oracle's case lines for a registry in [`GeneratorData`] order (`names[id]`).
        fn case(&mut self, case: &str, entries: &[RegistryBiome<BiomeId>], names: &[String]) {
            self.emit(format!("C {case}"));
            let bindings = match LiveBiomeBindings::bind(entries, &[]) {
                Ok(bindings) => bindings,
                Err(MissingBiome(name)) => return self.emit(format!("X {name}")),
            };
            let mut line = String::from("B");
            let all = std::iter::once(bindings.fallback())
                .chain(OVERRIDES.map(|o| bindings.override_biome(o)));
            for biome in all {
                let bits = bindings.base_temperature(&biome).to_bits();
                line += &format!(" {}={bits:x}", names[biome.0 as usize]);
            }
            self.emit(line);
            for (tag, kind) in [("G", CaveType::Global), ("U", CaveType::Unique)] {
                let mut line = String::from(tag);
                for biome in bindings.cave().pool(kind).values() {
                    line = line + " " + &names[biome.0 as usize];
                }
                self.emit(line);
            }
        }
        fn check(&self, section: &str) {
            let java = JAVA.iter().find(|(name, ..)| *name == section).unwrap();
            assert_eq!((self.0, self.1), (java.1, java.2), "{section}");
        }
    }

    fn spec(name: &str, temperature: f32, tags: &[&str]) -> RegistryBiome<BiomeId> {
        let tags = tags.iter().map(|tag| Arc::<str>::from(*tag));
        RegistryBiome::new(BiomeId(0), name, temperature, 0.5, true, tags)
    }

    /// Puts `entries` in [`GeneratorData`]'s order (by name) and numbers them; returns the names by id.
    fn registry(
        mut entries: Vec<RegistryBiome<BiomeId>>,
    ) -> (Vec<RegistryBiome<BiomeId>>, Vec<String>) {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for (index, entry) in entries.iter_mut().enumerate() {
            entry.value = Arc::new(BiomeId(index as u32));
        }
        let names = entries.iter().map(|e| e.name.to_string()).collect();
        (entries, names)
    }

    /// The oracle's `MODS`.
    fn mods() -> Vec<RegistryBiome<BiomeId>> {
        vec![
            spec("terraforged:cave", 0.8, &[OW]),
            spec("terralith:cave/frostfire_caves", 0.0, &[OW]),
            spec("zz:a_caves", 0.5, &[]),
            spec("a:cave", 0.5, &[OW]),
            spec("a:deep_dark", 0.5, &[OW]),
            spec("biomesoplenty:glowing_grotto", 0.6, &[NON_SURFACE]),
            spec("a:plains", 0.8, &[OW]),
            spec("terralith:ocean", 0.5, &[OW, "minecraft:is_ocean"]),
        ]
    }

    fn data() -> Option<Arc<GeneratorData>> {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client/26.3")
            });
        if !root.join("worldgen/world_presets.json").exists() {
            eprintln!("skipping: no worldgen bundle at {}", root.display());
            return None;
        }
        let registry = Box::leak(Box::new(crate::level::data::load_registry(&root).unwrap()));
        Some(GeneratorData::load(&root.join("worldgen"), registry).unwrap())
    }

    #[test]
    fn vanilla_modded_and_missing_registries_match_reimagined_java() {
        let Some(data) = data() else { return };
        let vanilla = registry_entries(&data);
        let names: Vec<String> = data.biome_names.iter().map(|n| n.to_string()).collect();
        let mut section = Section::default();
        section.case("vanilla", &vanilla, &names);
        // Through `from_generator_data` too: the same bindings.
        let live = LiveBiomeBindings::from_generator_data(&data, &[]).unwrap();
        assert_eq!(
            *live.fallback(),
            BiomeId(names.iter().position(|n| n == PLAINS).unwrap() as u32)
        );
        section.check("vanilla");

        let mut modded = vanilla.clone();
        modded.extend(mods());
        let mut section = Section::default();
        let (entries, names) = registry(modded.clone());
        section.case("modded", &entries, &names);
        section.check("modded");

        let mut section = Section::default();
        for key in std::iter::once(PLAINS).chain(OVERRIDES.map(|o| o.key().unwrap())) {
            let without = modded.iter().filter(|e| *e.name != *key).cloned().collect();
            let (entries, names) = registry(without);
            section.case(identifier(key).1, &entries, &names);
        }
        section.check("missing");
    }

    /// The oracle's `Gen` LCG.
    struct Gen(u64);
    impl Gen {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % n as u64) as usize
        }
    }

    #[test]
    fn random_registries_match_reimagined_java() {
        let ns = ["minecraft", "terraforged", "terralith", "a", "zz"];
        let paths = "plains ocean river cave cave/deep lush_caves deep_dark dripstone_caves sulfur_caves x_caves \
            caves cave/ plains_caves forest a_b ab a.b";
        let paths: Vec<_> = paths.split_whitespace().collect();
        let floats = [
            -0.5,
            0.0,
            -0.0,
            0.15,
            0.5,
            0.8,
            2.0,
            f32::NAN,
            f32::from_bits(1),
        ];
        let required = std::iter::once(PLAINS).chain(OVERRIDES.map(|o| o.key().unwrap()));
        let required: Vec<_> = required.collect();
        let mut section = Section::default();
        for case in 0..RANDOM_CASES {
            let mut g = Gen(case.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1));
            let mut specs: Vec<RegistryBiome<BiomeId>> = Vec::new();
            for name in &required {
                let t = floats[g.next(floats.len())];
                if g.next(12) != 0 {
                    specs.push(spec(name, t, &[OW]));
                }
            }
            for _ in 0..g.next(20) {
                let name = format!("{}:{}", ns[g.next(ns.len())], paths[g.next(paths.len())]);
                let t = floats[g.next(floats.len())];
                let mut tags = Vec::new();
                if g.next(2) == 0 {
                    tags.push(OW);
                }
                if g.next(6) == 0 {
                    tags.push(NON_SURFACE);
                }
                if specs.iter().all(|s| *s.name != *name) {
                    specs.push(spec(&name, t, &tags));
                }
            }
            let (entries, names) = registry(specs);
            section.case(&case.to_string(), &entries, &names);
        }
        section.check("random");
    }

    /// Every pool, list and lookup holds the registry entry's own `Arc`, and the chunk hook fills from them: the
    /// surface layer is the surface sampler's biome with these overrides and temperatures, the cave layer a UNIQUE
    /// cave biome.
    #[test]
    fn bindings_share_the_registry_arcs_and_fill_chunks() {
        let mut specs = vec![
            spec("minecraft:forest", 0.7, &[OW, "minecraft:is_forest"]),
            spec("minecraft:snowy_plains", 0.0, &[OW]),
            spec("minecraft:desert", 2.0, &[OW, "minecraft:is_badlands"]),
            spec("minecraft:lush_caves", 0.5, &[OW]),
        ];
        for name in std::iter::once(PLAINS).chain(OVERRIDES.map(|o| o.key().unwrap())) {
            specs.push(spec(name, 0.5, &[OW, "minecraft:is_ocean"]));
        }
        let (entries, _) = registry(specs);
        let bindings = LiveBiomeBindings::bind(&entries, &[]).unwrap();
        let own =
            |biome: &Arc<BiomeId>| assert!(Arc::ptr_eq(biome, &entries[biome.0 as usize].value));
        own(&bindings.fallback());
        OVERRIDES
            .iter()
            .for_each(|o| own(&bindings.override_biome(*o)));
        bindings.map.overworld.iter().for_each(own);
        for pools in [&bindings.map.wet, &bindings.map.dry, &bindings.map.highland] {
            for climate in super::super::terraforged_engine::BiomeType::VALUES {
                pools
                    .get(climate)
                    .iter()
                    .flat_map(|p| p.values())
                    .for_each(own);
            }
        }
        let unique = bindings.cave().pool(CaveType::Unique).values();
        assert_eq!(unique.len(), 1);
        own(&unique[0]);
        assert!(bindings.cave().pool(CaveType::Global).values().is_empty());
        assert!(
            std::panic::catch_unwind(|| bindings.override_biome(BiomeOverride::Input)).is_err()
        );

        let g = TerraForgedGenerator::new(
            TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22),
            ErodedNoiseGenerator::new(
                generator(0, 0),
                ErosionFilterSettings::default(),
                HeightmapSmoothingSettings::default(),
            ),
        );
        g.capture_seed(7);
        let surface = bindings.surface_sampler(&g, WETLANDS, 1.0);
        let position = ChunkPos::new(-3, 5);
        let mut chunk = BiomeChunk::new(position, -64, 384, BiomeId(u32::MAX)).unwrap();
        bindings
            .create_biomes(&g, &mut chunk, position, &surface)
            .unwrap();
        for quart in 0..16 {
            let (x, z) = (
                (position.x << 2) + (quart & 3),
                (position.z << 2) + (quart >> 2),
            );
            let expected = surface
                .sample_biome(
                    7,
                    x << 2,
                    z << 2,
                    |b| bindings.base_temperature(b),
                    |r| bindings.override_biome(r),
                )
                .unwrap();
            assert_eq!(*chunk.noise_biome([x, 0, z]), *expected);
            let below = *chunk.noise_biome([x, -16, z]);
            assert!(below == *expected || below == *unique[0]);
        }
    }

    #[test]
    #[should_panic(expected = "holds another id")]
    fn misnumbered_entries_panic() {
        let _ = LiveBiomeBindings::bind(
            &[spec(PLAINS, 0.8, &[]), spec("minecraft:ocean", 0.5, &[])],
            &[],
        );
    }
}
