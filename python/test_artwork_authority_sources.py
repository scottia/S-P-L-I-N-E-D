from __future__ import annotations

import unittest

import splined
from tui.source_settings import PolicyDraft


class _Response:
    def __init__(self, *, status_code: int = 200, text: str = "", payload=None):
        self.status_code = status_code
        self.text = text
        self._payload = payload if payload is not None else {}

    def json(self):
        return self._payload


class _Http:
    def __init__(self, response: _Response):
        self.response = response
        self.calls: list[tuple[str, dict]] = []

    def get(self, url: str, **kwargs):
        self.calls.append((url, kwargs))
        return self.response


def _release(**values) -> splined.Release:
    defaults = {
        "mbid": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "title": "A Summer Place",
        "artist_credit": "Percy Faith",
        "release_group_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        "release_group_title": "A Summer Place",
        "track_count": 12,
    }
    defaults.update(values)
    return splined.Release(**defaults)


class ArtworkAuthoritySourceTests(unittest.TestCase):
    def test_amazon_original_url_removes_only_image_transform(self) -> None:
        self.assertEqual(
            splined.amazon_original_image_url(
                "https://m.media-amazon.com/images/I/81Y+xtQACkL._AC_UY218_.jpg"
            ),
            "https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg",
        )
        self.assertEqual(
            splined.amazon_original_image_url(
                "https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg"
            ),
            "https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg",
        )
        self.assertIsNone(
            splined.amazon_original_image_url(
                "https://example.invalid/images/I/81Y+xtQACkL._AC_UY218_.jpg"
            )
        )

    def test_amazon_store_search_returns_previewable_original_url(self) -> None:
        body = """
        <div data-component-type="s-search-result" data-asin="B000000001">
          <h2><span>A Summer Place</span></h2>
          <img class="s-image" src="https://m.media-amazon.com/images/I/81Y+xtQACkL._AC_UY218_.jpg">
        </div>
        """
        http = _Http(_Response(text=body))
        refs = splined.discover_amazon(http, _release())
        self.assertEqual(len(refs), 1)
        self.assertEqual(refs[0].source, "amazon")
        self.assertEqual(
            refs[0].url,
            "https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg",
        )
        self.assertTrue(refs[0].front)
        self.assertEqual(http.calls[0][0], "https://www.amazon.com/s")

    def test_musicbrainz_priority_uses_release_group_caa_artwork(self) -> None:
        http = _Http(
            _Response(
                payload={
                    "images": [
                        {
                            "id": 17,
                            "image": "https://coverartarchive.org/release/x/front.jpg",
                            "front": True,
                            "approved": True,
                            "types": ["Front"],
                        }
                    ]
                }
            )
        )
        refs = splined.discover_musicbrainz_artwork(http, _release())
        self.assertEqual(len(refs), 1)
        self.assertEqual(refs[0].source, "musicbrainz")
        self.assertEqual(
            http.calls[0][0],
            "https://coverartarchive.org/release-group/"
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/",
        )

    def test_policy_editor_exposes_mb_and_opt_in_amazon(self) -> None:
        draft = PolicyDraft.from_config({"sources": {"cover_sources": []}})
        self.assertIn("musicbrainz", draft.source_order)
        self.assertIn("amazon", draft.source_order)
        self.assertTrue(draft.policies["musicbrainz"]["enabled"])
        self.assertFalse(draft.policies["amazon"]["enabled"])
        self.assertEqual(
            draft.policies["musicbrainz"]["minimum_range_type"],
            "LowerRange",
        )


if __name__ == "__main__":
    unittest.main()
