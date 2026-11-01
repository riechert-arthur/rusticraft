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

public final class TerraForgedBiomeSamplerDecisionOracle {
    public static void main(String[] args) {
        float[] noises = {0F, 0.25F, 0.5F, 1F, Float.intBitsToFloat(0x7fc01234)};
        for (float noise : noises) {
            System.out.printf("R %d %d%n", Float.floatToRawIntBits(noise),
                    Float.floatToRawIntBits(cellRoll(noise, 4153)));
            System.out.printf("M %d %d%n", Float.floatToRawIntBits(noise),
                    Float.floatToRawIntBits(cellRoll(noise, 7309)));
        }

        slope(480, 0.2F, 0.221F, 0.4F, 0.4F);
        slope(480, 0.2F, 0.222F, 0.4F, 0.4F);
        slope(384, 0.7F, 0.6F, 0.1F, 0.11F);
        for (int height : new int[]{79, 80, 120, 121, 200}) {
            System.out.printf("F %d %s %s %s%n", height,
                    freezes(0.2F, height, 62),
                    freezes(0.6F, height, 62),
                    freezes(0.1F, height, 62));
        }
    }

    private static void slope(int maxY, float west, float east, float north, float south) {
        int half = Math.max(1, 24 / 2);
        float dx = (east - west) * maxY;
        float dz = (south - north) * maxY;
        float gradient = Math.max(Math.abs(dx), Math.abs(dz)) / (half * 2);
        System.out.printf("S %d %s%n", Float.floatToRawIntBits(gradient), gradient > 0.42F);
    }

    private static boolean freezes(float baseTemperature, int height, int seaLevel) {
        int threshold = seaLevel + 17;
        float temperature = baseTemperature;
        if (height > threshold) temperature -= (height - threshold) * 0.05F / 40F;
        return temperature < 0.15F;
    }

    private static float cellRoll(float biomeNoise, int salt) {
        return random(hash(Float.floatToIntBits(biomeNoise), salt));
    }

    private static int hash(int seed, int value) {
        int hash = seed ^ 31337 * value;
        hash = hash * hash * hash * 60493;
        return (hash >> 13) ^ hash;
    }

    private static float random(int value) {
        value ^= 1619;
        value ^= 31337;
        float mapped = (value * value * value * 60493) / 2147483650F;
        float clamped = mapped < -1F ? -1F : mapped > 1F ? 1F : mapped;
        return (clamped + 1F) / 2F;
    }
}
