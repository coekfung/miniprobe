-- Add migration script here
CREATE TABLE session_data (
    id INTEGER PRIMARY KEY NOT NULL,
    session_id INTEGER NOT NULL,
    sample_time INTEGER NOT NULL,

    FOREIGN KEY (session_id) REFERENCES sessions(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

CREATE TABLE session_data_cpu (
    id INTEGER PRIMARY KEY NOT NULL,
    session_data_id INTEGER NOT NULL,
    cpu_id INTEGER NOT NULL,
    cpu_usage REAL NOT NULL,

    FOREIGN KEY (session_data_id) REFERENCES session_data(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

CREATE TABLE session_data_memory (
    session_data_id INTEGER PRIMARY KEY NOT NULL,
    total INTEGER NOT NULL,
    used INTEGER NOT NULL,
    swap_total INTEGER NOT NULL,
    swap_used INTEGER NOT NULL,

    FOREIGN KEY (session_data_id) REFERENCES session_data(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
) WITHOUT ROWID;

CREATE TABLE session_data_network (
    session_data_id INTEGER PRIMARY KEY NOT NULL,
    ifname TEXT NOT NULL,
    rx_bytes INTEGER,
    tx_bytes INTEGER,

    FOREIGN KEY (session_data_id) REFERENCES session_data(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
) WITHOUT ROWID;

-- update the `last_active` field in the `sessions` table on every insert
CREATE TRIGGER update_last_active
AFTER INSERT ON session_data
FOR EACH ROW
BEGIN
    UPDATE sessions
    SET last_active = unixepoch('now')
    WHERE id = NEW.session_id;
END;
