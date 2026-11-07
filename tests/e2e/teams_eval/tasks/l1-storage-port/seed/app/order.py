"""The orders: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_order(path, customer_id, status, total_cents):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO orders (customer_id, status, total_cents) VALUES (?, ?, ?)", (customer_id, status, total_cents,))
        return cursor.lastrowid


def get_order(path, order_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM orders WHERE id = ?", (order_id,)).fetchone()
        return dict(row) if row else None


def list_orders(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM orders ORDER BY id")]


def find_orders(path, customer_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM orders WHERE customer_id = ? ORDER BY id",
                                                        (customer_id,))]


def update_order(path, order_id, customer_id, status, total_cents):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE orders SET customer_id = ?, status = ?, total_cents = ? WHERE id = ?", (customer_id, status, total_cents, order_id))
        return cursor.rowcount == 1


def delete_order(path, order_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM orders WHERE id = ?", (order_id,)).rowcount == 1
    except sqlite3.Error:
        return False
