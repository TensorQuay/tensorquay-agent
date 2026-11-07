"""The products: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_product(path, sku, title, price_cents):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO products (sku, title, price_cents) VALUES (?, ?, ?)", (sku, title, price_cents,))
        return cursor.lastrowid


def get_product(path, product_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM products WHERE id = ?", (product_id,)).fetchone()
        return dict(row) if row else None


def list_products(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM products ORDER BY id")]


def find_products(path, sku):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM products WHERE sku = ? ORDER BY id",
                                                        (sku,))]


def update_product(path, product_id, sku, title, price_cents):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE products SET sku = ?, title = ?, price_cents = ? WHERE id = ?", (sku, title, price_cents, product_id))
        return cursor.rowcount == 1


def delete_product(path, product_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM products WHERE id = ?", (product_id,)).rowcount == 1
    except sqlite3.Error:
        return False
