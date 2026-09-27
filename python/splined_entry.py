#!/usr/bin/env python3
"""Operational SPLINED entrypoint with shared runtime policy installation."""

from __future__ import annotations

import splined_scan
from splined_ranking_policy import install


install(splined_scan.core, splined_scan)


if __name__ == "__main__":
    raise SystemExit(splined_scan.main())
