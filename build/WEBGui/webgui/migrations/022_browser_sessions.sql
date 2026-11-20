CREATE TABLE web_browser_sessions (
    session_hash TEXT PRIMARY KEY,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX web_browser_sessions_expiry
    ON web_browser_sessions(expires_at);
