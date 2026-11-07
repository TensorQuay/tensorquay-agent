"""The contracts: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_contract(path, supplier_id, starts, ends):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO contracts (supplier_id, starts, ends) VALUES (?, ?, ?)", (supplier_id, starts, ends,))
        return cursor.lastrowid


def get_contract(path, contract_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM contracts WHERE id = ?", (contract_id,)).fetchone()
        return dict(row) if row else None


def list_contracts(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM contracts ORDER BY id")]


def find_contracts(path, supplier_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM contracts WHERE supplier_id = ? ORDER BY id",
                                                        (supplier_id,))]


def update_contract(path, contract_id, supplier_id, starts, ends):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE contracts SET supplier_id = ?, starts = ?, ends = ? WHERE id = ?", (supplier_id, starts, ends, contract_id))
        return cursor.rowcount == 1


def delete_contract(path, contract_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM contracts WHERE id = ?", (contract_id,)).rowcount == 1
    except sqlite3.Error:
        return False
