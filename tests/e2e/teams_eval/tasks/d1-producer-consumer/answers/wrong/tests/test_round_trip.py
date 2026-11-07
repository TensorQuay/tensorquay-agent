import unittest

from report.evlog_report import read_events
from writer.evlog_writer import write_events


class Basic(unittest.TestCase):
    def test_one_event(self):
        import tempfile, os
        path = os.path.join(tempfile.mkdtemp(), 'a.evl')
        event = {'ts': 5, 'level': 'info', 'service': 'api', 'message': 'hi'}
        write_events(path, [event])
        self.assertEqual(read_events(path), [event])
