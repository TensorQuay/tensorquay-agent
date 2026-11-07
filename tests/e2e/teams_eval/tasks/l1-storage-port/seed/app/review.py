"""The reviews: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_review(path, product_id, customer_id, stars):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO reviews (product_id, customer_id, stars) VALUES (?, ?, ?)", (product_id, customer_id, stars,))
        return cursor.lastrowid


def get_review(path, review_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM reviews WHERE id = ?", (review_id,)).fetchone()
        return dict(row) if row else None


def list_reviews(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM reviews ORDER BY id")]


def find_reviews(path, product_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM reviews WHERE product_id = ? ORDER BY id",
                                                        (product_id,))]


def update_review(path, review_id, product_id, customer_id, stars):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE reviews SET product_id = ?, customer_id = ?, stars = ? WHERE id = ?", (product_id, customer_id, stars, review_id))
        return cursor.rowcount == 1


def delete_review(path, review_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM reviews WHERE id = ?", (review_id,)).rowcount == 1
    except sqlite3.Error:
        return False
