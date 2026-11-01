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

import com.terraforged.mod.worldgen.biome.CaveBiomeSampler;
import com.terraforged.mod.worldgen.biome.util.BiomeMapManager;
import com.terraforged.mod.worldgen.cave.CaveType;

public final class TerraForgedCaveBiomeSamplerOracle extends CaveBiomeSampler {
    private TerraForgedCaveBiomeSamplerOracle(boolean global, int count) {
        super(800, new BiomeMapManager(global, count));
    }

    public static void main(String[] args) {
        var sampler = new TerraForgedCaveBiomeSamplerOracle(true, 3);
        for (int[] point : new int[][]{{0,0,0}, {0,1024,-768}, {123456789,-2048,4096}, {-987654321,777,-333}}) {
            int seed = point[0], x = point[1], z = point[2];
            System.out.printf("%d %s %s%n",
                Float.floatToRawIntBits(sample(seed + OFFSET, x, z, sampler.frequency)),
                sampler.getUnderGroundBiome(seed, x, z, CaveType.UNIQUE),
                sampler.getUnderGroundBiome(seed, x, z, CaveType.GLOBAL));
        }
        for (boolean global : new boolean[]{false, true}) {
            var empty = new TerraForgedCaveBiomeSamplerOracle(global, 0);
            System.out.printf("empty %s %s%n", empty.getUnderGroundBiome(0,0,0,CaveType.UNIQUE),
                empty.getUnderGroundBiome(0,0,0,CaveType.GLOBAL));
        }
    }
}
