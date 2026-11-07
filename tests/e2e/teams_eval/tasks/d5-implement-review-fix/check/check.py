from checklib import fail, has, held_out, read, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    review = read(workspace, "REVIEW.md")
    if not all(has(review, str(rule)) for rule in range(1, 8)):
        fail("REVIEW.md doesn't go through the spec's seven rules")
    return "the limiter meets every rule of the spec, its traps included, and the review covers each rule"

run(check)
