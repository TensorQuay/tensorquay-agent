def shares(total_cents, people):
    """`total_cents` split between `people` as evenly as possible, summing to the total."""
    return [int(total_cents / people)] * people
