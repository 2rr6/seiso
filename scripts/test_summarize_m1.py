import unittest
from summarize_m1 import summarize


class AcceptanceTests(unittest.TestCase):
    def fixture(self, count=100, false=0):
        diagnoses = [{"id": str(i), "code": "KND001", "split": "holdout", "language": "en", "kind": "unknown"} for i in range(count)]
        labels = [{"id": str(i), "label": "fp" if i < false else "tp", "evidence": "Independently checked declaration."} for i in range(count)]
        return {"diagnostics": diagnoses}, {"report_sha256": "run", "labels": labels, "manually_reviewed_sample_ids": [str(i) for i in range(count)]}

    def test_threshold_is_per_rule_and_small_or_tuning_only_samples_cannot_pass(self):
        for count, false, expected in [(100, 5, True), (100, 6, False), (99, 0, False)]:
            report, annotations = self.fixture(count, false)
            result = summarize(report, "run", [annotations])
            self.assertEqual(result["rules"]["KND001"]["eligible_for_stable"], expected)
            self.assertFalse(result["m1_exit_passed"])
        report, annotations = self.fixture()
        for item in report["diagnostics"]:
            item["split"] = "tuning"
        self.assertFalse(summarize(report, "run", [annotations])["rules"]["KND001"]["eligible_for_stable"])

    def test_empty_protocol_samples_remain_unavailable_even_with_exception(self):
        result = summarize({"diagnostics": []}, "run", [], True)
        self.assertTrue(result["rules"]["SUP001"]["eligible_for_stable"])
        self.assertIsNone(result["rules"]["SUP001"]["holdout"]["precision"])
        self.assertFalse(result["m1_exit_passed"])

    def test_missing_duplicate_or_unbound_labels_fail_closed(self):
        report, annotations = self.fixture()
        for invalid in [annotations | {"labels": annotations["labels"][:-1]}, annotations | {"report_sha256": "other"},
                        annotations | {"labels": annotations["labels"] * 2}]:
            with self.assertRaises(ValueError):
                summarize(report, "run", [invalid])

    def test_uncertainty_and_unreviewed_oracle_labels_cannot_inflate_acceptance(self):
        report, annotations = self.fixture()
        annotations["manually_reviewed_sample_ids"] = []
        self.assertFalse(summarize(report, "run", [annotations])["rules"]["KND001"]["eligible_for_stable"])
        annotations["manually_reviewed_sample_ids"] = [str(i) for i in range(100)]
        for label in annotations["labels"][:6]:
            label["label"] = "uncertain"
        result = summarize(report, "run", [annotations])["rules"]["KND001"]
        self.assertEqual(result["holdout"]["precision"], 1)
        self.assertFalse(result["eligible_for_stable"])


if __name__ == "__main__":
    unittest.main()
