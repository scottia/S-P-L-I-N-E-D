"""Run and summarize non-gating Windows Rust/Slint PR diagnostics."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import os
import re
import subprocess
from typing import Callable, Sequence


@dataclass(frozen=True)
class CommandSpec:
    key: str
    label: str
    command: tuple[str, ...]
    prerequisites: tuple[str, ...] = ()
    nonzero_outcome: str = "Failed"


@dataclass(frozen=True)
class CommandResult:
    spec: CommandSpec
    outcome: str
    exit_code: int | None
    output: str
    executed: bool
    warnings: int = 0


COMMANDS = (
    CommandSpec(
        "toolchain",
        "Rust toolchain",
        ("rustup", "toolchain", "install", "1.99.0", "--profile", "minimal", "--no-self-update"),
    ),
    CommandSpec(
        "components",
        "Rustfmt and Clippy components",
        ("rustup", "component", "add", "rustfmt", "clippy", "--toolchain", "1.99.0"),
        ("toolchain",),
    ),
    CommandSpec(
        "formatting",
        "Formatting",
        ("cargo", "+1.99.0", "fmt", "--manifest-path", "windows/Cargo.toml", "--", "--check"),
        ("components",),
        "Warnings",
    ),
    CommandSpec(
        "rust_slint",
        "Rust/Slint diagnostics",
        (
            "cargo",
            "+1.99.0",
            "check",
            "--locked",
            "--manifest-path",
            "windows/Cargo.toml",
            "--all-targets",
            "--target",
            "x86_64-pc-windows-msvc",
        ),
        ("toolchain",),
    ),
    CommandSpec(
        "clippy",
        "Clippy",
        (
            "cargo",
            "+1.99.0",
            "clippy",
            "--locked",
            "--manifest-path",
            "windows/Cargo.toml",
            "--all-targets",
            "--target",
            "x86_64-pc-windows-msvc",
        ),
        ("components",),
    ),
)

WARNING_LINE = re.compile(r"(?im)^\s*warning(?:\[[^\]]+\])?:")
ERROR_LINE = re.compile(r"(?im)^\s*error(?:\[[^\]]+\])?:")


def command_text(command: Sequence[str]) -> str:
    return subprocess.list2cmdline(command)


def classify(spec: CommandSpec, exit_code: int, output: str) -> tuple[str, int]:
    warnings = len(WARNING_LINE.findall(output))
    if exit_code:
        if spec.key == "formatting" and ERROR_LINE.search(output):
            return "Failed", warnings
        return spec.nonzero_outcome, warnings
    if warnings:
        return "Warnings", warnings
    return "Passed", 0


def not_run(spec: CommandSpec, reason: str) -> CommandResult:
    return CommandResult(spec, "Not run", None, reason, False)


def execute(
    spec: CommandSpec,
    *,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
    repository: Path | None = None,
) -> CommandResult:
    print(f"::group::{spec.label}")
    print(f"$ {command_text(spec.command)}")
    try:
        completed = runner(
            list(spec.command),
            cwd=repository,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        output = "\n".join(
            part.rstrip() for part in (completed.stdout, completed.stderr) if part
        )
        if output:
            print(output)
        outcome, warnings = classify(spec, completed.returncode, output)
        result = CommandResult(
            spec, outcome, completed.returncode, output, True, warnings
        )
    except OSError as error:
        print(str(error))
        result = CommandResult(spec, "Failed", None, str(error), True)
    finally:
        print("::endgroup::")
    return result


def run_advisory(
    *,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
    repository: Path | None = None,
) -> list[CommandResult]:
    results: list[CommandResult] = []
    by_key: dict[str, CommandResult] = {}
    for spec in COMMANDS:
        unavailable = [
            key
            for key in spec.prerequisites
            if by_key[key].outcome not in {"Passed", "Warnings"}
        ]
        if unavailable:
            result = not_run(
                spec,
                "Prerequisite failed: " + ", ".join(unavailable),
            )
        else:
            result = execute(spec, runner=runner, repository=repository)
        results.append(result)
        by_key[spec.key] = result
    return results


def overall_outcome(results: Sequence[CommandResult]) -> str:
    outcomes = {result.outcome for result in results}
    if "Failed" in outcomes:
        return "Failed"
    if "Warnings" in outcomes:
        return "Warnings"
    if "Passed" in outcomes:
        return "Passed"
    return "Not run"


def render_summary(
    results: Sequence[CommandResult], *, revision: str, run_url: str
) -> str:
    overall = overall_outcome(results)
    lines = [
        "# Windows Rust/Slint — advisory",
        "",
        f"**Outcome:** {overall}",
        f"**Tested revision:** `{revision or 'unknown'}`",
        f"**Logs:** [workflow run]({run_url})" if run_url else "**Logs:** unavailable",
        "",
        "This report is diagnostic only. It does not approve, modify, or merge source.",
        "",
        "| Diagnostic | Outcome | Executed | Exit result | Command | Logs |",
        "| --- | --- | --- | ---: | --- | --- |",
    ]
    for result in results:
        command = command_text(result.spec.command).replace("|", "\\|")
        exit_result = "—" if result.exit_code is None else str(result.exit_code)
        log_link = f"[run]({run_url})" if run_url and result.executed else "—"
        lines.append(
            f"| {result.spec.label} | **{result.outcome}** | "
            f"{'Yes' if result.executed else 'No'} | {exit_result} | `{command}` | {log_link} |"
        )
    lines.extend(
        [
            "",
            "`Warnings` and `Failed` are tolerated by this advisory job, but are not reported as passed.",
            "Commands marked `Not run` were listed for traceability and were not executed.",
            "",
        ]
    )
    return "\n".join(lines)


def report_annotations(results: Sequence[CommandResult]) -> None:
    for result in results:
        if result.outcome in {"Warnings", "Failed"}:
            detail = (
                f"exit {result.exit_code}"
                if result.exit_code is not None
                else "command unavailable"
            )
            if result.warnings:
                detail += f", {result.warnings} warning line(s)"
            print(
                f"::warning title=Windows advisory {result.spec.label}::"
                f"{result.outcome}: {detail}; inspect the workflow log"
            )


def append_summary(summary: str, destination: str) -> None:
    if destination:
        with Path(destination).open("a", encoding="utf-8", newline="\n") as handle:
            handle.write(summary)
    else:
        print(summary)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--not-run", action="store_true")
    parser.add_argument("--revision", default=os.environ.get("TESTED_REVISION", ""))
    parser.add_argument("--run-url", default=os.environ.get("RUN_URL", ""))
    parser.add_argument(
        "--summary", default=os.environ.get("GITHUB_STEP_SUMMARY", "")
    )
    args = parser.parse_args()
    try:
        results = (
            [not_run(spec, "Change scope does not require Windows diagnostics") for spec in COMMANDS]
            if args.not_run
            else run_advisory(repository=Path.cwd())
        )
        report_annotations(results)
        append_summary(
            render_summary(results, revision=args.revision, run_url=args.run_url),
            args.summary,
        )
    except Exception as error:  # The reporting job itself remains advisory.
        print(f"::warning title=Windows advisory reporting failed::{error}")
        fallback = (
            "# Windows Rust/Slint — advisory\n\n"
            "**Outcome:** Failed\n\n"
            f"**Tested revision:** `{args.revision or 'unknown'}`\n\n"
            f"The advisory reporter failed: `{error}`. Inspect the workflow logs.\n"
        )
        append_summary(fallback, args.summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
