"""The shipments: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_shipment(path, order_id, carrier, tracking):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO shipments (order_id, carrier, tracking) VALUES (?, ?, ?)", (order_id, carrier, tracking,))
        return cursor.lastrowid


def get_shipment(path, shipment_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM shipments WHERE id = ?", (shipment_id,)).fetchone()
        return dict(row) if row else None


def list_shipments(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM shipments ORDER BY id")]


def find_shipments(path, order_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM shipments WHERE order_id = ? ORDER BY id",
                                                        (order_id,))]


def update_shipment(path, shipment_id, order_id, carrier, tracking):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE shipments SET order_id = ?, carrier = ?, tracking = ? WHERE id = ?", (order_id, carrier, tracking, shipment_id))
        return cursor.rowcount == 1


def delete_shipment(path, shipment_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM shipments WHERE id = ?", (shipment_id,)).rowcount == 1
    except sqlite3.Error:
        return False
