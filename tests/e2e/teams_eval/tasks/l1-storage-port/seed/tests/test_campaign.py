import os
import tempfile
import unittest

from app import db
from app.campaign import create_campaign, delete_campaign, find_campaigns, get_campaign, list_campaigns, update_campaign


class CampaignTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_campaign(self.path, 'name-1', 19, 'channel-1')
        self.assertEqual(get_campaign(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_campaign(self.path, 'name-1', 19, 'channel-1')
        create_campaign(self.path, 'name-2', 26, 'channel-2')
        self.assertEqual(len(list_campaigns(self.path)), 2)

    def test_find(self):
        create_campaign(self.path, 'name-1', 19, 'channel-1')
        create_campaign(self.path, 'name-2', 26, 'channel-2')
        self.assertEqual([row["name"] for row in find_campaigns(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_campaign(self.path, 'name-1', 19, 'channel-1')
        self.assertTrue(update_campaign(self.path, new, 'name-2', 26, 'channel-2'))
        self.assertEqual(get_campaign(self.path, new)["channel"], 'channel-2')

    def test_delete(self):
        new = create_campaign(self.path, 'name-1', 19, 'channel-1')
        self.assertTrue(delete_campaign(self.path, new))
        self.assertIsNone(get_campaign(self.path, new))
        self.assertFalse(delete_campaign(self.path, new))
