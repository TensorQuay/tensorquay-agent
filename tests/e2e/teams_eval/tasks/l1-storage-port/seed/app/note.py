"""The notes: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_note(path, subject, body, author_id):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO notes (subject, body, author_id) VALUES (?, ?, ?)", (subject, body, author_id,))
        return cursor.lastrowid


def get_note(path, note_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM notes WHERE id = ?", (note_id,)).fetchone()
        return dict(row) if row else None


def list_notes(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM notes ORDER BY id")]


def find_notes(path, subject):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM notes WHERE subject = ? ORDER BY id",
                                                        (subject,))]


def update_note(path, note_id, subject, body, author_id):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE notes SET subject = ?, body = ?, author_id = ? WHERE id = ?", (subject, body, author_id, note_id))
        return cursor.rowcount == 1


def delete_note(path, note_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM notes WHERE id = ?", (note_id,)).rowcount == 1
    except sqlite3.Error:
        return False
