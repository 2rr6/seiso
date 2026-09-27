from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import github_release


class GitHubReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Release Tests")
        self.git("config", "user.email", "tests@example.com")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        self.commit("Old release")
        self.git("tag", "v1.0.0")
        self.commit("Fix links")
        self.notes = self.root / "notes.md"
        self.notes.write_text("## Changes\n\nFix links.\n")
        self.assets = self.root / "dist"
        self.assets.mkdir()
        (self.assets / "seiso.tgz").write_bytes(b"test archive")

    def git(self, *args):
        return github_release.git(*args, root=self.root)

    def commit(self, message):
        (self.root / "source.txt").write_text(message)
        self.git("add", "source.txt")
        self.git("commit", "--quiet", "-m", message)

    def test_notes_combine_manual_text_and_exact_commit_range(self):
        note = self.root / "docs/release-notes/1.1.0.md"
        note.parent.mkdir(parents=True)
        note.write_text("## Better links\n\nSupports anchors.\n")
        body = github_release.compose_notes("1.1.0", "scarletkc/seiso", self.root)
        self.assertTrue(body.startswith("## Better links\n\nSupports anchors."))
        self.assertIn("Fix links", body)
        self.assertNotIn("Old release (", body)
        self.assertIn("compare/v1.0.0...v1.1.0", body)

    def test_first_release_lists_all_history(self):
        self.git("tag", "-d", "v1.0.0")
        body = github_release.compose_notes("1.1.0", "scarletkc/seiso", self.root)
        self.assertIn("Old release", body)
        self.assertIn("Fix links", body)
        self.assertNotIn("Full diff", body)

    def test_rerun_excludes_current_tag_from_previous_selection(self):
        self.git("tag", "v1.1.0")
        body = github_release.compose_notes("1.1.0", "scarletkc/seiso", self.root)
        self.assertIn("Changes since v1.0.0", body)
        self.assertIn("Fix links", body)

    def test_conflicting_tag_fails(self):
        with self.assertRaisesRegex(ValueError, "another commit"):
            github_release.compose_notes("1.0.0", "scarletkc/seiso", self.root)

    def test_note_requires_heading_and_body(self):
        for contents in ["", "# Title\n\nBody", "## Title\n\n", "##  \nBody"]:
            with self.subTest(contents=contents), self.assertRaises(ValueError):
                self.notes.write_text(contents)
                github_release.handwritten_note(self.notes)

    @patch("github_release.run")
    @patch("github_release.get_release")
    def test_default_never_queries_or_writes_github(self, lookup, run):
        github_release.publish_github("1.1.0", "owner/repo", self.notes, self.assets, root=self.root)
        lookup.assert_not_called()
        run.assert_not_called()

    @patch("github_release.run")
    @patch("github_release.get_release", return_value=None)
    def test_create_pins_commit_and_uses_notes_file(self, lookup, run):
        github_release.publish_github("1.1.0", "owner/repo", self.notes, self.assets,
                                      execute=True, root=self.root)
        args = run.call_args_list[0].args[0]
        self.assertEqual(args[:4], ["gh", "release", "create", "v1.1.0"])
        self.assertEqual(args[args.index("--target") + 1], self.git("rev-parse", "HEAD"))
        self.assertEqual(args[args.index("--notes-file") + 1], str(self.notes))
        self.assertEqual(run.call_args_list[1].args[0][:3], ["gh", "release", "upload"])

    @patch("github_release.run")
    @patch("github_release.get_release", return_value={"draft": False, "assets": [{"name": "seiso.tgz"}]})
    def test_retry_only_uploads_missing_assets(self, lookup, run):
        (self.assets / "seiso.crate").write_bytes(b"crate")
        github_release.publish_github("1.1.0", "owner/repo", self.notes, self.assets,
                                      execute=True, root=self.root)
        run.assert_called_once()
        args = run.call_args.args[0]
        self.assertEqual(args[-1], str(self.assets / "seiso.crate"))
        self.assertNotIn(str(self.assets / "seiso.tgz"), args)

    @patch("github_release.run")
    @patch("github_release.get_release", return_value={"draft": False, "assets": [{"name": "seiso.tgz"}]})
    def test_complete_retry_leaves_release_unchanged(self, lookup, run):
        github_release.publish_github("1.1.0", "owner/repo", self.notes, self.assets,
                                      execute=True, root=self.root)
        run.assert_not_called()

    @patch("github_release.run")
    @patch("github_release.get_release", side_effect=RuntimeError("API unavailable"))
    def test_lookup_error_never_creates_release(self, lookup, run):
        with self.assertRaises(RuntimeError):
            github_release.publish_github("1.1.0", "owner/repo", self.notes, self.assets,
                                          execute=True, root=self.root)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
