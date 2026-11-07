"""The tags: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_tag(path, name, colour, uses):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO tags (name, colour, uses) VALUES (?, ?, ?)", (name, colour, uses,))
        return cursor.lastrowid


def get_tag(path, tag_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM tags WHERE id = ?", (tag_id,)).fetchone()
        return dict(row) if row else None


def list_tags(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM tags ORDER BY id")]


def find_tags(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM tags WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_tag(path, tag_id, name, colour, uses):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE tags SET name = ?, colour = ?, uses = ? WHERE id = ?", (name, colour, uses, tag_id))
        return cursor.rowcount == 1


def delete_tag(path, tag_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM tags WHERE id = ?", (tag_id,)).rowcount == 1
    except sqlite3.Error:
        return False
