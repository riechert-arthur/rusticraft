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

public final class TerraForgedHeightmapSmoothingOracle {
    public static void main(String[] args) {
        float[][] settings = {
                {0, 1.75F, 0.85F},
                {2, 1.75F, 0.85F},
                {1, 2.5F, 0.35F},
                {3, 0F, 1F},
        };
        for (float[] values : settings) {
            float[] map = fixtureMap(9);
            apply(map, 9, (int) values[0], values[1], values[2]);
            long hash = 0xcbf29ce484222325L;
            for (float value : map) {
                hash = (hash ^ Integer.toUnsignedLong(Float.floatToRawIntBits(value))) * 1099511628211L;
            }
            System.out.printf("0x%016x%n", hash);
        }
    }

    private static float[] fixtureMap(int length) {
        float[] map = new float[length * length];
        for (int index = 0; index < map.length; index++) {
            map[index] = (index * 37 % 101) / 100F + (index / length) * 0.125F;
        }
        return map;
    }

    private static void apply(float[] map, int length, int iterations, float radiusValue, float strength) {
        if (iterations <= 0) return;
        int radius = round(radiusValue + 0.5F);
        float radiusSquared = radiusValue * radiusValue;
        if (radius <= 0 || radiusSquared <= 0 || strength <= 0) return;
        while (iterations-- > 0) {
            for (int z = radius; z < length - radius; z++) {
                for (int x = radius; x < length - radius; x++) {
                    float total = 0;
                    float weights = 0;
                    for (int dz = -radius; dz <= radius; dz++) {
                        for (int dx = -radius; dx <= radius; dx++) {
                            float distanceSquared = dx * dx + dz * dz;
                            if (distanceSquared > radiusSquared) continue;
                            float weight = 1F - distanceSquared / radiusSquared;
                            total += map[(z + dz) * length + x + dx] * weight;
                            weights += weight;
                        }
                    }
                    if (weights > 0) {
                        int index = z * length + x;
                        map[index] -= (map[index] - total / weights) * strength;
                    }
                }
            }
        }
    }

    private static int round(float value) {
        return value >= 0 ? (int) (value + 0.5F) : (int) (value - 0.5F);
    }
}
