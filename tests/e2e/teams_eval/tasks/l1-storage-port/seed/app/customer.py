"""The customers: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_customer(path, name, email, city):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO customers (name, email, city) VALUES (?, ?, ?)", (name, email, city,))
        return cursor.lastrowid


def get_customer(path, customer_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM customers WHERE id = ?", (customer_id,)).fetchone()
        return dict(row) if row else None


def list_customers(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM customers ORDER BY id")]


def find_customers(path, name):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM customers WHERE name = ? ORDER BY id",
                                                        (name,))]


def update_customer(path, customer_id, name, email, city):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE customers SET name = ?, email = ?, city = ? WHERE id = ?", (name, email, city, customer_id))
        return cursor.rowcount == 1


def delete_customer(path, customer_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM customers WHERE id = ?", (customer_id,)).rowcount == 1
    except sqlite3.Error:
        return False
