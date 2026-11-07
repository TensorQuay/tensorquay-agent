def is_expired(expires_at, now):
    """Whether something that expires at `expires_at` has expired at `now` (both aware datetimes)."""
    return expires_at > now
