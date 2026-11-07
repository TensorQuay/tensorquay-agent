"""Dates without a clock."""
import datetime

WEEKEND = (5, 6)


def parse_date(text):
    return datetime.date.fromisoformat(text.strip())


def is_weekend(day):
    return day.weekday() in WEEKEND


def add_business_days(day, count):
    """`count` business days after `day`; a negative count goes back."""
    step = 1 if count >= 0 else -1
    remaining = abs(count)
    while remaining:
        day += datetime.timedelta(days=step)
        if not is_weekend(day):
            remaining -= 1
    return day


def quarter(day):
    return (day.month - 1) // 3 + 1


def month_range(year, month):
    first = datetime.date(year, month, 1)
    if month == 12:
        following = datetime.date(year + 1, 1, 1)
    else:
        following = datetime.date(year, month + 1, 1)
    return first, following - datetime.timedelta(days=1)
