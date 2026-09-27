import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from prepare_npm import prepare


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.wheels = self.root / "wheels"
        self.output = self.root / "npm"
        self.wheels.mkdir()
        self.output.mkdir()
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.0.0"\n', encoding="utf-8")
        (self.output / "package.json").write_text('{"name":"seiso","version":"0.0.0"}', encoding="utf-8")
        (self.root / "README.md").write_text("Readme", encoding="utf-8")
        (self.root / "LICENSE").write_text("License", encoding="utf-8")

    def wheel(self, platform, version="0.0.0"):
        filename = self.wheels / f"seiso-{version}-py3-none-{platform}.whl"
        binary = "seiso.exe" if platform == "win_amd64" else "seiso"
        with zipfile.ZipFile(filename, "w") as archive:
            archive.writestr(f"seiso-{version}.dist-info/METADATA", f"Name: seiso\nVersion: {version}\n")
            archive.writestr(f"seiso-{version}.data/scripts/{binary}", platform.encode())

    def test_packages_both_binaries_and_their_checksums(self):
        self.wheel("win_amd64")
        self.wheel("manylinux_2_17_x86_64")
        prepare(self.wheels, self.output, self.root)
        manifest = json.loads((self.output / "native/manifest.json").read_text())
        self.assertEqual(manifest["version"], "0.0.0")
        self.assertEqual(len(manifest["sha256"]), 2)
        for name, checksum in manifest["sha256"].items():
            self.assertEqual(checksum, hashlib.sha256((self.output / "native" / name).read_bytes()).hexdigest())
        self.assertEqual((self.output / "README.md").read_text(), "Readme")

    def test_missing_platform_does_not_write_a_partial_package(self):
        self.wheel("win_amd64")
        with self.assertRaisesRegex(ValueError, "Both Linux"):
            prepare(self.wheels, self.output, self.root)
        self.assertFalse((self.output / "native").exists())

    def test_mixed_versions_and_duplicate_platforms_fail(self):
        self.wheel("win_amd64", "0.0.1")
        self.wheel("manylinux_2_17_x86_64")
        with self.assertRaisesRegex(ValueError, "does not match"):
            prepare(self.wheels, self.output, self.root)
        self.wheel("manylinux_2_28_x86_64")
        with self.assertRaisesRegex(ValueError, "More than one wheel"):
            prepare(self.wheels, self.output, self.root)


if __name__ == "__main__":
    unittest.main()
