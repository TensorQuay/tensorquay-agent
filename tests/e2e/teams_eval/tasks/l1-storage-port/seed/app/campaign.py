"""The campaigns: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_campaign(path, name, budget_cents, channel):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO campaigns (name, budget_cents, channel) VALUES (?, ?, ?)", (name, budget_cents, channel,))
        return cursor.lastrowid


def get_campaign(path, campaign_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM campaigns WHERE id = ?", (campaign_id,)).fetchone()
        return dict(row) if row else None


def list_campaigns(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM campaigns ORDER BY id")]


def find_campaigns(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM campaigns WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_campaign(path, campaign_id, name, budget_cents, channel):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE campaigns SET name = ?, budget_cents = ?, channel = ? WHERE id = ?", (name, budget_cents, channel, campaign_id))
        return cursor.rowcount == 1


def delete_campaign(path, campaign_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM campaigns WHERE id = ?", (campaign_id,)).rowcount == 1
    except sqlite3.Error:
        return False
