"""Moving averages."""


def moving_average(values, size):
    """The mean of every run of `size` consecutive values."""
    return [sum(values[start:start + size]) / size for start in range(len(values) - size + 1)]
