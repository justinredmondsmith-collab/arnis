"""Check build identity admission without a Cargo dependency rebuild."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class BuildIdentityTests(unittest.TestCase):
    def test_archive_inside_unrelated_repository_requires_explicit_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build = root / "build-script"
            subprocess.run(
                ["rustc", "--edition=2021", str(ROOT / "src/build.rs"), "-o", str(build)],
                check=True,
            )
            subprocess.run(["git", "init", "-q", str(root / "enclosing")], check=True)
            enclosing = root / "enclosing"
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "unrelated",
                ],
                cwd=enclosing,
                check=True,
            )
            archive = enclosing / "archive"
            archive.mkdir()
            env = {
                key: value
                for key, value in os.environ.items()
                if not key.startswith("ARNIS_BUILD_")
            }
            env.update(CARGO_MANIFEST_DIR=str(archive), TARGET="fixture-target", RUSTC="rustc")
            missing = subprocess.run(
                [str(build)], cwd=archive, env=env, capture_output=True, text=True
            )
            self.assertNotEqual(missing.returncode, 0, "must not borrow enclosing Git identity")
            env.update(ARNIS_BUILD_COMMIT="a" * 40, ARNIS_BUILD_DIRTY="true")
            explicit = subprocess.run(
                [str(build)], cwd=archive, env=env, capture_output=True, text=True
            )
            self.assertEqual(explicit.returncode, 0, explicit.stderr)
            self.assertIn("ARNIS_SOURCE_COMMIT=" + "a" * 40, explicit.stdout)
            self.assertIn("ARNIS_SOURCE_DIRTY=true", explicit.stdout)
            env["CARGO_MANIFEST_DIR"] = str(ROOT)
            own = subprocess.run([str(build)], cwd=ROOT, env=env, capture_output=True, text=True)
            self.assertEqual(own.returncode, 0, own.stderr)
            head = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip()
            self.assertIn("ARNIS_SOURCE_COMMIT=" + head, own.stdout)


if __name__ == "__main__":
    unittest.main()
