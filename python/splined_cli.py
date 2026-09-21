from __future__ import annotations

import io
import sys
from contextlib import redirect_stdout

import splined as core
import splined_scan as scan
from splined_oauth_validation import run_oauth_validation


def _print_help_with_oauth_validation() -> int:
    buffer = io.StringIO()
    with redirect_stdout(buffer):
        rc = scan.main()
    inserted = False
    for line in buffer.getvalue().splitlines():
        print(line)
        if "--fanarttv-credentials" in line and not inserted:
            core.help_row(
                "      --oauth-validation",
                "Test saved credential tokens using random provider lookups",
            )
            inserted = True
    if not inserted:
        print()
        print("API/OAuth:")
        core.help_row(
            "      --oauth-validation",
            "Test saved credential tokens using random provider lookups",
        )
    return rc


def main() -> int:
    args = sys.argv[1:]
    if "--oauth-validation" in args:
        remaining = [arg for arg in args if arg != "--oauth-validation"]
        if remaining:
            print("--oauth-validation does not accept additional command options.", file=sys.stderr)
            return 2
        try:
            config_file, cfg = core.load_config()
            return run_oauth_validation(config_file, cfg)
        except core.SplinedError as exc:
            print(str(exc), file=sys.stderr)
            return 2
        except KeyboardInterrupt:
            print("\nSPLINED interrupted.", file=sys.stderr)
            return 130

    if "-h" in args or "--help" in args:
        return _print_help_with_oauth_validation()

    return scan.main()


if __name__ == "__main__":
    raise SystemExit(main())
