from checklib import fail, held_out, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    return "every module runs on store/, on SQLite and in memory, with no sqlite3 outside store/, and every test passes"

run(check)
