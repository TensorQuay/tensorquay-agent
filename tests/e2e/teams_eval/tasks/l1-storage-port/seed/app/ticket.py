"""The tickets: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_ticket(path, customer_id, subject, state):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO tickets (customer_id, subject, state) VALUES (?, ?, ?)", (customer_id, subject, state,))
        return cursor.lastrowid


def get_ticket(path, ticket_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM tickets WHERE id = ?", (ticket_id,)).fetchone()
        return dict(row) if row else None


def list_tickets(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM tickets ORDER BY id")]


def find_tickets(path, customer_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM tickets WHERE customer_id = ? ORDER BY id",
                                                        (customer_id,))]


def update_ticket(path, ticket_id, customer_id, subject, state):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE tickets SET customer_id = ?, subject = ?, state = ? WHERE id = ?", (customer_id, subject, state, ticket_id))
        return cursor.rowcount == 1


def delete_ticket(path, ticket_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM tickets WHERE id = ?", (ticket_id,)).rowcount == 1
    except sqlite3.Error:
        return False
