from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import splined


class FakeResponse:
    def __init__(
        self,
        chunks: list[bytes],
        *,
        content_length: int | None = None,
        status_code: int = 200,
    ) -> None:
        self.chunks = chunks
        self.headers = {}
        if content_length is not None:
            self.headers["Content-Length"] = str(content_length)
        self.status_code = status_code
        self.iterated = False
        self.closed = False

    def __enter__(self) -> "FakeResponse":
        return self

    def __exit__(self, _type, _value, _traceback) -> None:
        self.closed = True

    def iter_content(self, chunk_size: int):
        self.iterated = True
        if chunk_size != splined.DOWNLOAD_CHUNK_BYTES:
            raise AssertionError(f"unexpected chunk size: {chunk_size}")
        yield from self.chunks


class FakeHttp:
    def __init__(self, response: FakeResponse) -> None:
        self.response = response
        self.calls: list[tuple[str, dict]] = []

    def get(self, url: str, **kwargs):
        self.calls.append((url, kwargs))
        return self.response


class FakeImage:
    format = "PNG"
    size = (8, 8)

    def __enter__(self) -> "FakeImage":
        return self

    def __exit__(self, _type, _value, _traceback) -> None:
        return None

    def verify(self) -> None:
        return None


class OversizedFakeImage(FakeImage):
    size = (8192, 8193)


class BoundedArtworkDownloadTests(unittest.TestCase):
    def test_valid_response_below_limit_is_streamed_and_closed(self) -> None:
        payload = b"synthetic-valid-image"
        response = FakeResponse([payload], content_length=len(payload))
        http = FakeHttp(response)
        reference = splined.Ref("deezer", "album-1", "https://example.invalid/art.png")

        with tempfile.TemporaryDirectory() as directory:
            with patch.object(splined.Image, "open", return_value=FakeImage()):
                candidates, diagnostics = splined.download_candidates(
                    http,
                    [reference],
                    ["deezer"],
                    Path(directory),
                )

            self.assertEqual(diagnostics, [])
            self.assertEqual(len(candidates), 1)
            self.assertEqual(candidates[0].path.read_bytes(), payload)

        self.assertEqual(
            http.calls,
            [(reference.url, {"allow_redirects": True, "stream": True})],
        )
        self.assertTrue(response.iterated)
        self.assertTrue(response.closed)

    def test_exactly_25_mib_is_allowed(self) -> None:
        payload = b"x" * splined.MAX_DOWNLOAD_BYTES
        response = FakeResponse([payload], content_length=len(payload))
        self.assertEqual(splined.read_bounded_artwork_response(response), payload)

    def test_content_length_above_limit_is_rejected_before_reading(self) -> None:
        response = FakeResponse(
            [b"must-not-be-read"],
            content_length=splined.MAX_DOWNLOAD_BYTES + 1,
        )
        with self.assertRaisesRegex(splined.SplinedError, "25 MiB"):
            splined.read_bounded_artwork_response(response)
        self.assertFalse(response.iterated)

    def test_chunked_response_below_limit_is_allowed(self) -> None:
        response = FakeResponse([b"one", b"", b"two"])
        self.assertEqual(
            splined.read_bounded_artwork_response(response),
            b"onetwo",
        )

    def test_chunked_response_above_limit_is_rejected(self) -> None:
        response = FakeResponse(
            [b"x" * splined.MAX_DOWNLOAD_BYTES, b"overflow"]
        )
        with self.assertRaisesRegex(splined.SplinedError, "25 MiB"):
            splined.read_bounded_artwork_response(response)

    def test_empty_response_is_rejected(self) -> None:
        response = FakeResponse([b"", b""])
        with self.assertRaisesRegex(splined.SplinedError, "empty"):
            splined.read_bounded_artwork_response(response)

    def test_invalid_image_payload_is_reported_and_response_is_closed(self) -> None:
        response = FakeResponse([b"not-an-image"])
        http = FakeHttp(response)
        reference = splined.Ref("deezer", "album-2", "https://example.invalid/bad")

        with tempfile.TemporaryDirectory() as directory:
            with patch.object(splined.Image, "open", side_effect=ValueError("invalid image")):
                candidates, diagnostics = splined.download_candidates(
                    http,
                    [reference],
                    ["deezer"],
                    Path(directory),
                )

        self.assertEqual(candidates, [])
        self.assertEqual(len(diagnostics), 1)
        self.assertIn("invalid image", diagnostics[0][1])
        self.assertTrue(response.closed)

    def test_oversized_decoded_image_is_rejected_before_cache_write(self) -> None:
        response = FakeResponse([b"small-compressed-payload"])
        http = FakeHttp(response)
        reference = splined.Ref("deezer", "album-3", "https://example.invalid/bomb.png")

        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            with patch.object(splined.Image, "open", return_value=OversizedFakeImage()):
                candidates, diagnostics = splined.download_candidates(
                    http,
                    [reference],
                    ["deezer"],
                    cache,
                )

            self.assertEqual(candidates, [])
            self.assertEqual(list(cache.iterdir()), [])

        self.assertEqual(len(diagnostics), 1)
        self.assertIn("decoded-image limit", diagnostics[0][1])
        self.assertTrue(response.closed)


if __name__ == "__main__":
    unittest.main()
