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

public final class TerraForgedErosionRegionOracle {
    private static final int REGION_CHUNKS = 8;
    private static final int MARGIN_BLOCKS = 64;

    public static void main(String[] args) {
        for (int chunk : new int[]{-17, -16, -9, -8, -1, 0, 7, 8, 15, 16}) {
            int region = Math.floorDiv(chunk, REGION_CHUNKS);
            int origin = region * REGION_CHUNKS - 4;
            int offset = MARGIN_BLOCKS + (Math.floorMod(chunk, REGION_CHUNKS) << 4);
            System.out.printf("C %d %d %d %d%n", chunk, region, origin, offset);
        }
        for (int block : new int[]{-257, -256, -145, -144, -17, -16, -1, 0, 15, 16, 127, 128}) {
            int offset = MARGIN_BLOCKS
                    + (Math.floorMod(block >> 4, REGION_CHUNKS) << 4)
                    + (block & 15);
            System.out.printf("B %d %d%n", block, offset);
        }
    }
}
