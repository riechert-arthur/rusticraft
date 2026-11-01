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
    public static void main(String[] args) {
        ChunkAccess chunk = new ChunkAccess();
        BiomeSource source = new BiomeSource();
        ChunkUtil.fillNoiseBiomes(chunk, source, new GeneratorResource());
        if (source.calls != 16) throw new AssertionError("call count");
        for (Section section : chunk.sections) {
            for (int y = 0; y < 4; y++) for (int z = 0; z < 4; z++) for (int x = 0; x < 4; x++) {
                String expected = (x - 8) + ":-1:" + (z + 12);
                if (!expected.equals(section.values[x + z * 4 + y * 16]))
                    throw new AssertionError("vertical biome mismatch");
            }
        }
        ChunkAccess failed = new ChunkAccess();
        BiomeSource failing = new BiomeSource();
        failing.failAt = 3;
        try {
            ChunkUtil.fillNoiseBiomes(failed, failing, new GeneratorResource());
            throw new AssertionError("missing exception");
        } catch (IllegalStateException expected) {
            if (failing.calls != 3) throw new AssertionError("failure call order");
            for (Section section : failed.sections)
                for (String value : section.values)
                    if (value != null) throw new AssertionError("partial section write");
        }
        System.out.println("PASS: 16 ordered calls, 384 repeated cells, failure before section writes");
    }
}
class QuartPos { static int fromBlock(int block) { return block >> 2; } }
class Source { static final Object NOOP_CLIMATE_SAMPLER = new Object(); }
class Position {
    int getMinBlockX() { return -2 << 4; }
    int getMinBlockZ() { return 3 << 4; }
}
class HeightAccessor {
    int getMinSection() { return -4; }
    int getMaxSection() { return 2; }
}
class Buffer {
    String[] values = new String[16];
    void set(int x, int z, String biome) { values[x + z * 4] = biome; }
}
class GeneratorResource { Buffer biomeBuffer2D = new Buffer(); }
class BiomeSource {
    int calls, failAt = -1;
    String getNoiseBiome(int x, int y, int z, Object sampler) {
        int index = calls++;
        if (calls == failAt) throw new IllegalStateException("source failed");
        if (x != -8 + index % 4 || y != -1 || z != 12 + index / 4)
            throw new AssertionError("source coordinates/order");
        return x + ":" + y + ":" + z;
    }
}
class Section {
    String[] values = new String[64];
    void fillBiomesFromNoise(Buffer buffer, Object sampler, int x, int z) {
        if (x != 0 || z != 0) throw new AssertionError("buffer origin");
        for (int index = 0; index < 64; index++) values[index] = buffer.values[index % 16];
    }
}
class ChunkAccess {
    Section[] sections = java.util.stream.IntStream.range(0, 6).mapToObj(index -> new Section()).toArray(Section[]::new);
    Position getPos() { return new Position(); }
    HeightAccessor getHeightAccessorForGeneration() { return new HeightAccessor(); }
    int getSectionIndexFromSectionY(int section) { return section + 4; }
    Section getSection(int index) { return sections[index]; }
}
