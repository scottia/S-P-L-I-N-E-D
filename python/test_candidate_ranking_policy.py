from __future__ import annotations

import unittest
from dataclasses import dataclass
from types import SimpleNamespace

from splined_ranking_policy import (
    core_candidate_key,
    install,
    operational_candidate_key,
    operational_fallback_suggested,
)


@dataclass
class Ref:
    id: str
    approved: bool = True


@dataclass
class Candidate:
    source: str
    width: int
    height: int
    source_priority: int
    ref: Ref

    @property
    def short_side(self) -> int:
        return min(self.width, self.height)


def projected(candidate: Candidate, _cfg, _format_order):
    # Reproduce the reported production case: both sources project to the same
    # 3000x3000 Ideal result, but Cover Art Archive starts at 3024x3024 and is
    # benignly downscaled while iTunes starts at exactly 3000x3000.
    return {
        "policy_status": "accept",
        "acceptable": True,
        "distance": 0,
        "range_type": "Ideal",
        "upscaled": False,
        "cropped": False,
        "resized": candidate.width != 3000,
        "converted": False,
        "square": True,
        "short_side": 3000,
        "format": "jpeg",
    }


class IntrinsicResolutionRankingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.itunes = Candidate("itunes", 3000, 3000, 0, Ref("itunes"))
        self.coverart = Candidate(
            "coverartarchive",
            3024,
            3024,
            2,
            Ref("coverart"),
        )
        self.scan = SimpleNamespace(
            project_candidate=projected,
            provider_label=lambda source: source,
        )

    def test_larger_equivalent_source_beats_provider_order(self) -> None:
        chosen = min(
            [self.itunes, self.coverart],
            key=lambda candidate: operational_candidate_key(
                self.scan,
                candidate,
                {},
                ["jpeg"],
            ),
        )
        self.assertIs(chosen, self.coverart)

    def test_fallback_suggestion_uses_same_intrinsic_quality_tie_break(self) -> None:
        chosen = operational_fallback_suggested(
            self.scan,
            [self.itunes, self.coverart],
            {},
            ["jpeg"],
        )
        self.assertIs(chosen, self.coverart)

    def test_equal_intrinsic_sources_still_use_configured_provider_order(self) -> None:
        coverart_equal = Candidate(
            "coverartarchive",
            3000,
            3000,
            2,
            Ref("coverart-equal"),
        )
        chosen = min(
            [self.itunes, coverart_equal],
            key=lambda candidate: operational_candidate_key(
                self.scan,
                candidate,
                {},
                ["jpeg"],
            ),
        )
        self.assertIs(chosen, self.itunes)

    def test_core_and_operational_surfaces_install_one_policy(self) -> None:
        core = SimpleNamespace(
            project_candidate=lambda candidate, cfg, target: projected(
                candidate,
                cfg,
                [target],
            ),
            target_format_for_candidate=lambda _candidate, order: order[0],
            provider_label=lambda source: source,
            candidate_key=lambda *_args: (999,),
        )
        scan = SimpleNamespace(
            project_candidate=projected,
            provider_label=lambda source: source,
            candidate_key=lambda *_args: (999,),
            fallback_sort_key=lambda *_args: (999,),
            fallback_suggested=lambda *_args: None,
        )
        install(core, scan)
        self.assertLess(
            scan.candidate_key(self.coverart, {}, ["jpeg"]),
            scan.candidate_key(self.itunes, {}, ["jpeg"]),
        )
        self.assertLess(
            core_candidate_key(core, self.coverart, {}, ["jpeg"]),
            core_candidate_key(core, self.itunes, {}, ["jpeg"]),
        )


if __name__ == "__main__":
    unittest.main()
