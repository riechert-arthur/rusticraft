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

public final class TerraForgedGeneratorQueryOracle {
    public static void main(String[] args) {
        float[][] cases = {{0.25F, 0.4F}, {0.25F, 0.1F}, {-0.5F, 0.2F}};
        for (float[] value : cases) {
            int base = floor(scaledBase(value[0]));
            int height = floor(value[1] * 480F);
            System.out.printf("H %d %d%n", Math.max(base, height) + 1, height + 1);
        }
        column(0.25F, 0.1F);
        column(0.25F, 0.4F);
        System.out.printf("S %d%n", (int) 0x1234567889abcdefL);
    }

    private static void column(float baseNoise, float heightNoise) {
        int base = floor(scaledBase(baseNoise));
        int height = floor(heightNoise * 480F);
        int surface = Math.max(base, height);
        System.out.printf("C %d %d %d %d%n", height, surface, height, surface - height);
    }

    private static float scaledBase(float noise) {
        return (62F / 480F + 128F / 480F * noise) * 480F;
    }

    private static int floor(float value) {
        return (int) Math.floor(value);
    }
}
