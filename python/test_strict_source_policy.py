from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

from PIL import Image, ImageDraw

import splined
import splined_strict_source_policy as strict


def make_cover(path: Path, size: tuple[int, int] = (600, 600)) -> None:
    image = Image.new("RGB", size, "#e8d2ad")
    draw = ImageDraw.Draw(image)
    draw.rectangle((30, 35, size[0] - 30, size[1] - 35), outline="#171717", width=9)
    draw.ellipse((90, 110, size[0] - 90, size[1] - 130), fill="#b2183b")
    draw.polygon(((120, 480), (300, 190), (480, 480)), fill="#162c55")
    draw.text((175, 55), "SPLINED", fill="#111111")
    image.save(path, quality=95)


def candidate(path: Path, source: str, *, front: bool = True, types=None):
    with Image.open(path) as image:
        width, height = image.size
    return SimpleNamespace(
        source=source,
        path=path,
        width=width,
        height=height,
        source_priority=0,
        ref=SimpleNamespace(
            front=front,
            approved=True,
            types=list(types or (["Front"] if front else [])),
            id=path.name,
        ),
    )


def policy(_cfg, source: str):
    return {"strict_override": source.casefold() == "amazon"}


class StrictSourcePolicyTests(unittest.TestCase):
    def test_strict_override_uses_global_range_not_source_override(self) -> None:
        config = {
            "range": {"min": 1200, "ideal": 1800, "max": 2400, "ladder": 3600},
            "source_policies": {
                "amazon": {
                    "enabled": True,
                    "strict_override": True,
                    "source_override": True,
                    "minimum_range_type": "Ideal",
                    "minimum_short_side": 2000,
                }
            },
        }
        self.assertEqual(
            splined.source_policy_decision(config, "amazon", 1200, 1200)[0],
            "accept",
        )

    def test_resized_same_front_is_preferred_and_auto_eligible(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            caa_path = root / "caa.jpg"
            amazon_path = root / "amazon.jpg"
            make_cover(caa_path)
            with Image.open(caa_path) as image:
                image.resize((1200, 1200), Image.Resampling.LANCZOS).save(
                    amazon_path,
                    quality=90,
                )
            caa = candidate(caa_path, "coverartarchive")
            amazon = candidate(amazon_path, "amazon")

            strict.apply([caa, amazon], {}, policy)

            self.assertEqual(amazon.strict_status, "validated-front")
            self.assertTrue(amazon.strict_preferred_eligible)
            self.assertTrue(amazon.strict_auto_eligible)

    def test_product_photo_containing_cover_stays_manual_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            caa_path = root / "caa.jpg"
            product_path = root / "product.jpg"
            make_cover(caa_path)
            product = Image.new("RGB", (800, 600), "#9b948a")
            with Image.open(caa_path) as cover:
                product.paste(cover.resize((360, 360)), (220, 120))
            product.save(product_path, quality=92)
            caa = candidate(caa_path, "coverartarchive")
            amazon = candidate(product_path, "amazon")

            strict.apply([caa, amazon], {}, policy)

            self.assertEqual(amazon.strict_status, "unverified")
            self.assertFalse(amazon.strict_preferred_eligible)
            self.assertFalse(amazon.strict_auto_eligible)

    def test_non_front_exact_release_match_is_not_front_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            disc_path = root / "disc.jpg"
            amazon_path = root / "amazon.jpg"
            make_cover(disc_path)
            with Image.open(disc_path) as image:
                image.save(amazon_path, quality=90)
            disc = candidate(
                disc_path,
                "coverartarchive",
                front=False,
                types=["Medium"],
            )
            amazon = candidate(amazon_path, "amazon")

            strict.apply([disc, amazon], {}, policy)

            self.assertEqual(amazon.strict_status, "wrong-type")
            self.assertFalse(amazon.strict_preferred_eligible)

    def test_matching_local_becomes_a_validated_content_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            caa_path = root / "caa.jpg"
            local_path = root / "cover.jpg"
            amazon_path = root / "amazon.jpg"
            make_cover(caa_path)
            with Image.open(caa_path) as image:
                image.save(local_path, quality=93)
                image.resize((1200, 1200), Image.Resampling.LANCZOS).save(
                    amazon_path,
                    quality=90,
                )
            caa = candidate(caa_path, "coverartarchive")
            local = candidate(local_path, "local")
            amazon = candidate(amazon_path, "amazon")

            strict.apply([local, caa, amazon], {}, policy)

            self.assertTrue(local.strict_local_validated)
            self.assertTrue(amazon.strict_preferred_eligible)


if __name__ == "__main__":
    unittest.main()

