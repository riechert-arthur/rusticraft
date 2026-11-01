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

import com.terraforged.engine.world.biome.type.BiomeType;
import java.util.EnumMap;

public final class TerraForgedBiomePoolOracle {
    public static void main(String[] args) {
        var pools = new EnumMap<BiomeType, String>(BiomeType.class);
        pools.put(BiomeType.SAVANNA, "savanna");
        pools.put(BiomeType.TEMPERATE_FOREST, "temperate_forest");
        pools.put(BiomeType.TUNDRA, "tundra");

        var donors = new EnumMap<BiomeType, String>(pools);
        for (var type : BiomeType.values()) {
            if (pools.containsKey(type)) continue;
            BiomeType nearest = null;
            float nearestDistance = Float.MAX_VALUE;
            for (var candidate : donors.keySet()) {
                float distance = distance(type, candidate);
                if (distance < nearestDistance) {
                    nearestDistance = distance;
                    nearest = candidate;
                }
            }
            if (nearest != null) pools.put(type, donors.get(nearest));
            System.out.printf("P %s %s %d%n", type, nearest,
                    Float.floatToRawIntBits(nearestDistance));
        }
    }

    private static float distance(BiomeType a, BiomeType b) {
        float dt = midpoint(a.getMinTemperature(), a.getMaxTemperature())
                - midpoint(b.getMinTemperature(), b.getMaxTemperature());
        float dm = midpoint(a.getMinMoisture(), a.getMaxMoisture())
                - midpoint(b.getMinMoisture(), b.getMaxMoisture());
        dt *= 2F;
        return dt * dt + dm * dm;
    }

    private static float midpoint(float min, float max) {
        return (min + max) * 0.5F;
    }
}
