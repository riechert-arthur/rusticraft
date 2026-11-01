"""Execute the pinned Generator.getBaseHeight method against a raw-sample cache fixture."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
class LevelHeightAccessor {}
class RandomState {}
class Seeds { static int get(RandomState state) { return 17; } }
class NoiseSample { float baseNoise, heightNoise; }
class TerrainCache {
    NoiseSample sample = new NoiseSample();
    int calls;
    NoiseSample getSample(int seed, int x, int z) {
        if (seed != 17 || x != 3 || z != 5) throw new AssertionError("query arguments");
        calls++;
        return sample;
    }
}
class TerrainLevels {
    float getScaledBaseLevel(float noise) { return (62F/480F + 128F/480F * noise) * 480F; }
    float getScaledHeight(float noise) { return noise * 480F; }
    int getHeight(float value) { return com.terraforged.noise.util.NoiseUtil.floor(value); }
}
public class HeightQueryOracle {
    public static void main(String[] args) {
        Generator generator = new Generator();
        float[][] samples = {{0.25F,0.4F}, {0.25F,0.1F}, {-0.5F,0.2F}};
        int[][] expected = {{193,193}, {95,49}, {97,97}};
        for (int index = 0; index < samples.length; index++) {
            generator.terrainCache.sample.baseNoise = samples[index][0];
            generator.terrainCache.sample.heightNoise = samples[index][1];
            int ordinal = 0;
            for (var type : net.minecraft.world.level.levelgen.Heightmap.Types.values()) {
                int result = generator.getBaseHeight(3,5,type,new LevelHeightAccessor(),new RandomState());
                if (result != expected[index][ordinal++ < 4 ? 0 : 1]) throw new AssertionError(type);
            }
        }
        if (generator.terrainCache.calls != 18) throw new AssertionError("raw cache call count");
        System.out.println("PASS: 18 raw-cache height queries across all six heightmaps");
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--classpath", required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/Generator.java").read_text()
    start = source.index("    public int getBaseHeight(")
    opening = source.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        if source[end] == "{":
            depth += 1
        elif source[end] == "}":
            depth -= 1
        end += 1
    with tempfile.TemporaryDirectory(prefix="tf-height-query-") as temporary:
        directory = Path(temporary)
        reference = directory / "Generator.java"
        reference.write_text("class Generator { TerrainCache terrainCache = new TerrainCache(); "
                             "TerrainLevels levels = new TerrainLevels();\n" + source[start:end] + "\n}")
        fixture = directory / "HeightQueryOracle.java"
        fixture.write_text(FIXTURE)
        heightmap = directory / "net/minecraft/world/level/levelgen/Heightmap.java"
        heightmap.parent.mkdir(parents=True)
        heightmap.write_text("package net.minecraft.world.level.levelgen; public class Heightmap { "
                             "public enum Types { WORLD_SURFACE, WORLD_SURFACE_WG, MOTION_BLOCKING, "
                             "MOTION_BLOCKING_NO_LEAVES, OCEAN_FLOOR, OCEAN_FLOOR_WG } }")
        subprocess.run(["javac", "-cp", args.classpath, "-d", str(directory),
                        str(reference), str(fixture), str(heightmap)], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", f"{directory}:{args.classpath}",
                        "HeightQueryOracle"], check=True)


if __name__ == "__main__":
    main()
