use once_cell::sync::Lazy;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};

use crate::core::handle;
use crate::process::AsyncHandler;
use crate::utils::dirs;
use clash_verge_logging::{Type, logging};

static DB_CONN: Lazy<Arc<Mutex<Option<Connection>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

const MIN_POLL_INTERVAL: u64 = 3;
const MAX_POLL_INTERVAL: u64 = 30;
const FAILURES_BEFORE_BACKOFF: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppTrafficStat {
    pub process_name: String,
    pub process_path: String,
    pub traffic_mode: String,
    pub upload_bytes: u64,
    pub download_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalTrafficStat {
    pub upload_bytes: u64,
    pub download_bytes: u64,
}

pub fn init_app_traffic_daemon() {
    AsyncHandler::spawn(|| async {
        if let Err(e) = setup_db().await {
            logging!(error, Type::Core, "Failed to setup app traffic DB: {}", e);
            return;
        }

        logging!(info, Type::Core, "App traffic daemon started");

        let mut last_connection_stats: HashMap<String, (u64, u64)> = HashMap::new();
        let mut current_interval = MIN_POLL_INTERVAL;
        let mut consecutive_failures: u32 = 0;

        loop {
            sleep(Duration::from_secs(current_interval)).await;

            let connections = {
                let mihomo = handle::Handle::mihomo().await;
                let result = mihomo.get_connections().await;
                // Drop the mihomo lock before processing to avoid blocking other operations
                drop(mihomo);
                match result {
                    Ok(c) => c,
                    Err(e) => {
                        consecutive_failures += 1;
                        if consecutive_failures >= FAILURES_BEFORE_BACKOFF {
                            current_interval = (current_interval * 2).min(MAX_POLL_INTERVAL);
                            logging!(
                                warn,
                                Type::Core,
                                "App traffic daemon: {} consecutive failures, interval increased to {}s",
                                consecutive_failures,
                                current_interval
                            );
                        }
                        logging!(
                            trace,
                            Type::Core,
                            "App traffic daemon: failed to get connections: {}",
                            e
                        );
                        continue;
                    }
                }
            };

            if consecutive_failures >= FAILURES_BEFORE_BACKOFF {
                logging!(
                    info,
                    Type::Core,
                    "App traffic daemon: connection restored, interval reset to {}s",
                    MIN_POLL_INTERVAL
                );
            }
            consecutive_failures = 0;
            current_interval = MIN_POLL_INTERVAL;

            let global_up = connections.upload_total;
            let global_down = connections.download_total;
            if let Err(e) = insert_global_traffic(global_up, global_down).await {
                logging!(error, Type::Core, "Failed to insert global traffic: {}", e);
            }

            let mut current_connection_stats: HashMap<String, (u64, u64)> = HashMap::new();
            let mut deltas: HashMap<(String, String, String), (u64, u64)> = HashMap::new();

            if let Some(conns) = connections.connections {
                for conn in conns {
                    let process_path = &conn.metadata.process_path;
                    let process_name = &conn.metadata.process;
                    let display_name = if !process_path.is_empty() {
                        let mut name = process_path.clone();
                        if process_path.starts_with("/Applications/") && process_path.contains(".app/") {
                            if let Some(app_idx) = process_path.find(".app/") {
                                name = process_path[14..app_idx + 4].to_string();
                            }
                        } else if let Some(pos) = process_path.rfind('/') {
                            name = process_path[pos + 1..].to_string();
                        }
                        name
                    } else if !process_name.is_empty() {
                        process_name.clone()
                    } else {
                        continue;
                    };

                    let path_for_key = if !process_path.is_empty() {
                        process_path.clone()
                    } else {
                        format!("<{}>", process_name.as_str())
                    };

                    let is_direct = conn.chains.iter().any(|c| c.eq_ignore_ascii_case("direct"))
                        || conn.rule.eq_ignore_ascii_case("direct");
                    let is_reject = conn.chains.iter().any(|c| c.eq_ignore_ascii_case("reject"))
                        || conn.rule.eq_ignore_ascii_case("reject");

                    let traffic_mode = if is_direct {
                        "直连".to_string()
                    } else if is_reject {
                        "拦截".to_string()
                    } else if format!("{:?}", conn.metadata.connection_type)
                        .to_uppercase()
                        .contains("TUN")
                    {
                        "TUN".to_string()
                    } else {
                        "代理".to_string()
                    };

                    let key = (display_name, traffic_mode, path_for_key);
                    current_connection_stats.insert(conn.id.clone(), (conn.upload, conn.download));

                    let prev = last_connection_stats.get(&conn.id).copied().unwrap_or((0, 0));
                    let delta_up = conn.upload.saturating_sub(prev.0);
                    let delta_down = conn.download.saturating_sub(prev.1);

                    if delta_up > 0 || delta_down > 0 {
                        let entry = deltas.entry(key).or_insert((0, 0));
                        entry.0 += delta_up;
                        entry.1 += delta_down;
                    }
                }

                let mut db_deltas = Vec::new();
                for (key, (up, down)) in deltas {
                    db_deltas.push((key.0, key.1, key.2, up, down));
                }

                if !db_deltas.is_empty()
                    && let Err(e) = insert_traffic_deltas(&db_deltas).await
                {
                    logging!(error, Type::Core, "Failed to insert traffic: {}", e);
                }
            }

            last_connection_stats = current_connection_stats;
        }
    });
}

#[allow(clippy::significant_drop_tightening)]
async fn setup_db() -> anyhow::Result<()> {
    let path = dirs::app_home_dir()?.join("app_traffic.db");
    let conn = Connection::open(&path)?;

    let _ = conn.execute("ALTER TABLE app_traffic ADD COLUMN process_path TEXT DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE app_traffic ADD COLUMN traffic_mode TEXT DEFAULT ''", []);

    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_traffic (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            process_name TEXT NOT NULL,
            process_path TEXT DEFAULT '',
            traffic_mode TEXT DEFAULT '',
            upload_bytes INTEGER NOT NULL,
            download_bytes INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_timestamp ON app_traffic (timestamp)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS global_traffic (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            upload_bytes INTEGER NOT NULL,
            download_bytes INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_global_timestamp ON global_traffic (timestamp)",
        [],
    )?;

    let mut db_guard = DB_CONN.lock().await;
    *db_guard = Some(conn);
    Ok(())
}

#[allow(clippy::significant_drop_tightening)]
async fn insert_traffic_deltas(deltas: &[(String, String, String, u64, u64)]) -> anyhow::Result<()> {
    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO app_traffic (process_name, traffic_mode, process_path, upload_bytes, download_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (process_name, traffic_mode, process_path, up, down) in deltas {
                stmt.execute(params![
                    process_name,
                    traffic_mode,
                    process_path,
                    *up as i64,
                    *down as i64
                ])?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

#[allow(clippy::significant_drop_tightening)]
async fn insert_global_traffic(upload_bytes: u64, download_bytes: u64) -> anyhow::Result<()> {
    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        conn.execute(
            "INSERT INTO global_traffic (upload_bytes, download_bytes) VALUES (?1, ?2)",
            params![upload_bytes as i64, download_bytes as i64],
        )?;
    }
    Ok(())
}

#[allow(clippy::significant_drop_tightening)]
pub async fn query_traffic(period: &str) -> anyhow::Result<Vec<AppTrafficStat>> {
    use chrono::{Datelike as _, Local, NaiveTime, TimeZone as _, Utc};

    let now = Local::now();
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default();
    let start_local = match period {
        "day" => now.date_naive().and_time(midnight),
        "week" => {
            let days_since_monday = now.weekday().num_days_from_monday();
            let monday = now.date_naive() - chrono::Duration::days(days_since_monday as i64);
            monday.and_time(midnight)
        }
        "month" => {
            let first_of_month = now.date_naive().with_day(1).unwrap_or_else(|| now.date_naive());
            first_of_month.and_time(midnight)
        }
        _ => now.date_naive().and_time(midnight),
    };

    let start_utc = Local
        .from_local_datetime(&start_local)
        .single()
        .unwrap_or_else(|| Utc::now().with_timezone(&Local))
        .with_timezone(&Utc);
    let start_str = start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        let mut stmt = conn.prepare(
            "SELECT process_name, process_path, traffic_mode, SUM(upload_bytes), SUM(download_bytes)
             FROM app_traffic
             WHERE timestamp >= ?1
             GROUP BY process_name, process_path, traffic_mode
             ORDER BY SUM(download_bytes) DESC",
        )?;
        let rows = stmt.query_map(params![start_str], |row| {
            Ok(AppTrafficStat {
                process_name: row.get(0)?,
                process_path: row.get(1)?,
                traffic_mode: row.get(2)?,
                upload_bytes: row.get::<_, i64>(3)? as u64,
                download_bytes: row.get::<_, i64>(4)? as u64,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        return Ok(results);
    }
    Ok(vec![])
}

#[allow(clippy::significant_drop_tightening)]
pub async fn query_global_traffic(period: &str) -> anyhow::Result<Option<GlobalTrafficStat>> {
    use chrono::{Datelike as _, Local, NaiveTime, TimeZone as _, Utc};

    let now = Local::now();
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default();
    let start_local = match period {
        "day" => now.date_naive().and_time(midnight),
        "week" => {
            let days_since_monday = now.weekday().num_days_from_monday();
            let monday = now.date_naive() - chrono::Duration::days(days_since_monday as i64);
            monday.and_time(midnight)
        }
        "month" => {
            let first_of_month = now.date_naive().with_day(1).unwrap_or_else(|| now.date_naive());
            first_of_month.and_time(midnight)
        }
        _ => now.date_naive().and_time(midnight),
    };

    let start_utc = Local
        .from_local_datetime(&start_local)
        .single()
        .unwrap_or_else(|| Utc::now().with_timezone(&Local))
        .with_timezone(&Utc);
    let start_str = start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        let start_query = "SELECT upload_bytes, download_bytes FROM global_traffic
                           WHERE timestamp >= ?1 ORDER BY timestamp ASC LIMIT 1";
        let mut start_stmt = conn.prepare(start_query)?;
        let start_row = start_stmt.query_row(params![start_str], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
        });

        let end_query = "SELECT upload_bytes, download_bytes FROM global_traffic
                         ORDER BY timestamp DESC LIMIT 1";
        let mut end_stmt = conn.prepare(end_query)?;
        let end_row = end_stmt.query_row([], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
        });

        match (start_row, end_row) {
            (Ok((start_up, start_down)), Ok((end_up, end_down))) => {
                return Ok(Some(GlobalTrafficStat {
                    upload_bytes: end_up.saturating_sub(start_up),
                    download_bytes: end_down.saturating_sub(start_down),
                }));
            }
            _ => return Ok(None),
        }
    }
    Ok(None)
}

#[allow(clippy::significant_drop_tightening)]
pub async fn clear_traffic() -> anyhow::Result<()> {
    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        conn.execute("DELETE FROM app_traffic", [])?;
        conn.execute("DELETE FROM global_traffic", [])?;
    }
    Ok(())
}
