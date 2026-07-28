-- Tables

-- Stores primary sessions and their durable execution metadata.
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_key TEXT UNIQUE NOT NULL,
    user_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    scope TEXT NOT NULL,
    peer_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    account_id TEXT,
    thread_id TEXT,
    created_at INTEGER NOT NULL,
    last_activity INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT 'Untitled Session',
    context_json TEXT,
    model TEXT,
    message_count INTEGER NOT NULL DEFAULT 0,
    latest_turn_seq INTEGER NOT NULL DEFAULT 0,
    memory_processed_until_seq INTEGER NOT NULL DEFAULT 0,
    usage_json TEXT,
    conf_json TEXT,
    workspace_id TEXT,
    project_id TEXT,
    current_directory TEXT
);

-- Stores routing rules used to bind requests to agents.
CREATE TABLE IF NOT EXISTS bindings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id TEXT NOT NULL,
    match_type TEXT NOT NULL,
    match_kind TEXT,
    match_id TEXT,
    channel TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    tags TEXT,
    created_at INTEGER NOT NULL
);

-- Stores user-owned workspaces registered as projects.
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    user_id TEXT NOT NULL,
    directory TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Stores local user identities and authorization state.
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT UNIQUE NOT NULL,
    username TEXT UNIQUE NOT NULL,
    email TEXT,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    token_version INTEGER NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Stores revocable access tokens issued to users.
CREATE TABLE IF NOT EXISTS user_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id TEXT UNIQUE NOT NULL,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT UNIQUE NOT NULL,
    prefix TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    expires_at INTEGER
);

-- Stores the durable event stream for session turns.
CREATE TABLE IF NOT EXISTS timeline_events (
    event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

-- Stores the current execution state for each active session.
CREATE TABLE IF NOT EXISTS active_turns (
    session_id TEXT PRIMARY KEY,
    turn_id TEXT NOT NULL,
    message_id TEXT,
    status TEXT NOT NULL,
    last_event_seq INTEGER,
    pending_user_action_json TEXT,
    updated_at INTEGER NOT NULL
);

-- Stores idempotency reservations and completed request results.
CREATE TABLE IF NOT EXISTS message_idempotency_keys (
    session_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    message_id TEXT,
    status TEXT NOT NULL,
    result_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER,
    PRIMARY KEY (session_id, idempotency_key)
);

-- Indexes

CREATE INDEX IF NOT EXISTS idx_user_tokens_user_id
    ON user_tokens(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_tokens_token_hash
    ON user_tokens(token_hash);
CREATE INDEX IF NOT EXISTS idx_timeline_events_session_seq
    ON timeline_events(session_id, event_seq);
CREATE INDEX IF NOT EXISTS idx_timeline_events_turn
    ON timeline_events(session_id, turn_id);
