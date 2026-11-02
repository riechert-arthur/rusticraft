import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.terraforged.cereal.Cereal;
import com.terraforged.cereal.spec.Context;
import com.terraforged.cereal.value.DataList;
import com.terraforged.cereal.value.DataObject;
import com.terraforged.cereal.value.DataValue;
import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.util.NoiseSpec;
import com.terraforged.noise.util.NoiseUtil;
import java.nio.file.Files;
import java.nio.file.Path;

// Samples TerraForged 6dd607eb's cave noise: NoiseCaveGenerator's unique-cave and
// breach modifiers, and NoiseCave's height, cavern size and floor depth for each
// cave resource after NoiseCaveGenerator.createArray's reseeding.
// Classpath: Noise2D (TerraForged's seed-first version) with Cereal, and Gson.
// Args: pairs of <name> <cave json path>. Output lines are raw f32/int values.
public class TerraForgedNoiseCaveOracle {
    static final int[] SEEDS = {0, 1337, -987654321};
    static final int[][] POINTS = {{0, 0}, {37, -91}, {-1024, 4096}, {12345, -6789}, {-30001, 15003}};

    public static void main(String[] args) throws Exception {
        NoiseSpec.init();
        Module unique = new UniqueCaveDistributor(1286745, 1F / 500, 0.75F, 0.05F)
                .clamp(0.2, 1.0).map(0, 1)
                .warp(781624, 30, 1, 20);
        Module breach = Source.simplexRidge(1567328, 300, 2).clamp(0.7F * 0.8F, 0.7F).map(0, 1);
        for (int seed : SEEDS) {
            for (int[] p : POINTS) {
                System.out.println("modifier " + seed + " " + p[0] + " " + p[1] + " "
                        + bits(unique.getValue(seed, p[0], p[1])) + " " + bits(breach.getValue(seed, p[0], p[1])));
            }
        }
        // Unique caves occupy few cells: also report the first hits of a fixed scan.
        for (int seed : SEEDS) {
            int hits = 0;
            for (int n = 0; n < 20000 && hits < 4; n++) {
                int x = n * 97 - 900000, z = n * -61 + 300000;
                float value = unique.getValue(seed, x, z);
                if (value > 0) {
                    hits++;
                    System.out.println("unique " + seed + " " + x + " " + z + " " + bits(value));
                }
            }
        }
        for (int i = 0; i < args.length; i += 2) {
            JsonObject json = JsonParser.parseString(Files.readString(Path.of(args[i + 1]))).getAsJsonObject();
            boolean global = json.get("type").getAsString().equals("global");
            int count = global ? 2 : 1;
            int minY = json.has("min_y") ? json.get("min_y").getAsInt() : -32;
            int maxY = json.get("max_y").getAsInt();
            int size = json.get("size").getAsInt();
            for (int j = 0; j < count; j++) {
                long reseed = j * 0xFA90C2L;
                Module elevation = reseed(module(json.get("elevation")), reseed);
                Module shape = reseed(module(json.get("shape")), reseed);
                Module floor = reseed(module(json.get("floor")), reseed);
                for (int seed : SEEDS) {
                    for (int[] p : POINTS) {
                        int height = scale(seed, p[0], p[1], 1F, minY, maxY - minY, elevation);
                        int cavern = scale(seed, p[0], p[1], 0.6F, 0, size, shape);
                        int depth = scale(seed, p[0], p[1], 1F, 0, cavern, floor);
                        System.out.println(args[i] + " " + j + " " + seed + " " + p[0] + " " + p[1]
                                + " " + height + " " + cavern + " " + depth);
                    }
                }
            }
        }
    }

    static String bits(float value) {
        return Integer.toUnsignedString(Float.floatToRawIntBits(value));
    }

    // NoiseCave.getScaleValue.
    static int scale(int seed, int x, int z, float modifier, int min, int range, Module noise) {
        if (range <= 0) return 0;
        return min + NoiseUtil.floor(noise.getValue(seed, x, z) * range * modifier);
    }

    // ContextSeedable.withSeed: a Cereal round trip with the seed in the context.
    static Module reseed(Module module, long seed) {
        var context = new Context();
        context.getData().add("seed", seed);
        return Cereal.deserialize(Cereal.serialize(module).asObj(), Module.class, context);
    }

    static Module module(JsonElement json) {
        return Cereal.deserialize(toData(json).asObj(), Module.class, Context.NONE);
    }

    // com.terraforged.mod.data.util.DataUtil.toData with the "type" key.
    static DataValue toData(JsonElement json) {
        if (json.isJsonObject()) {
            var object = json.getAsJsonObject();
            var name = object.get("type");
            var data = new DataObject(name == null ? "" : name.getAsString());
            for (var entry : object.entrySet()) {
                if (!entry.getKey().equals("type")) data.add(entry.getKey(), toData(entry.getValue()));
            }
            return data;
        }
        if (json.isJsonArray()) {
            var list = new DataList();
            for (var entry : json.getAsJsonArray()) list.add(toData(entry));
            return list;
        }
        var prim = json.getAsJsonPrimitive();
        if (prim.isString()) return DataValue.of(prim.getAsString());
        if (prim.isNumber()) return DataValue.of(prim.getAsNumber());
        return DataValue.of(prim.getAsBoolean());
    }

    // TerraForged's com.terraforged.mod.worldgen.cave.UniqueCaveDistributor, verbatim
    // except for its preview main method.
    static class UniqueCaveDistributor implements Module {
        private final int seed;
        private final float frequency;
        private final float jitter;
        private final float density;

        UniqueCaveDistributor(int seed, float frequency, float jitter, float density) {
            this.seed = seed;
            this.frequency = frequency;
            this.jitter = jitter;
            this.density = density;
        }

        @Override
        public float getValue(int seed, float x, float y) {
            x *= frequency;
            y *= frequency;
            int maxX = NoiseUtil.floor(x) + 1;
            int maxY = NoiseUtil.floor(y) + 1;
            int cellX = maxX - 1;
            int cellY = maxY - 1;
            float distA = Float.POSITIVE_INFINITY;
            float distB = Float.POSITIVE_INFINITY;
            for (int cy = maxY - 2; cy <= maxY; cy++) {
                float ox = (cy & 1) * 0.5F;
                for (int cx = maxX - 2; cx <= maxX; cx++) {
                    var vec = NoiseUtil.cell(seed, cx, cy);
                    float px = cx + ox + (vec.x * jitter);
                    float py = cy + vec.y * jitter;
                    float d2 = NoiseUtil.dist2(x, y, px, py);
                    if (d2 < distA) {
                        distB = distA;
                        distA = d2;
                        cellX = cx;
                        cellY = cy;
                    } else if (d2 < distB) {
                        distB = d2;
                    }
                }
            }
            if (cellValue(cellX, cellY) > density) {
                return 0F;
            }
            return 1F - NoiseUtil.sqrt(distA / distB);
        }

        private float cellValue(int cellX, int cellY) {
            float noise = NoiseUtil.valCoord2D(seed, cellX, cellY);
            return (1 + noise) * 0.5F;
        }
    }
}
