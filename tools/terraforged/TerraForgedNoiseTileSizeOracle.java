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

public final class TerraForgedNoiseTileSizeOracle {
    public static void main(String[] args) {
        for (int radius : new int[]{0, 1, 2, -2}) {
            var size = new NoiseTileSize(radius);
            System.out.printf(
                    "%d: %d %d %d %d %d %d %d %d | %d %d %d %d%n",
                    radius,
                    size.chunkLength, size.chunkSize, size.chunkMin, size.chunkMax,
                    size.regionLength, size.regionSize, size.min, size.max,
                    size.chunkIndexOf(-7, 1), size.chunkIndexOfRel(-1, 1),
                    size.indexOf(-11, -4), size.indexOfRel(-11, -4));
        }
        System.out.println(NoiseTileSize.DEFAULT);
    }

    private static final class NoiseTileSize {
        static final int CHUNK_SIZE = 16;
        static final NoiseTileSize DEFAULT = new NoiseTileSize(1);

        final int chunkLength;
        final int chunkSize;
        final int chunkMin;
        final int chunkMax;
        final int regionLength;
        final int regionSize;
        final int min;
        final int max;

        NoiseTileSize(int chunkRadius) {
            chunkLength = 1 + chunkRadius * 2;
            chunkSize = chunkLength * chunkLength;
            chunkMin = -chunkRadius;
            chunkMax = chunkMin + chunkLength;
            regionLength = chunkLength * CHUNK_SIZE;
            regionSize = regionLength * regionLength;
            min = -CHUNK_SIZE * chunkRadius;
            max = min + regionLength;
        }

        int chunkIndexOf(int x, int z) {
            return z * chunkLength + x;
        }

        int chunkIndexOfRel(int x, int z) {
            x -= chunkMin;
            z -= chunkMin;
            return z * chunkLength + x;
        }

        int indexOf(int x, int z) {
            return z * regionLength + x;
        }

        int indexOfRel(int x, int z) {
            x -= min;
            z -= min;
            return indexOf(x, z);
        }

        @Override
        public String toString() {
            return "NoiseTileSize{" +
                    "chunkDiameter=" + chunkLength +
                    ", chunkSize=" + chunkSize +
                    ", chunkMin=" + chunkMin +
                    ", chunkMax=" + chunkMax +
                    ", regionDiameter=" + regionLength +
                    ", regionSize=" + regionSize +
                    ", min=" + min +
                    ", max=" + max +
                    '}';
        }
    }
}
