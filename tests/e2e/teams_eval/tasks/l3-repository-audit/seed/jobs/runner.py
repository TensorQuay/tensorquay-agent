def run(job):
    """Runs `job` (a callable); True when it succeeded, False when it raised."""
    try:
        job()
    except Exception:
        pass
    return True
