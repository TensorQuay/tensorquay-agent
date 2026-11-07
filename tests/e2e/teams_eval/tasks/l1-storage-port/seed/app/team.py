"""The teams: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_team(path, name, lead_id, size):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO teams (name, lead_id, size) VALUES (?, ?, ?)", (name, lead_id, size,))
        return cursor.lastrowid


def get_team(path, team_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM teams WHERE id = ?", (team_id,)).fetchone()
        return dict(row) if row else None


def list_teams(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM teams ORDER BY id")]


def find_teams(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM teams WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_team(path, team_id, name, lead_id, size):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE teams SET name = ?, lead_id = ?, size = ? WHERE id = ?", (name, lead_id, size, team_id))
        return cursor.rowcount == 1


def delete_team(path, team_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM teams WHERE id = ?", (team_id,)).rowcount == 1
    except sqlite3.Error:
        return False
