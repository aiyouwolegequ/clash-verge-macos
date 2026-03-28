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

#[derive(Debug, Serialize, Deserialize)]
pub struct AppTrafficStat {
    pub process_name: String,
    pub process_path: String,
    pub traffic_mode: String,
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

        loop {
            sleep(Duration::from_secs(5)).await;

            let connections = {
                let mihomo = handle::Handle::mihomo().await;
                match mihomo.get_connections().await {
                    Ok(c) => c,
                    Err(e) => {
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

            let mut current_connection_stats: HashMap<String, (u64, u64)> = HashMap::new();
            let mut deltas: HashMap<(String, String, String), (u64, u64)> = HashMap::new();

            if let Some(conns) = connections.connections {
                for conn in conns {
                    let process_path = conn.metadata.process_path;
                    if process_path.is_empty() {
                        continue;
                    }

                    let mut display_name = process_path.clone();
                    if process_path.starts_with("/Applications/") && process_path.contains(".app/") {
                        if let Some(app_idx) = process_path.find(".app/") {
                            display_name = process_path[14..app_idx + 4].to_string();
                        }
                    } else if let Some(pos) = process_path.rfind('/') {
                        display_name = process_path[pos + 1..].to_string();
                    }

                    let is_direct = conn.chains.iter().any(|c| c.eq_ignore_ascii_case("direct"))
                        || conn.rule.eq_ignore_ascii_case("direct");
                    let is_reject = conn.chains.iter().any(|c| c.eq_ignore_ascii_case("reject"))
                        || conn.rule.eq_ignore_ascii_case("reject");

                    let traffic_mode = if is_direct {
                        "直连".to_string()
                    } else if is_reject {
                        "拦截".to_string()
                    } else if format!("{:?}", conn.metadata.connection_type).eq_ignore_ascii_case("tun") {
                        "TUN".to_string()
                    } else {
                        "代理".to_string()
                    };

                    let key = (display_name, traffic_mode, process_path);
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

    // Add columns dynamically for previously created databases
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
pub async fn query_traffic(period: &str) -> anyhow::Result<Vec<AppTrafficStat>> {
    use chrono::{Datelike as _, Local, NaiveTime, TimeZone as _, Utc};

    let now = Local::now();
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default();
    let start_local = match period {
        "day" => {
            // 今日 00:00:00
            now.date_naive().and_time(midnight)
        }
        "week" => {
            // 本周一 00:00:00（周一=0）
            let days_since_monday = now.weekday().num_days_from_monday();
            let monday = now.date_naive() - chrono::Duration::days(days_since_monday as i64);
            monday.and_time(midnight)
        }
        "month" => {
            // 本月 1 日 00:00:00
            let first_of_month = now.date_naive().with_day(1).unwrap_or_else(|| now.date_naive());
            first_of_month.and_time(midnight)
        }
        _ => now.date_naive().and_time(midnight),
    };

    // 将本地日历边界时间转换为 UTC 字符串（SQLite CURRENT_TIMESTAMP 存的是 UTC）
    let start_utc = Local
        .from_local_datetime(&start_local)
        .single()
        .unwrap_or_else(|| Utc::now().with_timezone(&Local))
        .with_timezone(&Utc);
    let start_str = start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        let query = "SELECT process_name, process_path, traffic_mode, SUM(upload_bytes), SUM(download_bytes) 
             FROM app_traffic 
             WHERE timestamp >= ?1
             GROUP BY process_name, process_path, traffic_mode 
             ORDER BY SUM(download_bytes) DESC";

        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map(params![start_str], |row| {
            let up: i64 = row.get(3)?;
            let down: i64 = row.get(4)?;
            Ok(AppTrafficStat {
                process_name: row.get(0)?,
                process_path: row.get(1)?,
                traffic_mode: row.get(2)?,
                upload_bytes: up as u64,
                download_bytes: down as u64,
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
pub async fn clear_traffic() -> anyhow::Result<()> {
    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        conn.execute("DELETE FROM app_traffic", [])?;
    }
    drop(db_guard);
    Ok(())
}
