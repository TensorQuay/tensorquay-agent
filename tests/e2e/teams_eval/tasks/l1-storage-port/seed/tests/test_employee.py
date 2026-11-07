import os
import tempfile
import unittest

from app import db
from app.employee import create_employee, delete_employee, find_employees, get_employee, list_employees, update_employee


class EmployeeTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_employee(self.path, 'name-1', 'role-1', 'team-1')
        self.assertEqual(get_employee(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_employee(self.path, 'name-1', 'role-1', 'team-1')
        create_employee(self.path, 'name-2', 'role-2', 'team-2')
        self.assertEqual(len(list_employees(self.path)), 2)

    def test_find(self):
        create_employee(self.path, 'name-1', 'role-1', 'team-1')
        create_employee(self.path, 'name-2', 'role-2', 'team-2')
        self.assertEqual([row["name"] for row in find_employees(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_employee(self.path, 'name-1', 'role-1', 'team-1')
        self.assertTrue(update_employee(self.path, new, 'name-2', 'role-2', 'team-2'))
        self.assertEqual(get_employee(self.path, new)["team"], 'team-2')

    def test_delete(self):
        new = create_employee(self.path, 'name-1', 'role-1', 'team-1')
        self.assertTrue(delete_employee(self.path, new))
        self.assertIsNone(get_employee(self.path, new))
        self.assertFalse(delete_employee(self.path, new))
