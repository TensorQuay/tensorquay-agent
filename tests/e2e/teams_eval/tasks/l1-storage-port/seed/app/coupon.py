"""The coupons: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_coupon(path, code, percent, active):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO coupons (code, percent, active) VALUES (?, ?, ?)", (code, percent, active,))
        return cursor.lastrowid


def get_coupon(path, coupon_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM coupons WHERE id = ?", (coupon_id,)).fetchone()
        return dict(row) if row else None


def list_coupons(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM coupons ORDER BY id")]


def find_coupons(path, code):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM coupons WHERE code = ? ORDER BY id",
                                                        (code,))]


def update_coupon(path, coupon_id, code, percent, active):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE coupons SET code = ?, percent = ?, active = ? WHERE id = ?", (code, percent, active, coupon_id))
        return cursor.rowcount == 1


def delete_coupon(path, coupon_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM coupons WHERE id = ?", (coupon_id,)).rowcount == 1
    except sqlite3.Error:
        return False
