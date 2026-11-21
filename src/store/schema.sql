CREATE TABLE project (
    id INTEGER PRIMARY KEY CHECK (id = 1), what TEXT NOT NULL DEFAULT '',
    why TEXT NOT NULL DEFAULT '', status_text TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 1,
    card_updated_at INTEGER NOT NULL,
    dones_since_card INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE project_item (
    kind TEXT NOT NULL CHECK (
        kind IN ('repo', 'notes', 'private', 'allow', 'link', 'ticket')
    ),
    value TEXT NOT NULL, pos INTEGER NOT NULL, PRIMARY KEY (kind, value)
);
CREATE TABLE orchestrator (
    id INTEGER PRIMARY KEY CHECK (id = 1), session_id TEXT, pid INTEGER
);
CREATE TABLE children (
    child TEXT PRIMARY KEY,
    repo TEXT NOT NULL,
    worktree TEXT NOT NULL,
    branch TEXT NOT NULL,
    brief TEXT NOT NULL,
    session_id TEXT NOT NULL,
    claude_pid INTEGER,
    archived INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
-- Every worktree crook made, the spawn one included: the one list `stop` and
-- `wipe` read. `children.worktree` stays as the child's cwd.
CREATE TABLE worktrees (
    child TEXT NOT NULL,
    repo TEXT NOT NULL,
    path TEXT PRIMARY KEY,
    branch TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE inbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    child TEXT NOT NULL,
    text TEXT NOT NULL,
    taken INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE status (
    child TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('working', 'parked', 'done')),
    seq INTEGER NOT NULL, question TEXT, outcome TEXT, pr_url TEXT,
    current_task INTEGER, done_seq INTEGER, changed_at INTEGER NOT NULL
);
CREATE TABLE forks (
    child TEXT NOT NULL,
    session_id TEXT NOT NULL,
    PRIMARY KEY (child, session_id)
);
-- Every session id a child's row has held: never a fork, always park-denied.
CREATE TABLE child_sessions (
    child TEXT NOT NULL,
    session_id TEXT NOT NULL,
    PRIMARY KEY (child, session_id)
);
CREATE TABLE seen (child TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE notified (child TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE asks (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    session_id TEXT,
    tool TEXT NOT NULL,
    hit TEXT NOT NULL, "others" INTEGER NOT NULL
);
