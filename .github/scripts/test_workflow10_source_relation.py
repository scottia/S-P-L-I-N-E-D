"""Synthetic Git-history regressions for Workflow 10 source selection."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / ".github/scripts/resolve_workflow10_source_relation.py"
SPEC = importlib.util.spec_from_file_location("workflow10_relation", MODULE_PATH)
assert SPEC and SPEC.loader
RELATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELATION)


class Workflow10SourceRelationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        self.git("init", "-b", "main")
        self.git("config", "user.email", "workflow10@example.invalid")
        self.git("config", "user.name", "Workflow 10 Test")
        self.write("source.txt", "base\n")
        self.commit("base")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", *args],
            cwd=self.repo,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

    def write(self, name: str, value: str) -> None:
        (self.repo / name).write_text(value, encoding="utf-8")

    def commit(self, message: str, *, allow_empty: bool = False) -> str:
        self.git("add", "-A")
        args = ["commit", "-m", message]
        if allow_empty:
            args.append("--allow-empty")
        self.git(*args)
        return self.git("rev-parse", "HEAD")

    def classify(self, main: str, dev: str) -> str:
        previous = Path.cwd()
        try:
            os.chdir(self.repo)
            return RELATION.classify_source_relation(main, dev)
        finally:
            os.chdir(previous)

    def test_same_tree_accepts_promotion_only_commit(self) -> None:
        base = self.git("rev-parse", "HEAD")
        self.git("checkout", "-b", "dev")
        dev = self.commit("dev identity", allow_empty=True)
        self.assertIn("same tree", self.classify(base, dev))

    def test_direct_main_ancestor_accepts_dev_ahead(self) -> None:
        main = self.git("rev-parse", "HEAD")
        self.git("checkout", "-b", "dev")
        self.write("source.txt", "dev ahead\n")
        dev = self.commit("dev ahead")
        self.assertIn("directly advances", self.classify(main, dev))

    def test_content_preserving_promotion_accepts_later_dev(self) -> None:
        self.git("checkout", "-b", "dev")
        self.write("source.txt", "promoted\n")
        promoted_source = self.commit("approved source")
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "dev", "-m", "promote dev")
        main = self.git("rev-parse", "HEAD")
        self.git("checkout", "dev")
        self.write("later.txt", "reviewed later dev\n")
        dev = self.commit("later reviewed dev")
        relation = self.classify(main, dev)
        self.assertIn("content-preserving promoted source", relation)
        self.assertIn(promoted_source, relation)

    def test_true_divergence_fails_closed(self) -> None:
        self.git("checkout", "-b", "dev")
        self.write("dev.txt", "dev\n")
        dev = self.commit("dev change")
        self.git("checkout", "main")
        self.write("main.txt", "main\n")
        main = self.commit("main change")
        with self.assertRaisesRegex(ValueError, "without a safe"):
            self.classify(main, dev)

    def test_tree_changing_promotion_merge_fails_closed(self) -> None:
        self.git("checkout", "-b", "dev")
        self.write("source.txt", "candidate\n")
        self.commit("candidate")
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "--no-commit", "dev")
        self.write("promotion-only.txt", "unauthorized promotion change\n")
        main = self.commit("promotion with extra tree change")
        self.git("checkout", "dev")
        self.write("later.txt", "later\n")
        dev = self.commit("later dev")
        with self.assertRaisesRegex(ValueError, "without a safe"):
            self.classify(main, dev)


if __name__ == "__main__":
    unittest.main()
