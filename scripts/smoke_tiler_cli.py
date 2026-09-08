"""Disposable, network-isolated ABI smoke; this is not NYC qualification."""

import argparse
import hashlib
import io
import json
import os
import struct
import subprocess
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def run(binary, root, args, controls=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith("ARNIS_")}
    env.update(controls or {})
    env.update(
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_CONFIG_HOME=str(root / "config"),
        RAYON_NUM_THREADS="1",
    )
    return subprocess.run(
        [
            "bwrap",
            "--ro-bind",
            "/",
            "/",
            "--bind",
            str(root),
            str(root),
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--unshare-net",
            str(binary),
            *args,
        ],
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )


def region_payloads(path):
    """Ignore region header timestamps; validate and return decompressed chunk NBT."""
    chunks = {}
    for region in sorted(path.glob("*.mca")):
        data = region.read_bytes()
        assert len(data) >= 8192, region
        for index in range(1024):
            entry = int.from_bytes(data[index * 4 : index * 4 + 4], "big")
            if not entry:
                continue
            offset, sectors = (entry >> 8) * 4096, entry & 255
            length = int.from_bytes(data[offset : offset + 4], "big")
            assert 1 < length <= sectors * 4096 - 4
            assert data[offset + 4] == 2, "expected zlib NBT"
            payload = zlib.decompress(data[offset + 5 : offset + 4 + length])
            assert payload[0] == 10, "expected NBT compound"
            chunks[(region.name, index)] = payload
    assert chunks, "no readable chunks"
    return chunks


def normalized_nbt(payload):
    # nbtlib is already a dependency of the sibling arnis-tiler project.
    from collections.abc import Mapping

    import nbtlib

    def normalize(value):
        kind = type(value).__name__
        if isinstance(value, Mapping):
            return kind, tuple((key, normalize(item)) for key, item in sorted(value.items()))
        if isinstance(value, list):
            return kind, tuple(normalize(item) for item in value)
        if hasattr(value, "tolist"):
            return kind, value.tolist()
        return kind, str(value)

    return normalize(nbtlib.File.parse(io.BytesIO(payload)))


