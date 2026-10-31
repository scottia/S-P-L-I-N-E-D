"""Run the behavioral browser-JavaScript regressions through unittest discovery."""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import unittest


REPOSITORY = Path(__file__).resolve().parents[2]
NODE_SUITE = Path(__file__).with_suffix(".mjs")
NODE_TIMEOUT_SECONDS = 60


def _output(stdout: str | bytes | None, stderr: str | bytes | None) -> str:
    def text(value: str | bytes | None) -> str:
        if isinstance(value, bytes):
            return value.decode("utf-8", errors="replace")
        return value or ""

    return "\n".join(part for part in (text(stdout).strip(), text(stderr).strip()) if part)


def run_node_regressions(
    *,
    suite: Path = NODE_SUITE,
    node_locator=shutil.which,
    runner=subprocess.run,
    timeout: int = NODE_TIMEOUT_SECONDS,
) -> str:
    node = node_locator("node")
    if not node:
        raise AssertionError("Node.js is required to run test_artist_ui_regressions.mjs")
    command = [node, "--test", str(suite)]
    try:
        result = runner(
            command,
            cwd=REPOSITORY,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        details = _output(error.stdout, error.stderr)
        message = f"Node behavioral regressions timed out after {timeout} seconds"
        raise AssertionError(message + (f"\n{details}" if details else "")) from error

    details = _output(result.stdout, result.stderr)
    if result.returncode != 0:
        raise AssertionError(
            f"Node behavioral regressions failed with exit code {result.returncode}"
            + (f"\n{details}" if details else "\nNode produced no diagnostic output")
        )
    return details


class ArtistUiRegressionDiscoveryTests(unittest.TestCase):
    def test_node_behavioral_regressions(self) -> None:
        output = run_node_regressions()
        self.assertIn("fail 0", output)

    def test_nonzero_node_exit_propagates_with_output(self) -> None:
        def failed_runner(command, **_kwargs):
            return subprocess.CompletedProcess(
                command,
                1,
                stdout="TAP version 13\nnot ok 1 - isolated fixture",
                stderr="fixture stderr",
            )

        with self.assertRaises(AssertionError) as failure:
            run_node_regressions(node_locator=lambda _name: "node", runner=failed_runner)
        self.assertIn("exit code 1", str(failure.exception))
        self.assertIn("not ok 1 - isolated fixture", str(failure.exception))
        self.assertIn("fixture stderr", str(failure.exception))

    def test_missing_node_fails_clearly(self) -> None:
        with self.assertRaisesRegex(AssertionError, "Node.js is required"):
            run_node_regressions(node_locator=lambda _name: None)

    def test_node_timeout_fails_with_partial_output(self) -> None:
        def timed_out_runner(command, **kwargs):
            raise subprocess.TimeoutExpired(
                command,
                kwargs["timeout"],
                output="TAP version 13\n# partial fixture output",
                stderr="fixture timeout",
            )

        with self.assertRaises(AssertionError) as failure:
            run_node_regressions(node_locator=lambda _name: "node", runner=timed_out_runner)
        self.assertIn("timed out after 60 seconds", str(failure.exception))
        self.assertIn("partial fixture output", str(failure.exception))
        self.assertIn("fixture timeout", str(failure.exception))


if __name__ == "__main__":
    unittest.main()
