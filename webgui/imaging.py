"""The same bounded pixel pipeline serves browser previews and saved artwork."""

from __future__ import annotations

from io import BytesIO
import math

from PIL import Image, ImageEnhance, ImageFilter, ImageOps


def adjusted(source, values=None):
    values = values or {}

    def value(name):
        result = float(values.get(name, 0) or 0)
        if not math.isfinite(result) or not -100 <= result <= 100:
            raise ValueError("Artwork adjustment is outside its allowed range")
        return result

    with Image.open(BytesIO(source) if isinstance(source, bytes) else source) as opened:
        if opened.width * opened.height > 64 * 1024 * 1024:
            raise ValueError("Artwork exceeds the pixel limit")
        image = ImageOps.exif_transpose(opened).convert("RGBA")
    alpha = image.getchannel("A")
    rgb = image.convert("RGB")
    brightness = max(
        0.35,
        1
        + (
            value("Low/Dark Light")
            + value("Exposure")
            + value("Brightness")
            + value("Default") * 0.4
            + value("Gamma") * 0.35
        )
        / 220,
    )
    contrast = max(
        0.35,
        1
        + (
            value("Clarity")
            + value("High/Low Contrast")
            + value("Picture") * 0.4
            + value("Custom A") * 0.3
        )
        / 190,
    )
    saturation = max(
        0,
        1
        + (
            value("Vivid Color")
            + value("Portrait Mode") * 0.25
            + value("Custom B") * 0.4
        )
        / 120,
    )
    rgb = ImageEnhance.Brightness(rgb).enhance(brightness)
    rgb = ImageEnhance.Contrast(rgb).enhance(contrast)
    rgb = ImageEnhance.Color(rgb).enhance(saturation)
    hue = value("Hue") * 1.8 + value("Warm/Cool Color") * 0.2 + value("Custom C") * 0.5
    if hue:
        h, s, v = rgb.convert("HSV").split()
        h = h.point([(n + round(hue / 360 * 255)) % 256 for n in range(256)])
        rgb = Image.merge("HSV", (h, s, v)).convert("RGB")
    sepia = max(
        0, min(0.8, abs(value("Tint")) / 140 + max(0, value("Warm/Cool Color")) / 450)
    )
    if sepia:
        toned = rgb.convert(
            "RGB",
            (0.393, 0.769, 0.189, 0, 0.349, 0.686, 0.168, 0, 0.272, 0.534, 0.131, 0),
        )
        rgb = Image.blend(rgb, toned, sepia)
    blur = max(0, (value("Blur Background") + max(0, -value("Soft/Sharper"))) / 32)
    if blur:
        rgb = rgb.filter(ImageFilter.GaussianBlur(blur))
    sharpness = value("Soft/Sharper")
    if sharpness > 0:
        rgb = ImageEnhance.Sharpness(rgb).enhance(1 + sharpness / 30)
    image = rgb.convert("RGBA")
    image.putalpha(alpha)
    return image


def framed(image, kind, fit=None):
    if kind not in {"artist", "banner"}:
        return image
    fit = fit or {}
    zoom = float(fit.get("zoom", 1))
    x = float(fit.get("x", 0))
    y = float(fit.get("y", 0))
    if (
        not all(map(math.isfinite, (zoom, x, y)))
        or not 0.5 <= zoom <= 8
        or not -100 <= x <= 100
        or not -100 <= y <= 100
    ):
        raise ValueError("Fitment is outside its allowed range")
    width = min(image.width, 4096) if kind == "banner" else min(image.size)
    height = round(width * 600 / 1400) if kind == "banner" else width
    scale = max(width / image.width, height / image.height) * zoom
    if round(image.width * scale) * round(image.height * scale) > 64 * 1024 * 1024:
        raise ValueError("Fitment exceeds the pixel limit")
    rendered = image.resize(
        (max(1, round(image.width * scale)), max(1, round(image.height * scale))),
        Image.Resampling.LANCZOS,
    )
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    canvas.paste(
        rendered,
        (
            round((width - rendered.width) / 2 + width * x / 100),
            round((height - rendered.height) / 2 + height * y / 100),
        ),
    )
    return canvas


def upscale_to_policy(image, ideal=1800, max_percent=200):
    """Enlarge proportionally toward Ideal, bounded by Config v5 percent."""
    ideal = int(ideal)
    max_percent = int(max_percent)
    if ideal < 1 or not 100 <= max_percent <= 800:
        raise ValueError("Upscale settings are outside their allowed range")
    width, height = image.size
    short_side = min(width, height)
    target = min(ideal, short_side * max_percent // 100)
    if short_side < 1 or target <= short_side:
        return image, False, short_side
    scale = target / short_side
    output_size = (max(1, round(width * scale)), max(1, round(height * scale)))
    if output_size[0] * output_size[1] > 64 * 1024 * 1024:
        raise ValueError("Upscaled artwork exceeds the pixel limit")
    return image.resize(output_size, Image.Resampling.LANCZOS), True, target


def prepare_embedded_thumbnail(image, side):
    """Match Windows COMP squaring: crop near-square art, contain wider art."""
    side = int(side)
    if not 300 <= side <= 4000:
        raise ValueError("Embedded thumbnail size must be between 300 and 4000")
    source = ImageOps.exif_transpose(image).convert("RGBA")
    opaque = Image.new("RGB", source.size, (0, 0, 0))
    opaque.paste(source, mask=source.getchannel("A"))
    deviation = abs(opaque.width - opaque.height) / max(opaque.size)
    if deviation <= 0.02:
        return ImageOps.fit(
            opaque,
            (side, side),
            method=Image.Resampling.LANCZOS,
        )
    fitted = ImageOps.contain(
        opaque,
        (side, side),
        method=Image.Resampling.LANCZOS,
    )
    canvas = Image.new("RGB", (side, side), (0, 0, 0))
    canvas.paste(
        fitted,
        ((side - fitted.width) // 2, (side - fitted.height) // 2),
    )
    return canvas


def render(
    source,
    *,
    values=None,
    package_values=None,
    fit=None,
    kind="cover",
    upscale=False,
    upscale_ideal=1800,
    upscale_max_percent=200,
):
    image = framed(adjusted(source, values), kind, fit)
    if package_values:
        image = adjusted_bytes(image, package_values)
    if upscale:
        image, _, _ = upscale_to_policy(image, upscale_ideal, upscale_max_percent)
    return image


def adjusted_bytes(image, values):
    body = BytesIO()
    image.save(body, format="PNG")
    return adjusted(body.getvalue(), values)


def encode(image, format="png"):
    output = BytesIO()
    if format in {"jpeg", "jpg"}:
        canvas = Image.new("RGB", image.size, (0, 0, 0))
        if image.mode == "RGBA":
            canvas.paste(image, mask=image.getchannel("A"))
        else:
            canvas.paste(image)
        canvas.save(output, format="JPEG", quality=95)
    else:
        image.save(output, format="PNG")
    return output.getvalue()
