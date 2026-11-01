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

public final class TerraForgedErosionBrushOracle {
    public static void main(String[] args) {
        for (int size : new int[]{16, 32}) {
            var brushes = new Brushes(size, 7);
            for (int index : new int[]{0, size / 2, (size / 2) * size + size / 2, size * size - 1}) {
                int[] offsets = brushes.edgeOffsets[index];
                float[] weights = offsets == null ? brushes.weights : brushes.edgeWeights[index];
                if (offsets == null) offsets = brushes.offsets;
                long hash = 0xcbf29ce484222325L;
                for (int point = 0; point < offsets.length; point++) {
                    hash = (hash ^ Integer.toUnsignedLong(offsets[point])) * 1099511628211L;
                    hash = (hash ^ Integer.toUnsignedLong(Float.floatToRawIntBits(weights[point]))) * 1099511628211L;
                }
                System.out.printf("%d %d %d 0x%016x%n", size, index, offsets.length, hash);
            }
        }
    }

    private static final class Brushes {
        final int[] offsets;
        final float[] weights;
        final int[][] edgeOffsets;
        final float[][] edgeWeights;

        Brushes(int size, int radius) {
            var disc = new Disc(radius);
            offsets = new int[disc.count];
            weights = new float[disc.count];
            for (int point = 0; point < disc.count; point++) {
                offsets[point] = disc.y[point] * size + disc.x[point];
                weights[point] = disc.weights[point] / disc.weightSum;
            }
            edgeOffsets = new int[size * size][];
            edgeWeights = new float[size * size][];
            for (int index = 0; index < edgeOffsets.length; index++) {
                int centerX = index % size;
                int centerY = index / size;
                boolean clipped = centerX < disc.reach || centerX >= size - disc.reach
                        || centerY < disc.reach || centerY >= size - disc.reach;
                if (!clipped) continue;
                float weightSum = 0;
                int count = 0;
                for (int point = 0; point < disc.count; point++) {
                    int x = centerX + disc.x[point];
                    int y = centerY + disc.y[point];
                    if (x >= 0 && x < size && y >= 0 && y < size) {
                        weightSum += disc.weights[point];
                        count++;
                    }
                }
                int[] edgeOffset = new int[count];
                float[] edgeWeight = new float[count];
                int at = 0;
                for (int point = 0; point < disc.count; point++) {
                    int x = centerX + disc.x[point];
                    int y = centerY + disc.y[point];
                    if (x >= 0 && x < size && y >= 0 && y < size) {
                        edgeOffset[at] = disc.y[point] * size + disc.x[point];
                        edgeWeight[at] = disc.weights[point] / weightSum;
                        at++;
                    }
                }
                edgeOffsets[index] = edgeOffset;
                edgeWeights[index] = edgeWeight;
            }
        }
    }

    private static final class Disc {
        final int[] x;
        final int[] y;
        final float[] weights;
        final int count;
        final float weightSum;
        final int reach;

        Disc(int radius) {
            x = new int[radius * radius * 4];
            y = new int[radius * radius * 4];
            weights = new float[radius * radius * 4];
            int count = 0;
            float weightSum = 0;
            int reach = 0;
            for (int dy = -radius; dy <= radius; dy++) {
                for (int dx = -radius; dx <= radius; dx++) {
                    float distance = dx * dx + dy * dy;
                    if (distance < radius * radius) {
                        weights[count] = 1 - (float) Math.sqrt(distance) / radius;
                        weightSum += weights[count];
                        x[count] = dx;
                        y[count] = dy;
                        count++;
                        reach = Math.max(reach, Math.max(Math.abs(dx), Math.abs(dy)));
                    }
                }
            }
            this.count = count;
            this.weightSum = weightSum;
            this.reach = reach;
        }
    }
}
