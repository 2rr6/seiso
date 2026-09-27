import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import corpus


class CorpusIntegrityTests(unittest.TestCase):
    def source(self, content=b"# Docs\n"):
        entry = {"path": "docs/readme.md", "bytes": len(content), "git_blob": corpus.blob_hash(content),
                 "sha256": hashlib.sha256(content).hexdigest()}
        source = {"id": "sample", "repository": "owner/repository", "commit": "a" * 40,
                  "documents": [entry], "licenses": []}
        return source, entry

    def test_revisions_and_cache_keys_must_be_immutable_hashes(self):
        source, entry = self.source()
        for changed in [source | {"commit": "main"}, source | {"id": "../escape"},
                        source | {"documents": [entry | {"git_blob": "../escape"}]}]:
            with self.assertRaises(ValueError):
                corpus.validate_lock({"schema_version": 1, "sources": [changed]})

    def test_duplicate_source_repositories_are_rejected(self):
        source, _ = self.source()
        with self.assertRaises(ValueError):
            corpus.validate_lock({"schema_version": 1, "sources": [source, source | {"id": "other"}]})

    def test_verified_cache_is_used_without_network(self):
        source, entry = self.source()
        with tempfile.TemporaryDirectory() as directory, patch.object(corpus, "ROOT", Path(directory)), patch.object(corpus, "urlopen") as network:
            path = Path(directory) / "data/blobs" / entry["git_blob"]
            path.parent.mkdir(parents=True)
            path.write_bytes(b"# Docs\n")
            self.assertEqual(corpus.fetch_blob(source, entry), entry["sha256"])
            network.assert_not_called()

    def test_wrong_download_is_rejected_before_cache_write(self):
        source, entry = self.source()
        with tempfile.TemporaryDirectory() as directory, patch.object(corpus, "ROOT", Path(directory)), patch.object(corpus, "urlopen") as network:
            network.return_value.__enter__.return_value.read.return_value = b"Changed"
            with self.assertRaisesRegex(ValueError, "Git blob mismatch"):
                corpus.fetch_blob(source, entry)
            self.assertFalse((Path(directory) / "data").exists())


if __name__ == "__main__":
    unittest.main()
