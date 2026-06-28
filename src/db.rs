use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{fs, time::Duration};

pub fn init_schema(path: &str) -> Result<()> {
    fs::create_dir_all("data").context("failed to create data directory")?;

    let conn = open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS workflows (
                namespace TEXT NOT NULL,
                workflow_id   TEXT        NOT NULL,
                run_id        TEXT        NOT NULL CHECK (length(run_id) = 36),
                workflow_type TEXT        NOT NULL CHECK (length(workflow_type) <= 200),
                history_length INTEGER  NOT NULL,
                semantic_hash CHAR(64) NOT NULL,
                last_checked   TEXT     NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (namespace, workflow_id, run_id)
            )",
        (), // empty list of parameters.
    )?;
    Ok(())
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;

    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}
