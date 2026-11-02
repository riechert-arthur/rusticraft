import com.terraforged.mod.worldgen.test.Volcano;
import com.terraforged.mod.worldgen.test.VolcanoConfig;

// Runs TerraForged 6dd607eb's worldgen/test/Volcano over its default config: the points
// collected for chunks (centres below y 180 by VolcanoFeature.test's stand-in, here every
// centre), then each column's highest value. Classpath: the pinned Volcano.java, a
// VolcanoConfig record with the original's fields and Range, and Minecraft's Mth.
// Output: "chunk <cx> <cz> <x> <z> <height> <mouth> <hash>" for every non-zero column.
public class TerraForgedVolcanoOracle {
    public static void main(String[] args) {
        var config = VolcanoConfig.defaultConfig();
        var cache = new Volcano.Cache();
        for (long seed : new long[] {1234L, -987654321987L}) {
            for (int[] c : new int[][] {{0, 0}, {5, -3}, {-12, 7}, {40, 40}}) {
                cache.reset();
                Volcano.collectPoints(seed, c[0], c[1], null, config, cache, (x, z, ctx) -> true);
                for (int dz = 0; dz < 16; dz += 3) {
                    for (int dx = 0; dx < 16; dx += 3) {
                        int x = (c[0] << 4) + dx, z = (c[1] << 4) + dz;
                        var value = Volcano.getHighest(x, z, config, cache);
                        if (value.height == 0) continue;
                        System.out.println(seed + " " + c[0] + " " + c[1] + " " + x + " " + z + " "
                                + value.height + " " + value.mouth + " " + value.hash);
                    }
                }
            }
        }
    }
}
