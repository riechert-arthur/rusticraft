import com.terraforged.cereal.Cereal;
import com.terraforged.cereal.spec.DataSpecs;
import com.terraforged.engine.module.Ridge;
import com.terraforged.noise.Module;
import com.terraforged.noise.util.NoiseSpec;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import com.terraforged.cereal.spec.Context;
import com.terraforged.cereal.value.DataList;
import com.terraforged.cereal.value.DataObject;
import com.terraforged.cereal.value.DataValue;
import java.nio.file.Files;
import java.nio.file.Path;

// Decodes each TerraForged terrain-noise resource's "noise" graph the way
// TerrainNoise.CODEC does (DataUtil.toData, Cereal with Engine 0.3.0's specs,
// then TerrainNoise's minimum lift) and
// prints raw f32 bits at fixed coordinates: "<name> <x> <y> <bits>".
// Args: pairs of <name> <terrain-noise json path>.
public class TerraForgedTerrainNoiseOracle {
    static final float[][] POINTS = {
        {0f, 0f}, {128.5f, -63.25f}, {-4096f, 8192f}, {1234.75f, 987.5f},
        {-30000.5f, 15000.25f}, {7.125f, -7.875f}, {65536f, -65536f}, {301.0f, 4022.5f},
    };

    static final double MIN_NOISE = 5.0 / 255.0;

    public static void main(String[] args) throws Exception {
        NoiseSpec.init();
        DataSpecs.register(Ridge.spec());
        for (int i = 0; i < args.length; i += 2) {
            String name = args[i];
            JsonElement json = JsonParser.parseString(Files.readString(Path.of(args[i + 1])));
            DataObject noise = toData(json.getAsJsonObject().get("noise")).asObj();
            Module module = Cereal.deserialize(noise, Module.class, Context.NONE);
            // TerrainNoise's constructor lifts graphs that can reach zero.
            if (module.minValue() < MIN_NOISE) module = module.bias(MIN_NOISE).clamp(0, 1);
            for (float[] p : POINTS) {
                System.out.println(name + " " + p[0] + " " + p[1] + " "
                        + Integer.toUnsignedString(Float.floatToRawIntBits(module.getValue(p[0], p[1]))));
            }
        }
    }

    // Mirrors com.terraforged.mod.data.util.DataUtil.toData with the "type" key.
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
}
