"""Thread-scoped browser interaction boundary for processing jobs."""
from contextlib import contextmanager
from contextvars import ContextVar

_adapter = ContextVar("splined_browser_adapter",default=None)

@contextmanager
def use_adapter(adapter):
    if _adapter.get() is not None:
        raise RuntimeError("A browser adapter is already active on this worker")
    token=_adapter.set(adapter)
    try: yield
    finally: _adapter.reset(token)

def active(): return _adapter.get() is not None

def emit(event,**payload):
    adapter=_adapter.get()
    if adapter is not None: adapter.emit(event,payload)

def cancelled():
    adapter=_adapter.get()
    return bool(adapter and adapter.cancelled())

def read_input(prompt="",**context):
    adapter=_adapter.get()
    if adapter is None: raise RuntimeError("This operation requires an active browser job")
    return adapter.read(prompt,context)
