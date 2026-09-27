import unittest

import review_links


class IndependentLinkOracleTests(unittest.TestCase):
    def entries(self):
        return [{"path": "docs", "mode": "040000"},
                {"path": "docs/Guide.md", "mode": "100644"},
                {"path": "docs/unicode 日本語.md", "mode": "100644"},
                {"path": "vendor", "mode": "160000"},
                {"path": "shortcut", "mode": "120000"}]

    def test_physical_targets_resolve_case_sensitively_and_decode_once(self):
        entries = self.entries()
        self.assertEqual(review_links.resolve_target("docs/README.md", "Guide.md?q=1#section", entries)[:2], ("fp", "docs/Guide.md"))
        self.assertEqual(review_links.resolve_target("docs/README.md", "guide.md", entries)[:2], ("tp", "docs/guide.md"))
        self.assertEqual(review_links.resolve_target("docs/README.md", "/docs", entries)[:2], ("fp", "docs"))
        self.assertEqual(review_links.resolve_target("docs/README.md", "unicode%20%E6%97%A5%E6%9C%AC%E8%AA%9E.md", entries)[0], "fp")
        self.assertEqual(review_links.resolve_target("docs/README.md", "page%3Fq.md", entries)[:2], ("tp", "docs/page?q.md"))

    def test_unknown_paths_never_become_positive(self):
        for destination in ["../vendor/file.md", "../shortcut/../absent.md", "../../outside.md", "page%2.md", "page%FF.md", "${name}.md", "{{ name }}.md", "../shortcut"]:
            with self.subTest(destination=destination):
                self.assertEqual(review_links.resolve_target("docs/README.md", destination, self.entries())[0], "uncertain")
        duplicated = self.entries() + [{"path": "docs/Guide.md", "mode": "100644"}]
        self.assertEqual(review_links.resolve_target("docs/README.md", "Guide.md", duplicated)[0], "uncertain")

    def test_external_and_fragment_only_links_are_outside_check(self):
        for destination in ["https://example.com/a", "//example.com/a", "mailto:test@example.com", "#section", "?search"]:
            with self.subTest(destination=destination):
                self.assertEqual(review_links.resolve_target("docs/README.md", destination, self.entries())[0], "fp")

    def test_reference_definition_and_source_destination_are_independent_checks(self):
        raw = b"[Guide][ref]\n\n[REF]: missing.md\n"
        start = raw.index(b"missing.md")
        link = {"span": {"start": 0, "end": 12}, "destination_span": {"start": start, "end": start + 10},
                "reference": "ref", "destination": "missing.md"}
        self.assertTrue(review_links.source_link(raw, link)[0])
        self.assertTrue(review_links.source_link(raw, link | {"reference": "another"})[0])
        self.assertFalse(review_links.source_link(raw, link | {"destination": "elsewhere.md"})[0])
        malformed = raw.replace(b"[REF]", b"[BAD]")
        self.assertFalse(review_links.source_link(malformed, link)[0])

    def test_multiline_reference_definitions_are_verified_from_original_bytes(self):
        raw = b"[Guide][ref]\n\n[REF]:\n  missing.md\n"
        start = raw.index(b"missing.md")
        link = {"span": {"start": 0, "end": 12}, "destination_span": {"start": start, "end": start + 10},
                "reference": "ref", "destination": "missing.md"}
        self.assertTrue(review_links.source_link(raw, link)[0])

    def test_manual_review_receipt_requires_matching_report_and_sample(self):
        result = {"report_sha256": "a" * 64, "labels": [{"id": "one", "split": "holdout"}]}
        receipt = {"schema_version": 1, "report_sha256": "a" * 64, "reviewer_kind": "independent_agent_with_oracle",
                   "manually_reviewed_sample_ids": ["one"], "additional_reviewed_ids": [], "findings": ["Reviewed original source."]}
        with self.assertRaises(ValueError):
            review_links.apply_manual_review(result, receipt | {"report_sha256": "b" * 64})
        with self.assertRaises(ValueError):
            review_links.apply_manual_review(result, receipt | {"manually_reviewed_sample_ids": []})
        self.assertTrue(review_links.apply_manual_review(result, receipt)["labels"][0]["agent_context_reviewed"])


if __name__ == "__main__":
    unittest.main()