def smoke(candidate, stock=None):
    with tempfile.TemporaryDirectory(prefix="arnis-v31-smoke-") as directory:
        root = Path(directory)
        query = run(candidate, root, ["--tiler-capabilities"])
        assert query.returncode == 0 and not query.stderr, query.stderr
        report = json.loads(query.stdout)
        assert report["upstream"]["commit"] == "3918513acb4e5e9ef4332418531a7c444d2b5acf"
        assert not report["capabilities"], "development smoke must not imply qualification"
        assert not list(root.iterdir()), "capability query had filesystem side effects"
        for args in [
            ["--tiler-capabilities=true"],
            ["--tiler-capabilities", "--bbox=40,-74,41,-73"],
        ]:
            result = run(candidate, root, args)
            assert result.returncode == 2 and not result.stdout
        partial = run(candidate, root, [], {"ARNIS_FETCH_ONLY": "1"})
        assert partial.returncode == 2 and not partial.stdout
        assert not list(root.iterdir())

        osm = encoded({"elements": []})
        (root / "osm.json").write_bytes(osm)
        climate = (ROOT / "assets/climate/koppen_0p1.bin").read_bytes()
        (root / "koppen_0p1.bin").write_bytes(climate)
        profile_hash = hashlib.sha256(
            encoded(json.loads((ROOT / "docs/contracts/tiler-profile.json").read_text()))
        ).hexdigest()
        manifest = encoded(
            {
                "schema_version": 1,
                "profile_sha256": profile_hash,
                "entries": [
                    {
                        "kind": "osm",
                        "key": "master-osm",
                        "path": "osm.json",
                        "sha256": hashlib.sha256(osm).hexdigest(),
                        "size_bytes": len(osm),
                    },
                    {
                        "kind": "climate",
                        "key": "koppen_0p1.bin",
                        "path": "koppen_0p1.bin",
                        "sha256": hashlib.sha256(climate).hexdigest(),
                        "size_bytes": len(climate),
                    },
                ],
            }
        )
        (root / "sources.json").write_bytes(manifest)
        meta = {
            "format_version": 2,
            "contract": "arnis-tiler/v3.1/1",
            "bbox": [40.0, -74.0, 40.0001, -73.9999],
            "width": 4,
            "height": 4,
            "world_width": 4,
            "world_height": 4,
            "scale": 1.0,
            "projection": "local",
            "orientation": "northwest-row-major",
            "min_height_m": 0.0,
            "blocks_per_meter": 1.0,
            "effective_ground_level": 10,
            "requested_ground_level": 10,
            "min_ground_level": 10,
            "extended_max_y": 2031,
            "water_floor": 10,
            "sink_floor": 10,
            "sea_level_y": 10.0,
            "source_mode": "aws-only",
            "selected_provider": "aws",
            "provider_attempts": [{"name": "aws", "outcome": "success"}],
            "source_manifest_sha256": hashlib.sha256(manifest).hexdigest(),
            "profile_sha256": profile_hash,
            "postprocess": "master-once-v1",
            "height_units": "minecraft_y",
            "payload_bytes": 160,
            "land_cover_cells_per_meter": 1.0,
            "climate": "Temperate",
            "snow_threshold_y": 150,
            "climate_anchor": [40.00005, -73.99995],
        }
        header = encoded(meta)
        prefix = b"ARNTGRID2" + struct.pack("<I", len(header)) + header
        payload = struct.pack("<16f", *([10.0] * 16)) + bytes([30] * 16) + bytes(16) + bytes(64)
        grid = prefix + hashlib.sha256(prefix + payload).digest() + payload
        (root / "master.grid").write_bytes(grid)
        controls = {
            "ARNIS_TILER_ABI": "1",
            "ARNIS_TILER_PROFILE": "nyc-conservative-v1",
            "ARNIS_TILER_SOURCE_MANIFEST": str(root / "sources.json"),
            "ARNIS_TILED_RENDER": "1",
            "ARNIS_USE_ELEVATION_GRID": str(root / "master.grid"),
            "ARNIS_TILE_MASTER_OFFSET": "0,0",
            "ARNIS_TILE_OVERRIDE_DIMS": "4,4",
        }
        args = ["--bbox=40,-74,40.0001,-73.9999", f"--output-dir={root / 'tile'}"]
        result = run(candidate, root, args, controls)
        assert result.returncode == 0, result.stderr + result.stdout
        assert [p.name for p in (root / "tile").iterdir()] == ["region"]
        chunks = region_payloads(root / "tile/region")
        print(
            f"ABI smoke: standalone JSON; malformed=2; {len(chunks)} readable Java chunks; "
            "region-only output; network namespace isolated"
        )
        if stock:
            rejected = run(stock, root, ["--tiler-capabilities"])
            assert rejected.returncode != 0
            # A bounded stock-behavior oracle: flat, outside ESA coverage, no integration controls.
            # Full terrain/provider and NYC qualification belongs to the frozen-input harness.
            manifests = []
            for label, binary in [("stock", stock), ("candidate", candidate)]:
                output = root / label
                options = [
                    "--bbox=85,0,85.00002,0.0002",
                    f"--output-dir={output}",
                    f"--file={root / 'osm.json'}",
                    "--mode=geo-only",
                    "--legacy-trees",
                    "--canopy-height=false",
                    "--overture=false",
                    "--no-3d",
                    "--map-item=false",
                    "--signage=none",
                ]
                result = run(binary, root, options)
                assert result.returncode == 0, result.stderr + result.stdout
                worlds = list(output.glob("*/region"))
                assert len(worlds) == 1, worlds
                manifests.append(region_payloads(worlds[0]))
            assert manifests[0].keys() == manifests[1].keys(), "stock/fork chunk set differs"
            for key in manifests[0]:
                assert normalized_nbt(manifests[0][key]) == normalized_nbt(manifests[1][key]), (
                    f"stock/fork semantic chunk NBT differs: {key}"
                )
            print(
                f"Stock oracle: {len(manifests[0])} normalized chunk NBT payloads identical; "
                "stock capability probe rejected"
            )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--stock", type=Path)
    options = parser.parse_args()
    smoke(options.candidate.resolve(), options.stock.resolve() if options.stock else None)
