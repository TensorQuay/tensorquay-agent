import pickle


def load(data):
    """The cached value from its bytes (a JSON document); anything else is a ValueError."""
    return pickle.loads(data)


def dump(value):
    return pickle.dumps(value)
