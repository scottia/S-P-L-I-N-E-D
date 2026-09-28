#!/usr/bin/env python3
"""Operational SPLINED entrypoint with shared runtime policy installation."""

from __future__ import annotations

import splined_scan
from splined_ranking_policy import install as install_ranking_policy
from splined_tui_overlay_policy import install as install_tui_overlay_policy


install_ranking_policy(splined_scan.core, splined_scan)
install_tui_overlay_policy(splined_scan.core)


if __name__ == "__main__":
    raise SystemExit(splined_scan.main())
