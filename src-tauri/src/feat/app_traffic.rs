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

#[cfg(target_os = "macos")]
#[allow(clippy::type_complexity)]
static LSOF_CACHE: Lazy<Arc<Mutex<HashMap<String, (String, String)>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

#[cfg(not(target_os = "macos"))]
async fn resolve_process_by_port(_port: &str) -> Option<(String, String)> {
    None
}

#[cfg(target_os = "macos")]
async fn resolve_process_by_port(port: &str) -> Option<(String, String)> {
    if port.is_empty() {
        return None;
    }

    {
        let cache = LSOF_CACHE.lock().await;
        if let Some(result) = cache.get(port) {
            return Some(result.clone());
        }
    }

    let output = tokio::process::Command::new("lsof")
        .args(["-i", &format!(":{}", port), "-n", "-P", "-Fpcn"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current_pid = String::new();
    let mut current_cmd = String::new();

    for line in stdout.lines() {
        if let Some(stripped) = line.strip_prefix('p') {
            current_pid = stripped.to_string();
        } else if let Some(stripped) = line.strip_prefix('c') {
            current_cmd = stripped.to_string();
        } else if line.starts_with('n') && !current_pid.is_empty() && !current_cmd.is_empty() {
            if let Ok(ps_output) = tokio::process::Command::new("ps")
                .args(["-p", &current_pid, "-o", "args="])
                .output()
                .await
            {
                let args = String::from_utf8_lossy(&ps_output.stdout);
                let path = args.lines().next().unwrap_or("").trim();
                if !path.is_empty() && path.starts_with('/') {
                    let name = if path.starts_with("/Applications/") && path.contains(".app/") {
                        if let Some(idx) = path.find(".app/") {
                            path[14..idx + 4].to_string()
                        } else {
                            current_cmd.clone()
                        }
                    } else if let Some(pos) = path.rfind('/') {
                        path[pos + 1..].to_string()
                    } else {
                        current_cmd.clone()
                    };

                    let result = (name, path.to_string());
                    LSOF_CACHE.lock().await.insert(port.to_string(), result.clone());
                    return Some(result);
                }
            }

            let result = (current_cmd.clone(), format!("<{}>", current_cmd));
            LSOF_CACHE.lock().await.insert(port.to_string(), result.clone());
            return Some(result);
        }
    }

    None
}

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

#[derive(Debug, Serialize, Deserialize)]
pub struct AppTrafficDomainStat {
    pub domain: String,
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
        let mut last_global_total: (u64, u64) = (0, 0);
        let mut current_interval = MIN_POLL_INTERVAL;
        let mut consecutive_failures: u32 = 0;
        let mut is_first_poll = true;

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
            // 计算全局流量增量（与 per-app 统计保持一致）
            // 核心重启后 total 归零，delta 为 0（saturating_sub），不会产生错误数据
            let delta_global_up = global_up.saturating_sub(last_global_total.0);
            let delta_global_down = global_down.saturating_sub(last_global_total.1);
            last_global_total = (global_up, global_down);

            // 首次轮询仅建立基准，不写入数据库，避免 burst
            if !is_first_poll
                && (delta_global_up > 0 || delta_global_down > 0)
                && let Err(e) = insert_global_traffic(delta_global_up, delta_global_down).await
            {
                logging!(error, Type::Core, "Failed to insert global traffic: {}", e);
            }

            let mut current_connection_stats: HashMap<String, (u64, u64)> = HashMap::new();
            let mut deltas: HashMap<(String, String, String), (u64, u64)> = HashMap::new();
            let mut domain_deltas: HashMap<(String, String, String), (u64, u64)> = HashMap::new();

            if let Some(conns) = connections.connections {
                for conn in conns {
                    let process_path = &conn.metadata.process_path;
                    let process_name = &conn.metadata.process;
                    let host = &conn.metadata.host;
                    let remote_destination = &conn.metadata.remote_destination;

                    let lsof_fallback = if process_path.is_empty() && process_name.is_empty() {
                        resolve_process_by_port(&conn.metadata.source_port).await
                    } else {
                        None
                    };

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
                    } else if let Some((ref name, _)) = lsof_fallback {
                        name.clone()
                    } else if !host.is_empty() {
                        host.clone()
                    } else if !remote_destination.is_empty() {
                        remote_destination.clone()
                    } else {
                        continue;
                    };

                    let path_for_key = if !process_path.is_empty() {
                        process_path.clone()
                    } else if !process_name.is_empty() {
                        format!("<{}>", process_name.as_str())
                    } else if let Some((_, ref path)) = lsof_fallback {
                        path.clone()
                    } else if !host.is_empty() {
                        format!("[{}]", host.as_str())
                    } else {
                        format!("[{}]", remote_destination.as_str())
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

                    let key = (display_name, traffic_mode.clone(), path_for_key.clone());
                    current_connection_stats.insert(conn.id.clone(), (conn.upload, conn.download));

                    let prev = last_connection_stats.get(&conn.id).copied().unwrap_or((0, 0));
                    let delta_up = conn.upload.saturating_sub(prev.0);
                    let delta_down = conn.download.saturating_sub(prev.1);

                    if delta_up > 0 || delta_down > 0 {
                        let entry = deltas.entry(key).or_insert((0, 0));
                        entry.0 += delta_up;
                        entry.1 += delta_down;

                        let domain = if !host.is_empty() {
                            host.clone()
                        } else {
                            remote_destination.clone()
                        };
                        let domain_key = (path_for_key.clone(), traffic_mode.clone(), domain);
                        let domain_entry = domain_deltas.entry(domain_key).or_insert((0, 0));
                        domain_entry.0 += delta_up;
                        domain_entry.1 += delta_down;
                    }
                }

                // 首次轮询仅建立基准，不写入数据库
                if !is_first_poll {
                    let mut db_deltas = Vec::new();
                    for (key, (up, down)) in deltas {
                        db_deltas.push((key.0, key.1, key.2, up, down));
                    }

                    if !db_deltas.is_empty()
                        && let Err(e) = insert_traffic_deltas(&db_deltas).await
                    {
                        logging!(error, Type::Core, "Failed to insert traffic: {}", e);
                    }

                    let mut db_domain_deltas = Vec::new();
                    for (key, (up, down)) in domain_deltas {
                        db_domain_deltas.push((key.0, key.1, key.2, up, down));
                    }

                    if !db_domain_deltas.is_empty()
                        && let Err(e) = insert_domain_deltas(&db_domain_deltas).await
                    {
                        logging!(error, Type::Core, "Failed to insert domain traffic: {}", e);
                    }
                }
            }

            last_connection_stats = current_connection_stats;
            is_first_poll = false;
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

    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_traffic_domain (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            process_path TEXT NOT NULL,
            traffic_mode TEXT DEFAULT '',
            domain TEXT NOT NULL,
            upload_bytes INTEGER NOT NULL,
            download_bytes INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_domain_timestamp ON app_traffic_domain (timestamp)",
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
async fn insert_domain_deltas(deltas: &[(String, String, String, u64, u64)]) -> anyhow::Result<()> {
    let mut db_guard = DB_CONN.lock().await;
    if let Some(conn) = db_guard.as_mut() {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO app_traffic_domain (process_path, traffic_mode, domain, upload_bytes, download_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (process_path, traffic_mode, domain, up, down) in deltas {
                stmt.execute(params![process_path, traffic_mode, domain, *up as i64, *down as i64])?;
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
pub async fn query_traffic_detail(
    process_path: &str,
    traffic_mode: &str,
    period: &str,
) -> anyhow::Result<Vec<AppTrafficDomainStat>> {
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
            "SELECT domain, SUM(upload_bytes), SUM(download_bytes)
             FROM app_traffic_domain
             WHERE process_path = ?1 AND traffic_mode = ?2 AND timestamp >= ?3
             GROUP BY domain
             ORDER BY SUM(download_bytes) DESC",
        )?;
        let rows = stmt.query_map(params![process_path, traffic_mode, start_str], |row| {
            Ok(AppTrafficDomainStat {
                domain: row.get(0)?,
                upload_bytes: row.get::<_, i64>(1)? as u64,
                download_bytes: row.get::<_, i64>(2)? as u64,
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
        // 现在 global_traffic 存储的是增量值，直接 SUM 即可
        // 不再受核心重启导致累计值归零的影响
        let query = "SELECT SUM(upload_bytes), SUM(download_bytes) FROM global_traffic
                     WHERE timestamp >= ?1";
        let mut stmt = conn.prepare(query)?;
        let result = stmt.query_row(params![start_str], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.unwrap_or(0) as u64,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
            ))
        });

        match result {
            Ok((up, down)) if up > 0 || down > 0 => {
                return Ok(Some(GlobalTrafficStat {
                    upload_bytes: up,
                    download_bytes: down,
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
        conn.execute("DELETE FROM app_traffic_domain", [])?;
        conn.execute("DELETE FROM global_traffic", [])?;
    }
    Ok(())
}
