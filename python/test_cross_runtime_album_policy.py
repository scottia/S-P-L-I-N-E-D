import json
from pathlib import Path
from types import SimpleNamespace
import unittest

import splined_compilation_authority_policy as policy


FIXTURE = json.loads(
    (Path(__file__).parents[1] / "fixtures" / "cross-runtime-album-policy.json")
    .read_text(encoding="utf-8")
)


class CrossRuntimeAlbumPolicyTests(unittest.TestCase):
    def test_album_output_target_matches_shared_fixture(self):
        for item in FIXTURE["albums"]:
            track = SimpleNamespace(
                album_mbid=item["album_mbid"],
                compilation=item["compilation"],
            )
            eligible = policy.manual_album_eligible([track])
            self.assertEqual(eligible, item["manual_compilation"], item["name"])
            self.assertEqual(
                "embedded-track" if eligible else "folder",
                item["output_target"],
                item["name"],
            )

    def test_automatic_release_policy_matches_shared_fixture(self):
        for item in FIXTURE["automatic_release_classes"]:
            release = {
                "release-group": {
                    "primary-type": item["primary"],
                    "secondary-types": item["secondary"],
                }
            }
            classified = policy._release_class(release)
            self.assertEqual(
                classified[0] if classified else None,
                item["expected"],
                item,
            )


if __name__ == "__main__":
    unittest.main()
