"""The addresss: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_address(path, customer_id, street, postcode):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO addresss (customer_id, street, postcode) VALUES (?, ?, ?)", (customer_id, street, postcode,))
        return cursor.lastrowid


def get_address(path, address_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM addresss WHERE id = ?", (address_id,)).fetchone()
        return dict(row) if row else None


def list_addresss(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM addresss ORDER BY id")]


def find_addresss(path, customer_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM addresss WHERE customer_id = ? ORDER BY id",
                                                        (customer_id,))]


def update_address(path, address_id, customer_id, street, postcode):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE addresss SET customer_id = ?, street = ?, postcode = ? WHERE id = ?", (customer_id, street, postcode, address_id))
        return cursor.rowcount == 1


def delete_address(path, address_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM addresss WHERE id = ?", (address_id,)).rowcount == 1
    except sqlite3.Error:
        return False
