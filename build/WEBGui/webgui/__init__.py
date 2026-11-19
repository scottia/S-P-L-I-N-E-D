"""SPLINED browser application and Python processing backend."""

# Keep the established processing modules importable under their existing
# module identities while the presentation is a separate, importable package.
from pathlib import Path
import sys

_backend = str(Path(__file__).with_name("backend"))
if _backend not in sys.path:
    sys.path.insert(0, _backend)
