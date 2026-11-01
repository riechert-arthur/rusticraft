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

public final class TerraForgedBiomeChunkFillOracle {
    private static final int CAVE_BIOME_MAX_Y = 0;

    public static void main(String[] args) {
        for (int sectionY = -2; sectionY <= 1; sectionY++) {
            System.out.printf("S %d %s%n", sectionY, isCaveBiomeSection(sectionY));
        }
        for (int quartY = -5; quartY <= 4; quartY++) {
            System.out.printf("Q %d %s%n", quartY, isCaveBiomeQuart(quartY));
        }

        int chunkX = -2;
        int chunkZ = 3;
        int biomeX = chunkX << 2;
        int biomeZ = chunkZ << 2;
        boolean anyCave = false;
        String[] surface = new String[16];
        String[] cave = new String[16];
        for (int dz = 0; dz < 4; dz++) {
            for (int dx = 0; dx < 4; dx++) {
                int index = dx + dz * 4;
                int x = biomeX + dx;
                int z = biomeZ + dz;
                surface[index] = "surface:" + x + ":" + z;
                String layer = (x + z & 1) == 0 ? "cave:" + x + ":" + z : null;
                anyCave |= layer != null;
                cave[index] = layer != null ? layer : surface[index];
                System.out.printf("C %d %d%n", x, z);
            }
        }
        System.out.printf("B %s %s %s%n", anyCave, cave[0], cave[1]);
        System.out.printf("Y %s %s%n", cave[0], surface[0]);
    }

    private static boolean isCaveBiomeSection(int sectionY) {
        return (sectionY << 4) + 15 <= CAVE_BIOME_MAX_Y;
    }

    private static boolean isCaveBiomeQuart(int quartY) {
        return isCaveBiomeSection(quartY >> 2);
    }
}
