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

import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.util.NoiseUtil;

public final class TerraForgedNoiseHeightOracle {
    public static void main(String[] args) {
        float depthMin = 22F / 480F;
        float heightMin = 62F / 480F;
        float depthRange = heightMin - depthMin;
        float baseRange = 128F / 480F;
        float heightRange = 1F - (heightMin + baseRange);
        Module ocean = Source.simplex(8763214, 64, 3).scale(0.4F);
        int[][] cases = {
                {0x3e000000, 0x3d800000},
                {0x3ec00000, 0x3d800000},
                {0x3f066666, 0x3e000000},
                {0x3f200000, 0x3e000000},
        };
        for (int[] entry : cases) {
            float continent = Float.intBitsToFloat(entry[0]);
            float base = Float.intBitsToFloat(entry[1]);
            float height;
            if (continent < 0.25F) {
                height = depthMin + ocean.getValue(42, 128.25F, -64.5F) * depthRange;
            } else if (continent < 0.5F) {
                float lower = depthMin + ocean.getValue(42, 128.25F, -64.5F) * depthRange;
                float alpha = (continent - 0.25F) / (0.5F - 0.25F);
                height = NoiseUtil.lerp(lower, heightMin, alpha);
            } else {
                float upperRaw = 0.25F * 1.2F;
                float upper = heightMin + baseRange * base + heightRange * upperRaw;
                if (continent < 0.55F) {
                    float alpha = (continent - 0.5F) / (0.55F - 0.5F);
                    height = NoiseUtil.lerp(heightMin, upper, alpha);
                } else {
                    height = upper;
                }
            }
            System.out.printf("0x%08x%n", Float.floatToRawIntBits(height));
        }
    }
}
