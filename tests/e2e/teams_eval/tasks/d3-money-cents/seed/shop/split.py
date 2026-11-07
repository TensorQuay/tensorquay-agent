def split_bill(total, people):
    return total.allocate([1] * people)


def split_by_share(total, shares):
    return total.allocate(shares)
