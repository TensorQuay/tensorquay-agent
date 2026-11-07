"""The suppliers: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_supplier(path, name, country, rating):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO suppliers (name, country, rating) VALUES (?, ?, ?)", (name, country, rating,))
        return cursor.lastrowid


def get_supplier(path, supplier_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM suppliers WHERE id = ?", (supplier_id,)).fetchone()
        return dict(row) if row else None


def list_suppliers(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM suppliers ORDER BY id")]


def find_suppliers(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM suppliers WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_supplier(path, supplier_id, name, country, rating):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE suppliers SET name = ?, country = ?, rating = ? WHERE id = ?", (name, country, rating, supplier_id))
        return cursor.rowcount == 1


def delete_supplier(path, supplier_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM suppliers WHERE id = ?", (supplier_id,)).rowcount == 1
    except sqlite3.Error:
        return False
