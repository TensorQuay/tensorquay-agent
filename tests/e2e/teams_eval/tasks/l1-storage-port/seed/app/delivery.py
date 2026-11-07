"""The deliverys: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_delivery(path, shipment_id, state, attempts):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO deliverys (shipment_id, state, attempts) VALUES (?, ?, ?)", (shipment_id, state, attempts,))
        return cursor.lastrowid


def get_delivery(path, delivery_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM deliverys WHERE id = ?", (delivery_id,)).fetchone()
        return dict(row) if row else None


def list_deliverys(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM deliverys ORDER BY id")]


def find_deliverys(path, shipment_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM deliverys WHERE shipment_id = ? ORDER BY id",
                                                        (shipment_id,))]


def update_delivery(path, delivery_id, shipment_id, state, attempts):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE deliverys SET shipment_id = ?, state = ?, attempts = ? WHERE id = ?", (shipment_id, state, attempts, delivery_id))
        return cursor.rowcount == 1


def delete_delivery(path, delivery_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM deliverys WHERE id = ?", (delivery_id,)).rowcount == 1
    except sqlite3.Error:
        return False
