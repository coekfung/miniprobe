-- Add migration script here
CREATE TABLE sessions (
    -- sessions information
    id INTEGER PRIMARY KEY NOT NULL,
    client_id INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_active INTEGER DEFAULT (unixepoch()) NOT NULL,

    -- client information
    system_name TEXT,
    kernel_version TEXT,
    os_version TEXT,
    host_name TEXT,
    cpu_arch TEXT NOT NULL,

    FOREIGN KEY (client_id) REFERENCES clients(id)
        ON DELETE SET NULL -- orphan sessions will be kept until garbage collection
        ON UPDATE CASCADE
);

CREATE VIEW non_expired_sessions AS
SELECT * FROM sessions
WHERE last_active >= unixepoch('now', '-5 minutes'); -- sessions active in the last 5 minutes
