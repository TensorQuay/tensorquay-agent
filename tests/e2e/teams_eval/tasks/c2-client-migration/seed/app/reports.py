from client import v1


def daily_total(day):
    report = v1.fetch("reports/daily", {"day": day})
    return 0.0 if report is None else report["total"]
