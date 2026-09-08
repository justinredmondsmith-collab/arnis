"""Exercise real AWS/COG decoders with synthetic bytes and no network or cache.

This proves frozen export plumbing, not real-world terrain accuracy.
"""

import argparse
import hashlib
import json
import math
import shutil
import struct
import tempfile
import zlib
from pathlib import Path

from smoke_tiler_cli import ROOT, encoded, run

BBOX = [40.7000, -74.0100, 40.7001, -74.0099]
ESA_URL = (
    "https://esa-worldcover.s3.eu-central-1.amazonaws.com/v200/2021/map/"
    "ESA_WorldCover_10m_2021_v200_N39W075_Map.tif"
)


def png():
    def chunk(kind, data):
        return (
            struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        )

    # Terrarium RGB (128, 20, 0) is exactly 20 metres.
    raw = (b"\0" + bytes([128, 20, 0]) * 256) * 256
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 256, 256, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def fixtures(root):
    entries = []

    def add(kind, key, data):
        path = f"source-{len(entries)}.bin"
        (root / path).write_bytes(data)
        entries.append(
            dict(
                kind=kind,
                key=key,
                path=path,
                sha256=hashlib.sha256(data).hexdigest(),
                size_bytes=len(data),
            )
        )

    add("osm", "master-osm", encoded({"elements": []}))
    add("climate", "koppen_0p1.bin", (ROOT / "assets/climate/koppen_0p1.bin").read_bytes())
    tiles = set()
    for lat in (BBOX[0], BBOX[2]):
        for lon in (BBOX[1], BBOX[3]):
            tiles.add(
                (
                    math.floor((lon + 180) / 360 * 32768),
                    math.floor((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * 32768),
                )
            )
    for x, y in sorted(tiles):
        add("elevation", f"aws:15:{x}:{y}", png())

    # Valid classic TIFF, one 360x360 uncompressed tile covering the 3-degree
    # product. Deliberately low-resolution synthetic data, all grassland.
    pixels = bytes([30]) * (360 * 360)
    tags = [
        (256, 360),
        (257, 360),
        (259, 1),
        (322, 360),
        (323, 360),
        (324, 65536),
        (325, len(pixels)),
    ]
    header = b"II" + struct.pack("<HIH", 42, 8, len(tags))
    header += b"".join(struct.pack("<HHII", tag, 4, 1, value) for tag, value in tags)
    header += bytes(4)
    header = header.ljust(65536, b"\0")
    add("land_cover", f"{ESA_URL}#bytes=0-65535", header)
    add("land_cover", f"{ESA_URL}#bytes=65536-{65536 + len(pixels) - 1}", pixels)
    return entries


def smoke(candidate):
    with tempfile.TemporaryDirectory(prefix="arnis-frozen-export-") as directory:
        root = Path(directory)
        entries = fixtures(root)
        profile = hashlib.sha256(
            encoded(json.loads((ROOT / "docs/contracts/tiler-profile.json").read_text()))
        ).hexdigest()

        def manifest(selected):
            data = encoded(dict(schema_version=1, profile_sha256=profile, entries=selected))
            (root / "sources.json").write_bytes(data)
            return hashlib.sha256(data).hexdigest()

        controls = dict(
            ARNIS_TILER_ABI="1",
            ARNIS_TILER_PROFILE="nyc-conservative-v1",
            ARNIS_TILER_SOURCE_MANIFEST=str(root / "sources.json"),
            ARNIS_SAVE_ELEVATION_GRID=str(root / "master.grid"),
            ARNIS_FETCH_ONLY="1",
        )
        args = [f"--bbox={','.join(map(str, BBOX))}", f"--output-dir={root / 'unused-world'}"]
        for label, selected in [
            ("missing AWS", [e for e in entries if e["kind"] != "elevation"]),
            ("missing ESA range", entries[:-1]),
            ("missing climate", [e for e in entries if e["kind"] != "climate"]),
        ]:
            manifest(selected)
            result = run(candidate, root, args, controls)
            assert result.returncode == 1, (label, result.returncode, result.stderr, result.stdout)
            assert not (root / "master.grid").exists(), label
            assert not (root / "unused-world").exists(), label

        for kind in ("elevation", "land_cover", "climate"):
            entry = next(e for e in entries if e["kind"] == kind)
            path = root / entry["path"]
            original = path.read_bytes()
            corrupt = bytes(len(original))
            path.write_bytes(corrupt)
            selected = [dict(e) for e in entries]
            next(e for e in selected if e["path"] == entry["path"])["sha256"] = hashlib.sha256(
                corrupt
            ).hexdigest()
            manifest(selected)
            result = run(candidate, root, args, controls)
            assert result.returncode == 1, (kind, result.stderr, result.stdout)
            assert not (root / "master.grid").exists(), kind
            path.write_bytes(original)

        source_hash = manifest(entries)
        result = run(candidate, root, args, controls)
        assert result.returncode == 0, result.stderr + result.stdout
        grid = (root / "master.grid").read_bytes()
        assert grid[:9] == b"ARNTGRID2"
        size = struct.unpack_from("<I", grid, 9)[0]
        metadata = json.loads(grid[13 : 13 + size])
        digest = grid[13 + size : 45 + size]
        payload = grid[45 + size :]
        assert hashlib.sha256(grid[: 13 + size] + payload).digest() == digest
        assert metadata["source_manifest_sha256"] == source_hash
        assert metadata["selected_provider"] == "aws"
        assert math.isclose(metadata["min_height_m"], 20.0, abs_tol=1e-8)
        assert all(
            math.isfinite(metadata[key])
            for key in ("min_height_m", "blocks_per_meter", "sea_level_y")
        )
        cells = metadata["width"] * metadata["height"]
        assert len(payload) == cells * 10
        assert all(math.isfinite(y[0]) for y in struct.iter_unpack("<f", payload[: cells * 4]))
        assert set(payload[cells * 4 : cells * 5]) == {30}
        assert not (root / "unused-world").exists()
        assert not (root / "cache").exists(), "frozen export touched cache"
        manifest(entries[:-1])
        result = run(candidate, root, args, controls)
        assert result.returncode == 1, result.stderr + result.stdout
        assert (root / "master.grid").read_bytes() == grid, "failed export replaced master"
        print(
            f"Frozen export: {cells} cells; decoded AWS/ESA values; climate binding; "
            "missing/corrupt inputs fail; failed replacement preserves master; "
            "network isolated, no cache/world output"
        )


def real_smoke(candidate, source_directory):
    spec = json.loads((ROOT / "docs/contracts/frozen-provider-fixture.json").read_text())
    expected = encoded({key: spec[key] for key in ("schema_version", "profile_sha256", "entries")})
    assert (source_directory / "sources.json").read_bytes() == expected
    with tempfile.TemporaryDirectory(prefix="arnis-real-provider-export-") as directory:
        root = Path(directory)
        for entry in spec["entries"]:
            data = (source_directory / entry["path"]).read_bytes()
            assert len(data) == entry["size_bytes"]
            assert hashlib.sha256(data).hexdigest() == entry["sha256"]
            shutil.copyfile(source_directory / entry["path"], root / entry["path"])
        (root / "sources.json").write_bytes(expected)
        grids = []
        for attempt in range(2):
            controls = dict(
                ARNIS_TILER_ABI="1",
                ARNIS_TILER_PROFILE="nyc-conservative-v1",
                ARNIS_TILER_SOURCE_MANIFEST=str(root / "sources.json"),
                ARNIS_SAVE_ELEVATION_GRID=str(root / f"master-{attempt}.grid"),
                ARNIS_FETCH_ONLY="1",
            )
            args = [
                f"--bbox={','.join(map(str, spec['bbox']))}",
                f"--output-dir={root / 'unused-world'}",
            ]
            result = run(candidate, root, args, controls)
            assert result.returncode == 0, result.stdout + result.stderr
            grids.append((root / f"master-{attempt}.grid").read_bytes())
        assert grids[0] == grids[1], "repeated frozen export differs"
        assert not (root / "cache").exists()
        assert not (root / "unused-world").exists()
        print(
            "Real provider fixture: two network-isolated exports identical; "
            f"grid SHA-256 {hashlib.sha256(grids[0]).hexdigest()}"
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    parser.add_argument(
        "--real-sources", type=Path, help="previously acquired pinned fixture directory"
    )
    options = parser.parse_args()
    smoke(options.candidate.resolve())
    if options.real_sources:
        real_smoke(options.candidate.resolve(), options.real_sources.resolve())
