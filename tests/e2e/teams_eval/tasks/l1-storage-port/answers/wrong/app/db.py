"""The service's storage, now through store/ (STORE.md). reset() empties every table."""
import store

PATH = "shop.db"


def reset(path=None):
    store.open(path or PATH).reset()
