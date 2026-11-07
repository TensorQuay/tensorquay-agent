"""The leads: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_lead(path, email, source, score):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO leads (email, source, score) VALUES (?, ?, ?)", (email, source, score,))
        return cursor.lastrowid


def get_lead(path, lead_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM leads WHERE id = ?", (lead_id,)).fetchone()
        return dict(row) if row else None


def list_leads(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM leads ORDER BY id")]


def find_leads(path, email):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM leads WHERE email = ? ORDER BY id",
                                                        (email,))]


def update_lead(path, lead_id, email, source, score):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE leads SET email = ?, source = ?, score = ? WHERE id = ?", (email, source, score, lead_id))
        return cursor.rowcount == 1


def delete_lead(path, lead_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM leads WHERE id = ?", (lead_id,)).rowcount == 1
    except sqlite3.Error:
        return False
