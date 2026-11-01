"""Compile the original cave sampler with minimal Minecraft registry stand-ins."""

import argparse
from pathlib import Path
import subprocess
import tempfile


STUBS = {
    "net/minecraft/core/Holder.java": """
package net.minecraft.core;
public record Holder<T>(int id) {
    public String toString() { return Integer.toString(id); }
}
""",
    "net/minecraft/world/level/biome/Biome.java": """
package net.minecraft.world.level.biome;
public class Biome {}
""",
    "com/terraforged/mod/data/ModBiomes.java": """
package com.terraforged.mod.data;
public class ModBiomes {
    public static final java.util.function.Supplier<Object> CAVE = () -> "cave";
}
""",
    "com/terraforged/mod/worldgen/cave/CaveType.java": """
package com.terraforged.mod.worldgen.cave;
public enum CaveType { GLOBAL, UNIQUE }
""",
    "com/terraforged/mod/worldgen/biome/util/BiomeMapManager.java": """
package com.terraforged.mod.worldgen.biome.util;
import java.util.Optional;
import java.util.stream.Stream;
import java.util.stream.IntStream;
import net.minecraft.core.Holder;
import net.minecraft.world.level.biome.Biome;
public class BiomeMapManager {
    private final Registry registry;
    public BiomeMapManager(boolean global, int count) { registry = new Registry(global, count); }
    public Registry getBiomes() { return registry; }
    public record Registry(boolean global, int count) {
        public Optional<Holder<Biome>> getHolder(Object key) {
            return global ? Optional.of(new Holder<>(9)) : Optional.empty();
        }
        public Stream<Holder<Biome>> holders() {
            return IntStream.range(0, count).mapToObj(Holder<Biome>::new);
        }
    }
}
""",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--classpath", required=True)
    args = parser.parse_args()
    root = args.source / "src/main/java/com/terraforged/mod"
    originals = [
        root / "worldgen/biome/CaveBiomeSampler.java",
        root / "util/storage/WeightMap.java",
        root / "util/MathUtil.java",
    ]
    with tempfile.TemporaryDirectory(prefix="tf-cave-oracle-") as temporary:
        directory = Path(temporary)
        sources = []
        for relative, contents in STUBS.items():
            path = directory / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents)
            sources.append(path)
        driver = Path(__file__).with_name("TerraForgedCaveBiomeSamplerOracle.java")
        subprocess.run(["javac", "-cp", args.classpath, "-d", str(directory),
                        *map(str, originals), *map(str, sources), str(driver)], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp",
                        f"{directory}:{args.classpath}", "TerraForgedCaveBiomeSamplerOracle"],
                       check=True)


if __name__ == "__main__":
    main()
