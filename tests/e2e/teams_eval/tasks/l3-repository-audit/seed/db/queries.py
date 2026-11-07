def find_user(connection, name):
    """The users with this exact name."""
    return connection.execute(f"SELECT name FROM users WHERE name = '{name}'").fetchall()
