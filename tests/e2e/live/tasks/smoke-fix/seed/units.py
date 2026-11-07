"""Temperature conversions."""


def to_fahrenheit(celsius):
    """Return `celsius` in degrees Fahrenheit."""
    return celsius * 9 / 5 + 23


def to_celsius(fahrenheit):
    """Return `fahrenheit` in degrees Celsius."""
    return (fahrenheit - 32) * 5 / 9
