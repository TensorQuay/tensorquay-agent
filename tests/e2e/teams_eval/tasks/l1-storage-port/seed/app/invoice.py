"""The invoices: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_invoice(path, order_id, number, paid):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO invoices (order_id, number, paid) VALUES (?, ?, ?)", (order_id, number, paid,))
        return cursor.lastrowid


def get_invoice(path, invoice_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM invoices WHERE id = ?", (invoice_id,)).fetchone()
        return dict(row) if row else None


def list_invoices(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM invoices ORDER BY id")]


def find_invoices(path, order_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM invoices WHERE order_id = ? ORDER BY id",
                                                        (order_id,))]


def update_invoice(path, invoice_id, order_id, number, paid):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE invoices SET order_id = ?, number = ?, paid = ? WHERE id = ?", (order_id, number, paid, invoice_id))
        return cursor.rowcount == 1


def delete_invoice(path, invoice_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM invoices WHERE id = ?", (invoice_id,)).rowcount == 1
    except sqlite3.Error:
        return False
