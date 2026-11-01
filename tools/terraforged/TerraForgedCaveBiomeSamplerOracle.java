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

import com.terraforged.noise.util.Noise;
import com.terraforged.noise.util.NoiseUtil;

public final class TerraForgedCaveBiomeSamplerOracle {
    private static final int OFFSET = 124897;
    private static final int MASK_OFFSET = 987541;
    private static final float FREQUENCY = 1F / 800F;
    private static final float MASK_FREQUENCY = 1F / 1600F;
    private static final float MASK_THRESHOLD = 0.62F;

    public static void main(String[] args) {
        int[][] cases = {
                {0, 0, 0},
                {0, 1024, -768},
                {123456789, -2048, 4096},
                {-987654321, 777, -333},
        };
        System.out.printf("F %d%n", Float.floatToRawIntBits(FREQUENCY));
        for (int[] value : cases) {
            int seed = value[0];
            int x = value[1];
            int z = value[2];
            float choice = sample(seed + OFFSET, x, z, FREQUENCY);
            float mask = sample(seed + MASK_OFFSET, x, z, MASK_FREQUENCY);
            int selected = mask < MASK_THRESHOLD ? -1 : select(choice, 3);
            System.out.printf("C %d %d %d%n",
                    Float.floatToRawIntBits(choice), Float.floatToRawIntBits(mask), selected);
        }
    }

    private static float sample(int seed, int x, int z, float frequency) {
        float nx = x * frequency;
        float nz = z * frequency;
        float noise = (1 + Noise.singleSimplex(nx, nz, seed)) * 0.5F;
        return NoiseUtil.clamp(noise, 0F, 1F);
    }

    private static int select(float noise, int count) {
        float value = noise * (count * 0.99999F);
        if (value < 1F) return 0;
        for (int index = 1; index < count; index++) {
            if (value < index + 1F) return index;
        }
        return -1;
    }
}
