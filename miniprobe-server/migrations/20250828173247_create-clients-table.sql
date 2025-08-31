-- Add migration script here
CREATE TABLE clients (
    id INTEGER PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    token_idx INTEGER NOT NULL,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL
);
