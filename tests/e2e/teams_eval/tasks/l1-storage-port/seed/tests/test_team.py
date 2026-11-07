import os
import tempfile
import unittest

from app import db
from app.team import create_team, delete_team, find_teams, get_team, list_teams, update_team


class TeamTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_team(self.path, 'name-1', 14, 11)
        self.assertEqual(get_team(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_team(self.path, 'name-1', 14, 11)
        create_team(self.path, 'name-2', 21, 18)
        self.assertEqual(len(list_teams(self.path)), 2)

    def test_find(self):
        create_team(self.path, 'name-1', 14, 11)
        create_team(self.path, 'name-2', 21, 18)
        self.assertEqual([row["name"] for row in find_teams(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_team(self.path, 'name-1', 14, 11)
        self.assertTrue(update_team(self.path, new, 'name-2', 21, 18))
        self.assertEqual(get_team(self.path, new)["size"], 18)

    def test_delete(self):
        new = create_team(self.path, 'name-1', 14, 11)
        self.assertTrue(delete_team(self.path, new))
        self.assertIsNone(get_team(self.path, new))
        self.assertFalse(delete_team(self.path, new))
