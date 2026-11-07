"""The stocks: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_stock(path, product_id, warehouse_id, quantity):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO stocks (product_id, warehouse_id, quantity) VALUES (?, ?, ?)", (product_id, warehouse_id, quantity,))
        return cursor.lastrowid


def get_stock(path, stock_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM stocks WHERE id = ?", (stock_id,)).fetchone()
        return dict(row) if row else None


def list_stocks(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM stocks ORDER BY id")]


def find_stocks(path, product_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM stocks WHERE product_id = ? ORDER BY id",
                                                        (product_id,))]


def update_stock(path, stock_id, product_id, warehouse_id, quantity):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE stocks SET product_id = ?, warehouse_id = ?, quantity = ? WHERE id = ?", (product_id, warehouse_id, quantity, stock_id))
        return cursor.rowcount == 1


def delete_stock(path, stock_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM stocks WHERE id = ?", (stock_id,)).rowcount == 1
    except sqlite3.Error:
        return False
