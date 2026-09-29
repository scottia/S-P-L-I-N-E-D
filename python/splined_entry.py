#!/usr/bin/env python3
"""Operational SPLINED entrypoint with shared runtime policy installation."""

from __future__ import annotations

import splined_scan
from splined_media_build_policy import install as install_media_build_policy
from splined_media_fast_index_policy import install as install_media_fast_index_policy
from splined_media_finalize_policy import install as install_media_finalize_policy
from splined_media_index import install as install_media_index
from splined_media_load_policy import install as install_media_load_policy
from splined_media_warm_start_policy import install as install_media_warm_start_policy
from splined_ranking_policy import install as install_ranking_policy
from splined_tui_overlay_policy import install as install_tui_overlay_policy
from splined_tui_progress_policy import install as install_tui_progress_policy


install_ranking_policy(splined_scan.core, splined_scan)
install_tui_overlay_policy(splined_scan.core)
install_tui_progress_policy()
install_media_build_policy(splined_scan.core)
install_media_fast_index_policy(splined_scan.core)
install_media_load_policy()
install_media_warm_start_policy()
install_media_finalize_policy(splined_scan.core)
install_media_index(splined_scan.core, splined_scan)


if __name__ == "__main__":
    raise SystemExit(splined_scan.main())
