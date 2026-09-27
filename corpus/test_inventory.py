import copy
import gzip
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import inventory


class InventoryIntegrityTests(unittest.TestCase):
    def source(self):
        return {"id": "sample", "repository": "owner/repository", "commit": "a" * 40, "tree": "b" * 40,
                "documents": [{"path": "docs/README.md", "git_blob": "c" * 40}], "licenses": []}

    def response(self):
        return {"sha": "b" * 40, "truncated": False, "tree": [
            {"path": "docs/README.md", "type": "blob", "mode": "100644", "sha": "c" * 40},
            {"path": "docs", "type": "tree", "mode": "040000", "sha": "d" * 40},
        ]}

    def fixture(self, directory):
        root = Path(directory)
        source = self.source()
        content = inventory.json_bytes({"schema_version": 1, "sources": [source]})
        (root / "corpus.lock.json").write_bytes(content)
        payload = inventory.normalize_tree(self.response(), source)
        archive = inventory.archive_bytes(payload)
        record = {**inventory.identity(source), "archive": "sample.json.gz", "sha256": inventory.digest(archive), "entries": 2}
        lock = {"schema_version": 1, "corpus_lock_sha256": inventory.digest(content), "sources": [record]}
        inventory.write_bytes(root / "inventory/sample.json.gz", archive)
        inventory.write_bytes(root / "inventory/inventory.lock.json", inventory.json_bytes(lock))
        return source, record, lock

    def test_incomplete_or_wrong_trees_and_missing_locked_blobs_are_rejected(self):
        source = self.source()
        response = self.response()
        for changed in [response | {"truncated": True}, response | {"sha": "e" * 40}, response | {"tree": response["tree"][1:]}]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                inventory.normalize_tree(changed, source)
        without_flag = response.copy()
        del without_flag["truncated"]
        with self.assertRaises(ValueError):
            inventory.normalize_tree(without_flag, source)

    def test_names_and_uncertain_entries_are_preserved_as_metadata(self):
        source, response = self.source(), self.response()
        response["tree"].extend([
            {"path": "CON/notes:en?.md", "type": "blob", "mode": "100644", "sha": "a" * 40},
            {"path": "external", "type": "commit", "mode": "160000", "sha": "a" * 40},
            {"path": "link", "type": "blob", "mode": "120000", "sha": "a" * 40},
            {"path": "docs/readme.md", "type": "blob", "mode": "100644", "sha": "a" * 40},
            response["tree"][0].copy(),
        ])
        payload = inventory.normalize_tree(response, source)
        self.assertEqual(len(payload["entries"]), 7)
        self.assertEqual(sum(entry["path"] == "docs/README.md" for entry in payload["entries"]), 2)
        for name in ["../escape", "/absolute", "docs//empty", "docs/./dot", "nul\0byte"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                inventory.valid_path(name)
        inventory.valid_path("C:/Windows")

    def test_archive_is_deterministic_and_identity_is_checked(self):
        source = self.source()
        payload = inventory.normalize_tree(self.response(), source)
        content = inventory.archive_bytes(payload)
        self.assertEqual(content, inventory.archive_bytes(payload))
        self.assertEqual(content[4:8], b"\0\0\0\0")
        self.assertEqual(gzip.decompress(content), inventory.json_bytes(payload))
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            inventory.validate_archive(content, source | {"commit": "e" * 40})

    def test_existing_lock_reuses_verified_archive_without_network(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(inventory, "ROOT", Path(directory)), patch.object(inventory.subprocess, "run") as network:
            source, record, _ = self.fixture(directory)
            self.assertEqual(inventory.fetch_source(source, record), record)
            inventory.verify()
            network.assert_not_called()

    def test_tampered_archive_and_stale_or_unsafe_lock_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(inventory, "ROOT", Path(directory)):
            _, _, lock = self.fixture(directory)
            corpus_lock, corpus_hash = inventory.load_corpus()
            for changed in [lock | {"corpus_lock_sha256": "0" * 64}, lock | {"sources": []}]:
                inventory.write_bytes(Path(directory) / "inventory/inventory.lock.json", inventory.json_bytes(changed))
                with self.assertRaises(ValueError):
                    inventory.load_inventory_lock(corpus_lock, corpus_hash)
            changed = copy.deepcopy(lock)
            changed["sources"][0]["archive"] = "../escape.json.gz"
            inventory.write_bytes(Path(directory) / "inventory/inventory.lock.json", inventory.json_bytes(changed))
            with self.assertRaises(ValueError):
                inventory.load_inventory_lock(corpus_lock, corpus_hash)
            inventory.write_bytes(Path(directory) / "inventory/inventory.lock.json", inventory.json_bytes(lock))
            archive_path = Path(directory) / "inventory/sample.json.gz"
            archive_path.write_bytes(archive_path.read_bytes() + b"tampered")
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                inventory.verify()

    def test_missing_archive_is_fetched_from_pinned_tree_and_compared_to_lock(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(inventory, "ROOT", Path(directory)), patch.object(inventory.subprocess, "run") as network:
            source, record, _ = self.fixture(directory)
            path = Path(directory) / "inventory/sample.json.gz"
            path.unlink()
            network.return_value.stdout = json.dumps(self.response()).encode()
            self.assertEqual(inventory.fetch_source(source, record), record)
            self.assertEqual(network.call_args.args[0], ["gh", "api", "repos/owner/repository/git/trees/" + "b" * 40 + "?recursive=1"])
            path.unlink()
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                inventory.fetch_source(source, record | {"sha256": "0" * 64})
            self.assertFalse(path.exists())


if __name__ == "__main__":
    unittest.main()
