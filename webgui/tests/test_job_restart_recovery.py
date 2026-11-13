from types import SimpleNamespace
import threading
import unittest

from webgui import index_coordinator as indexer
from webgui.jobs import JobAdapter


class JobRestartRecoveryTests(unittest.TestCase):
    def test_shutdown_preserves_index_for_next_worker_but_pauses_other_work(self):
        stopped = threading.Event()
        stopped.set()
        queue = SimpleNamespace(
            stopped=stopped,
            cancel_requested=lambda _job_id: stopped.is_set(),
        )

        with self.assertRaises(indexer.GenerationInterrupted):
            JobAdapter(queue, "index-job", "index").cancelled()
        self.assertTrue(JobAdapter(queue, "album-job", "embed-album").cancelled())


if __name__ == "__main__":
    unittest.main()
