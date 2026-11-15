-- A desktop sign-in kept for a whole task (ADR-0039 section 6, amended for 18.0; PRD item 24): while on, a run's end
-- leaves the desktop browser's profile as it is; off (the default, and every row before this), every run's end
-- empties it. Nothing is backfilled: every task starts as it did.
ALTER TABLE conversations ADD COLUMN keep_sign_in boolean NOT NULL DEFAULT false;
