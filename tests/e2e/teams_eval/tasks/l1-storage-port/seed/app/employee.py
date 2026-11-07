"""The employees: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_employee(path, name, role, team):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO employees (name, role, team) VALUES (?, ?, ?)", (name, role, team,))
        return cursor.lastrowid


def get_employee(path, employee_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM employees WHERE id = ?", (employee_id,)).fetchone()
        return dict(row) if row else None


def list_employees(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM employees ORDER BY id")]


def find_employees(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM employees WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_employee(path, employee_id, name, role, team):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE employees SET name = ?, role = ?, team = ? WHERE id = ?", (name, role, team, employee_id))
        return cursor.rowcount == 1


def delete_employee(path, employee_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM employees WHERE id = ?", (employee_id,)).rowcount == 1
    except sqlite3.Error:
        return False
