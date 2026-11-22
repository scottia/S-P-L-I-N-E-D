"""Regression checks for exact dev-to-main promotion validation."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
VALIDATE = ROOT / ".github/workflows/validate.yml"
PROMOTION = (
    "${{ github.event_name == 'pull_request' && github.base_ref == 'main' "
    "&& github.head_ref == 'dev' }}"
)
NON_PROMOTION = (
    "github.event_name != 'pull_request' || github.base_ref != 'main' || "
    "github.head_ref != 'dev'"
)


def job(source: str, name: str) -> str:
    match = re.search(r"(?m)^  " + re.escape(name) + r":\s*$", source)
    if not match:
        raise AssertionError(f"Missing job: {name}")
    following = re.search(r"(?m)^  [a-z][a-z0-9-]*:\s*$", source[match.end():])
    end = match.end() + following.start() if following else len(source)
    return source[match.start():end]


class PromotionValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = VALIDATE.read_text(encoding="utf-8")

    def test_exact_promotion_uses_one_provenance_job(self) -> None:
        promotion = job(self.workflow, "promotion-provenance")
        self.assertIn(PROMOTION, promotion)
        self.assertIn("github.event.pull_request.head.repo.full_name", promotion)
        self.assertIn('test "$(git rev-parse origin/dev)" = "$HEAD_SHA"', promotion)
        self.assertIn('git merge-base --is-ancestor "$BASE_SHA" "$HEAD_SHA"', promotion)

    def test_exact_promotion_does_not_repeat_source_suites(self) -> None:
        for name in (
            "audio-integrity-webgui",
            "audio-integrity-windows",
            "scope",
            "policy-review",
            "webgui",
            "windows-advisory",
        ):
            with self.subTest(job=name):
                self.assertIn(NON_PROMOTION, job(self.workflow, name))

    def test_feature_prs_and_workflow_dispatch_keep_full_validation(self) -> None:
        self.assertIn("workflow_dispatch:", self.workflow)
        self.assertIn("github.event_name != 'pull_request'", self.workflow)
        self.assertIn("Audio integrity — WebGUI real artwork writer", self.workflow)
        self.assertIn("Audio integrity — Windows real artwork writer", self.workflow)
        self.assertIn("Canonical policy JSON — review gate", self.workflow)


if __name__ == "__main__":
    unittest.main()
