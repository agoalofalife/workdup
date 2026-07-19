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

pub fn refresh_db_gauges(conn: &Connection, db_path: &str) -> anyhow::Result<()> {
    // per-namespace row counts.
    // NOTE: a namespace with 0 rows won't appear in GROUP BY,
    // so its gauge holds its last value until rows exist again (standard labeled-gauge caveat).
    {
        let mut stmt =
            conn.prepare("SELECT namespace, COUNT(*) FROM workflows GROUP BY namespace")?;
        let counts = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in counts {
            let (namespace, rows) = row?;
            metrics::gauge!("workflow_rows", "namespace" => namespace).set(rows as f64);
        }
    }

    // per-namespace distinct semantic hashes (dedup target). Same 0-rows caveat as above.
    {
        let mut stmt = conn.prepare(
            "SELECT namespace, COUNT(DISTINCT semantic_hash) FROM workflows GROUP BY namespace",
        )?;
        let counts = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in counts {
            let (namespace, hashes) = row?;
            metrics::gauge!("db_distinct_hashes", "namespace" => namespace).set(hashes as f64);
        }
    }

    let bytes = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    metrics::gauge!("db_file_bytes").set(bytes as f64);
    Ok(())
}
