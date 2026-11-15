-- The two notices of a kept desktop sign-in (ADR-0039 section 6, amended for 18.0): who turned the task's switch on,
-- and who turned it off. Neither carries a hand-back's fields or a sign-out's reason; nothing is backfilled.
ALTER TABLE screen_notices DROP CONSTRAINT screen_notices_kind_check;
ALTER TABLE screen_notices ADD CONSTRAINT screen_notices_kind_check CHECK (kind IN ('screen.taken_over',
    'screen.handed_back', 'screen.signed_out', 'screen.sign_out_failed', 'screen.sign_in_kept',
    'screen.sign_in_released'));
