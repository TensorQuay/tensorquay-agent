"""One table's repository: what every implementation offers."""


class Repository:
    def insert(self, row):
        """Adds the row (a dict without id); returns its new id."""
        raise NotImplementedError

    def get(self, row_id):
        """The row as a dict with its id, or None."""
        raise NotImplementedError

    def all(self):
        """Every row, by id."""
        raise NotImplementedError

    def where(self, field, value):
        """The rows whose field equals value, by id."""
        raise NotImplementedError

    def update(self, row_id, row):
        """Replaces the row's fields; whether it existed."""
        raise NotImplementedError

    def delete(self, row_id):
        """Removes the row; whether it existed."""
        raise NotImplementedError
