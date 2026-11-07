from checklib import fail, held_out, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    return "Money is integer cents throughout the package, its public API unchanged, and every case exact"

run(check)
