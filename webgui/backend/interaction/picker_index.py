"""Library identity models and configured directory exclusions."""
from dataclasses import dataclass
import re
from typing import Iterable


def should_ignore(name: str, patterns: Iterable[str]) -> bool:
    """Apply configured wildcard rules without hiding valid dot-prefixed music."""
    for pattern in patterns:
        value = str(pattern).strip()
        if not value:
            continue
        expression = "^" + re.escape(value).replace(r"\*", ".*").replace(r"\?", ".") + "$"
        if re.fullmatch(expression, name, flags=re.IGNORECASE):
            return True
    return False

@dataclass(frozen=True)
class PickerArtist:
    path: str
    name: str
    # Kept for the presentation/model API. Every artist exposed from a
    # complete snapshot is indexed and loaded.
    indexed: bool = True
    last_indexed: float | None = None

@dataclass(frozen=True)
class PickerAlbum:
    path: str
    artist_path: str
    name: str
    local_art: tuple[str, ...] = ()
    inventory_fingerprint: str | None = None
