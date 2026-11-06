"""Report the permanent Windows Rust/Slint pull-request policy exception."""

from __future__ import annotations

from pathlib import Path
import argparse
import os


POLICY_OUTCOME = "Not run — permanent PR policy exception"


def render_summary(*, revision: str, run_url: str) -> str:
    lines = [
        "# Windows Rust/Slint — advisory",
        "",
        f"**Outcome:** {POLICY_OUTCOME}",
        f"**Tested revision:** `{revision or 'unknown'}`",
        f"**Logs:** [workflow run]({run_url})" if run_url else "**Logs:** unavailable",
        "",
        "Strict Windows Rust/Slint diagnostics are not executed during pull-request validation.",
        "This permanent repository policy applies to every pull request targeting `dev` or `main`.",
        "Strict formatting, Clippy, Windows regression tests, and the clean production build remain available in Workflow 01 release validation.",
        "",
        "No Windows runner, Rust toolchain, Cargo command, or Slint build was started by this compatibility report.",
        "",
    ]
    return "\n".join(lines)


def append_summary(summary: str, destination: str) -> None:
    if destination:
        with Path(destination).open("a", encoding="utf-8", newline="\n") as handle:
            handle.write(summary)
    else:
        print(summary)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", default=os.environ.get("TESTED_REVISION", ""))
    parser.add_argument("--run-url", default=os.environ.get("RUN_URL", ""))
    parser.add_argument(
        "--summary", default=os.environ.get("GITHUB_STEP_SUMMARY", "")
    )
    args = parser.parse_args()
    print(f"::notice title=Windows PR validation policy::{POLICY_OUTCOME}")
    try:
        append_summary(
            render_summary(revision=args.revision, run_url=args.run_url),
            args.summary,
        )
    except Exception as error:
        # The compatibility job is intentionally non-gating. A summary-write
        # problem must not be misrepresented as Windows validation execution.
        print(f"::warning title=Windows advisory reporting failed::{error}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
