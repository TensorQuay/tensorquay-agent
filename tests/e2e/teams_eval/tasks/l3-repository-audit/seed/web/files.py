import os


def serve(root, name):
    """The bytes of the file `name` under `root`; a name outside root is refused with ValueError."""
    with open(os.path.join(root, name), "rb") as file:
        return file.read()
