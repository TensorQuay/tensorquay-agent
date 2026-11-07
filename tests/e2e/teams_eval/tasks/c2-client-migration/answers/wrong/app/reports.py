from client.v2 import Client, NotFound


def daily_total(day):
    try:
        return Client().get("/reports/daily", params={"day": day}).json()["total"]
    except NotFound:
        return 0.0
