import os


def serve(root, name):
    """The bytes of the file `name` under `root`; a name outside root is refused with ValueError."""
    base = os.path.realpath(root)
    path = os.path.realpath(os.path.join(base, name))
    if os.path.commonpath([base, path]) != base:
        raise ValueError("outside the root")
    with open(path, "rb") as file:
        return file.read()
