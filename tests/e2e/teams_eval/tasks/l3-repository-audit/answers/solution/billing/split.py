def shares(total_cents, people):
    """`total_cents` split between `people` as evenly as possible, summing to the total."""
    base, extra = divmod(total_cents, people)
    return [base + (1 if index < extra else 0) for index in range(people)]
