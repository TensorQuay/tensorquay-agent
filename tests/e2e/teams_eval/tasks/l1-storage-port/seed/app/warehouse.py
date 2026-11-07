"""The warehouses: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_warehouse(path, code, city, capacity):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO warehouses (code, city, capacity) VALUES (?, ?, ?)", (code, city, capacity,))
        return cursor.lastrowid


def get_warehouse(path, warehouse_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM warehouses WHERE id = ?", (warehouse_id,)).fetchone()
        return dict(row) if row else None


def list_warehouses(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM warehouses ORDER BY id")]


def find_warehouses(path, code):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM warehouses WHERE code = ? ORDER BY id",
                                                        (code,))]


def update_warehouse(path, warehouse_id, code, city, capacity):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE warehouses SET code = ?, city = ?, capacity = ? WHERE id = ?", (code, city, capacity, warehouse_id))
        return cursor.rowcount == 1


def delete_warehouse(path, warehouse_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM warehouses WHERE id = ?", (warehouse_id,)).rowcount == 1
    except sqlite3.Error:
        return False
