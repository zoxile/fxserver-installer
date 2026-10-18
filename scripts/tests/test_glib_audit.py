import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("glib_audit", Path(__file__).parents[1] / "audit-glib-backport.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def finding(identifier, version="0.18.5"):
    return {"advisory": {"id": identifier, "title": "fixture"},
            "package": {"name": "glib", "version": version}}


class GlibAuditTests(unittest.TestCase):
    def test_only_exact_backported_finding_is_accepted(self):
        report = {"vulnerabilities": {"list": []}, "warnings": {"unsound": [finding(audit.PATCHED_ADVISORY)]}}
        self.assertEqual(audit.review(report), [])

    def test_future_advisories_block_the_backport(self):
        for key in ("vulnerabilities", "warnings"):
            report = {"vulnerabilities": {"list": []}, "warnings": {"unsound": []}}
            report[key]["list" if key == "vulnerabilities" else "unsound"].append(finding("RUSTSEC-future"))
            self.assertEqual(audit.review(report), ["RUSTSEC-future: fixture"])

    def test_other_versions_need_a_new_review(self):
        report = {"vulnerabilities": {"list": [finding(audit.PATCHED_ADVISORY, "0.18.4")]}}
        self.assertEqual(len(audit.review(report)), 1)


if __name__ == "__main__":
    unittest.main()
