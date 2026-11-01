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

import com.terraforged.engine.settings.FilterSettings;
import com.terraforged.engine.util.FastRandom;
import com.terraforged.mod.worldgen.noise.erosion.ErosionFilter;
import com.terraforged.mod.worldgen.noise.erosion.NoiseTileSize;

public final class TerraForgedErosionFilterOracle {
    public static void main(String[] args) {
        var settings = new FilterSettings.Erosion();
        settings.dropletsPerChunk = 3;
        settings.dropletLifetime = 10;
        settings.dropletVolume = 0.7F;
        settings.dropletVelocity = 0.7F;
        settings.erosionRate = 0.5F;
        settings.depositeRate = 0.5F;
        var size = NoiseTileSize.DEFAULT;
        var filter = new ErosionFilter(size.regionLength, settings);

        float[] tile = map(size.regionLength);
        filter.apply(42, -3, 5, size, new ErosionFilter.Resource(), new FastRandom(), tile);
        System.out.printf("0x%016x%n", hash(tile));

        float[] region = map(size.regionLength);
        filter.apply(-17, 8, -11, size.chunkLength, size.regionLength,
                new ErosionFilter.Resource(), new FastRandom(), region);
        System.out.printf("0x%016x%n", hash(region));
    }

    private static float[] map(int length) {
        float[] map = new float[length * length];
        for (int index = 0; index < map.length; index++) {
            int x = index % length;
            int z = index / length;
            map[index] = 0.2F + (x * 13 + z * 7) % 31 / 100F + x / 1000F;
        }
        return map;
    }

    private static long hash(float[] map) {
        long hash = 0xcbf29ce484222325L;
        for (float value : map) {
            hash = (hash ^ Integer.toUnsignedLong(Float.floatToRawIntBits(value))) * 1099511628211L;
        }
        return hash;
    }
}
