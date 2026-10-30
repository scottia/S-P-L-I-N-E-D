"""Start the browser server."""

import os
import sys
import tomllib
from pathlib import Path
import uvicorn
from webgui.server import create_app

for stream in (sys.stdout, sys.stderr):
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding="utf-8", errors="backslashreplace")

host = os.environ.get("SPLINED_WEB_HOST", "127.0.0.1")
if host not in {"127.0.0.1", "localhost", "::1"} and not os.environ.get(
    "SPLINED_WEB_TOKEN"
):
    raise SystemExit("Set SPLINED_WEB_TOKEN before binding WebGUI to a network address")

verbosity = "info"
config_file = Path(os.environ.get("SPLINED_CONFIG", "/config/config.toml"))
try:
    with config_file.open("rb") as handle:
        verbosity = str(tomllib.load(handle).get("verbosity", "info")).strip().lower()
except (OSError, tomllib.TOMLDecodeError):
    pass
server_log_level = {
    "debug": "debug",
    "info": "info",
    "warning": "warning",
    "error": "error",
    "none": "critical",
}.get(verbosity, "info")
uvicorn.run(
    create_app(),
    host=host,
    port=int(os.environ.get("SPLINED_WEB_PORT", "8080")),
    log_level=server_log_level,
    access_log=verbosity != "none",
)
