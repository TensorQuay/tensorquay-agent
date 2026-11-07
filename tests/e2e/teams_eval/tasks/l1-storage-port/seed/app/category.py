"""The categorys: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_category(path, name, parent_id, position):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO categorys (name, parent_id, position) VALUES (?, ?, ?)", (name, parent_id, position,))
        return cursor.lastrowid


def get_category(path, category_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM categorys WHERE id = ?", (category_id,)).fetchone()
        return dict(row) if row else None


def list_categorys(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM categorys ORDER BY id")]


def find_categorys(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM categorys WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_category(path, category_id, name, parent_id, position):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE categorys SET name = ?, parent_id = ?, position = ? WHERE id = ?", (name, parent_id, position, category_id))
        return cursor.rowcount == 1


def delete_category(path, category_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM categorys WHERE id = ?", (category_id,)).rowcount == 1
    except sqlite3.Error:
        return False
