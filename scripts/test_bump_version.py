from pathlib import Path
import tempfile
import tomllib
import unittest

from bump_version import next_version, plan_bump


class BumpVersionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "npm/seiso").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "seiso"\nversion = "1.2.3"\n'
            '[dependencies]\nexternal = "1.2.3"\n')
        (self.root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "seiso"\nversion = "1.2.3"\n'
            'dependencies = ["external 1.2.3"]\n'
            '[[package]]\nname = "external"\nversion = "1.2.3"\nsource = "registry+https://example.com"\n')
        (self.root / "npm/seiso/package.json").write_text(
            '{"name":"@scarletkc/seiso", "version": "1.2.3", "custom": "1.2.3"}\n')

    def test_version_modes_and_optional_v_prefix(self):
        for requested, expected in [("patch", "1.2.4"), ("minor", "1.3.0"),
                                    ("major", "2.0.0"), ("v2.3.4", "2.3.4")]:
            self.assertEqual(next_version("1.2.3", requested), expected)

    def test_invalid_equal_and_lower_versions_fail(self):
        for requested in ["1.2.3", "0.2.3", "01.3.0", "2.0", "2.0.0-rc.1", "2.0.0+build"]:
            with self.subTest(version=requested), self.assertRaises(ValueError):
                next_version("1.2.3", requested)

    def test_plan_updates_package_without_touching_external_versions(self):
        changes = plan_bump(self.root, "1.2.3", "1.2.4", "Fix links")
        manifest = tomllib.loads(changes[self.root / "Cargo.toml"])
        self.assertEqual(manifest["package"]["version"], "1.2.4")
        self.assertEqual(manifest["dependencies"]["external"], "1.2.3")
        lock = tomllib.loads(changes[self.root / "Cargo.lock"])["package"]
        self.assertEqual([p["version"] for p in lock], ["1.2.4", "1.2.3"])
        self.assertEqual(lock[0]["dependencies"], ["external 1.2.3"])
        self.assertIn('"custom": "1.2.3"', changes[self.root / "npm/seiso/package.json"])
        self.assertEqual(changes[self.root / "docs/release-notes/1.2.4.md"], "## Fix links\n\n")
        self.assertIn('version = "1.2.3"', (self.root / "Cargo.toml").read_text())

    def test_existing_note_is_preserved_and_no_versions_are_written(self):
        note = self.root / "docs/release-notes/1.2.4.md"
        note.parent.mkdir(parents=True)
        note.write_text("## Existing\n\nKeep this.\n")
        before = {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        with self.assertRaisesRegex(ValueError, "already exists"):
            plan_bump(self.root, "1.2.3", "1.2.4", "New title")
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_bad_note_title_fails_before_any_write(self):
        for note in ["", "   ", "Title\nSecond line"]:
            with self.subTest(note=note), self.assertRaisesRegex(ValueError, "single-line"):
                plan_bump(self.root, "1.2.3", "1.2.4", note)

    def test_missing_lock_package_is_rejected(self):
        (self.root / "Cargo.lock").write_text('version = 4\n')
        with self.assertRaisesRegex(ValueError, "must contain"):
            plan_bump(self.root, "1.2.3", "1.2.4")


if __name__ == "__main__":
    unittest.main()
