"""Acquire the checksum-pinned, tiny provider fixture before offline testing.

This separate acquisition command uses HTTP. Renderer invocations do not.
Changed remote data is rejected and requires a reviewed fixture update.
"""

import argparse
import hashlib
import json
import urllib.request
from pathlib import Path

from smoke_tiler_cli import ROOT, encoded


def acquire(destination):
    spec = json.loads((ROOT / "docs/contracts/frozen-provider-fixture.json").read_text())
    destination.mkdir(parents=True, exist_ok=False)
    for entry in spec["entries"]:
        kind, key = entry["kind"], entry["key"]
        if kind == "osm":
            data = encoded({"elements": []})
        elif kind == "climate":
            data = (ROOT / "assets/climate" / key).read_bytes()
        else:
            headers = {}
            if kind == "elevation":
                provider, z, x, y = key.split(":")
                assert provider == "aws"
                url = f"https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"
            elif kind == "land_cover":
                url, interval = key.split("#bytes=")
                headers["Range"] = f"bytes={interval}"
            else:
                raise ValueError(f"Unsupported fixture kind: {kind}")
            request = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(request, timeout=30) as response:
                if headers:
                    expected = f"bytes {interval}/"
                    if response.status != 206 or not response.headers.get(
                        "Content-Range", ""
                    ).startswith(expected):
                        raise ValueError("Provider did not honor exact byte range")
                data = response.read(entry["size_bytes"] + 1)
        if len(data) != entry["size_bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
            raise ValueError(f"Pinned fixture changed: {kind}/{key}")
        (destination / entry["path"]).write_bytes(data)
    manifest = {k: spec[k] for k in ("schema_version", "profile_sha256", "entries")}
    (destination / "sources.json").write_bytes(encoded(manifest))
    print(f"Acquired {len(spec['entries'])} exact sources in {destination}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new disposable directory")
    acquire(parser.parse_args().destination.resolve())
