"""Start the browser server; no terminal interface is installed."""

import os
import sys
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
uvicorn.run(
    create_app(),
    host=host,
    port=int(os.environ.get("SPLINED_WEB_PORT", "8080")),
    log_level="info",
)
