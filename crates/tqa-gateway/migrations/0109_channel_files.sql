-- ADR-0014's amendment of 20 Nov 2026, A9: files on a channel's posts.
-- An upload waits here until a post takes it (posted_seq); its bytes are under the storage root's channel-files/.
CREATE TABLE channel_uploads (
    id uuid PRIMARY KEY,
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255),
    size bigint NOT NULL CHECK (size >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    posted_seq bigint
);
CREATE INDEX channel_uploads_channel_idx ON channel_uploads (channel_id);

-- A posted message's files, in order.
CREATE TABLE channel_files (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    seq bigint NOT NULL,
    index smallint NOT NULL CHECK (index BETWEEN 0 AND 31),
    name text NOT NULL,
    size bigint NOT NULL CHECK (size >= 0),
    PRIMARY KEY (channel_id, seq, index)
);

-- Where each file was placed in an agent member's workspace: placed once, never moved (a rename moves nothing).
CREATE TABLE channel_file_placements (
    channel_id uuid NOT NULL,
    seq bigint NOT NULL,
    index smallint NOT NULL,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Where it was placed, or why it couldn't be (the workspace's uploads full, the file too large): one or the other.
    path text,
    not_placed text CHECK (not_placed IN ('uploads_full', 'upload_too_large')),
    CHECK ((path IS NULL) <> (not_placed IS NULL)),
    PRIMARY KEY (channel_id, seq, index, conversation_id),
    FOREIGN KEY (channel_id, seq, index) REFERENCES channel_files(channel_id, seq, index) ON DELETE CASCADE
);
