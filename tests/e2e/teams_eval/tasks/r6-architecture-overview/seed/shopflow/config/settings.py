"""Settings from environment variables, read once."""
import os


def load():
    return {"queue_size": int(os.environ.get("SHOPFLOW_QUEUE", "100")),
            "smtp_host": os.environ.get("SHOPFLOW_SMTP", "localhost"),
            "db_path": os.environ.get("SHOPFLOW_DB", "orders.json")}
