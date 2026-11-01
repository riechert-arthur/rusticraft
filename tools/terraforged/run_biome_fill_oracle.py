"""Execute the original ChunkUtil.fillNoiseBiomes method with test chunk stand-ins."""

import argparse
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/util/ChunkUtil.java").read_text()
    start = source.index("    public static void fillNoiseBiomes(")
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        if source[end] == "{":
            depth += 1
        elif source[end] == "}":
            depth -= 1
        end += 1
    method = source[start:end]
    with tempfile.TemporaryDirectory(prefix="tf-biome-fill-") as temporary:
        directory = Path(temporary)
        reference = directory / "ChunkUtil.java"
        reference.write_text("class ChunkUtil {\n" + method + "\n}\n")
        driver = Path(__file__).with_name("TerraForgedBiomeChunkFillOracle.java")
        subprocess.run(["javac", "-d", str(directory), str(reference), str(driver)], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", str(directory),
                        "TerraForgedBiomeChunkFillOracle"], check=True)


if __name__ == "__main__":
    main()
