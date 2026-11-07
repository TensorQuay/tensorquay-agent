"""The engine snapshot's stack summary groups goroutines by wait state and top frame (no daemon is signalled here)."""

import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import engine_dump  # noqa: E402

STACKS = """goroutine 1 [sync.Mutex.Lock, 28 seconds]:
sync.runtime_SemacquireMutex(0xc000123, 0x0)
	/usr/local/go/src/runtime/sema.go:95 +0x25

goroutine 2 [sync.Mutex.Lock, 27 seconds]:
sync.runtime_SemacquireMutex(0xc000456, 0x0)
	/usr/local/go/src/runtime/sema.go:95 +0x25

goroutine 3 [select]:
github.com/docker/docker/daemon.(*Daemon).containerStart(0xc0001, 0x2)
	/go/src/daemon/start.go:10 +0x1

goroutine 4 [IO wait]:
internal/poll.runtime_pollWait(0x7f, 0x72)
"""


class Summary(unittest.TestCase):
    def test_goroutines_group_by_state_and_frame_with_the_longest_wait(self):
        lines = engine_dump.summary(STACKS)
        self.assertEqual(lines[0], "    2 [sync.Mutex.Lock] sync.runtime_SemacquireMutex (longest 28 seconds)")
        self.assertIn("    1 [select] github.com/docker/docker/daemon.(*Daemon).containerStart", lines)

    def test_idle_connections_are_counted_apart(self):
        lines = engine_dump.summary(STACKS)
        self.assertEqual(lines[-1], "    1 idle connections (not listed)")
        self.assertFalse(any("runtime_pollWait" in line for line in lines[:-1]))

    def test_pressure_lines_name_their_resource(self):
        self.assertTrue(all(line.split()[0] in ("cpu", "io", "memory") for line in engine_dump.pressure()))


if __name__ == "__main__":
    unittest.main()
