from checklib import held_out, run

def check(workspace):
    held_out(workspace)
    return "both crashing handlers are fixed and every handler behaves as before"

run(check)
