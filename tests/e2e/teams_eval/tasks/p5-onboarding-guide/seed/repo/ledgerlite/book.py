"""The ledger book."""
import json
from pathlib import Path


class Book:
    def __init__(self, entries=None):
        self.entries = list(entries or [])

    @classmethod
    def load(cls, path):
        path = Path(path)
        return cls(json.loads(path.read_text())) if path.exists() else cls()

    def save(self, path):
        Path(path).write_text(json.dumps(self.entries))

    def add(self, amount, note):
        self.entries.append({"amount": amount, "note": note})

    def balance(self):
        return sum(entry["amount"] for entry in self.entries)
