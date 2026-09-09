use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use clash_verge_logging::{Type, logging};
use serde::Deserialize;
use serde_json::Value;
use tauri_plugin_mihomo::models::{LogLevel, WebSocketMessage};
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};

use super::{CoreManager, handle::Handle, logger::Logger, manager::RunningMode, service};

static STARTED: AtomicBool = AtomicBool::new(false);
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct CoreLog {
    #[serde(rename = "type")]
    level: String,
    payload: String,
}

fn log_message(data: Value) -> Result<Option<String>> {
    let entry = match serde_json::from_value::<CoreLog>(data.clone()) {
        Ok(entry) => entry,
        Err(_) => match serde_json::from_value::<WebSocketMessage>(data)? {
            WebSocketMessage::Text(text) => serde_json::from_str::<CoreLog>(&text)
                .context("invalid core log message or WebSocket transport error")?,
            WebSocketMessage::Close(_) => bail!("core log stream closed"),
            _ => return Ok(None),
        },
    };
    // Quote payloads so embedded newlines cannot masquerade as separate log records.
    Ok(Some(format!(
        "level={} msg={}",
        serde_json::to_string(&entry.level)?,
        serde_json::to_string(&entry.payload)?,
    )))
}

fn service_is_running() -> bool {
    service::has_active_service_session() && matches!(*CoreManager::global().get_running_mode(), RunningMode::Service)
}

/// One backend subscription for the app lifetime, independent of WebView subscriptions.
pub(super) fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::process::AsyncHandler::spawn(|| async {
        let mut connection_failed = false;
        while !Handle::global().is_exiting() {
            if !service_is_running() {
                sleep(RETRY_INTERVAL).await;
                continue;
            }

            let (closed_tx, mut closed_rx) = watch::channel(());
            let write_failed = AtomicBool::new(false);
            let connection: Result<_> = timeout(Duration::from_secs(5), async {
                Handle::mihomo()
                    .await
                    .ws_logs(LogLevel::DEBUG, move |data| match log_message(data) {
                        Ok(Some(message)) => match Logger::global().write_service_log(&message) {
                            Ok(()) => write_failed.store(false, Ordering::Relaxed),
                            Err(error) => {
                                if !write_failed.swap(true, Ordering::Relaxed) {
                                    logging!(error, Type::Service, "Failed to persist core log stream: {error:#}");
                                }
                            }
                        },
                        Ok(None) => {}
                        Err(_) => {
                            let _ = closed_tx.send(());
                        }
                    })
                    .await
            })
            .await
            .context("core log connection timed out")
            .and_then(|result| result.map_err(Into::into));

            match connection {
                Ok(id) => {
                    connection_failed = false;
                    // The service's authenticated memory buffer includes startup output produced before
                    // its API was ready. On reconnection it can also recover recent disconnected output;
                    // those records may overlap with the live stream and retain their core timestamps.
                    match service::get_clash_logs_by_service().await {
                        Ok(logs) => {
                            for line in logs {
                                if let Err(error) = Logger::global().write_service_log(&line) {
                                    logging!(warn, Type::Service, "Failed to persist recent core logs: {error:#}");
                                    break;
                                }
                            }
                        }
                        Err(error) => logging!(warn, Type::Service, "Failed to recover recent core logs: {error:#}"),
                    }

                    loop {
                        tokio::select! {
                            _ = closed_rx.changed() => break,
                            _ = sleep(RETRY_INTERVAL) => {
                                if Handle::global().is_exiting() || !service_is_running()
                                    || !Handle::mihomo().await.connection_manager.0.read().await.contains_key(&id) {
                                    break;
                                }
                            }
                        }
                    }
                    if let Err(error) = Handle::mihomo().await.disconnect(id, Some(1000)).await {
                        logging!(debug, Type::Service, "Failed to close core log stream: {error}");
                    }
                }
                Err(error) => {
                    if !connection_failed {
                        logging!(warn, Type::Service, "Core log stream unavailable, retrying: {error}");
                        connection_failed = true;
                    }
                }
            }
            sleep(RETRY_INTERVAL).await;
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_raw_and_wrapped_logs_without_losing_warning_payloads() {
        let raw = json!({"type": "warning", "payload": "[TCP] 国内网站: no route to host\nnext line"});
        let wrapped = serde_json::to_value(WebSocketMessage::Text(raw.to_string())).unwrap();
        let expected = "level=\"warning\" msg=\"[TCP] 国内网站: no route to host\\nnext line\"";
        assert_eq!(log_message(raw).unwrap().as_deref(), Some(expected));
        assert_eq!(log_message(wrapped).unwrap().as_deref(), Some(expected));
    }

    #[test]
    fn ignores_control_frames_and_reconnects_on_close_or_transport_errors() {
        let ping = serde_json::to_value(WebSocketMessage::Ping(vec![])).unwrap();
        assert!(log_message(ping).unwrap().is_none());
        let close = serde_json::to_value(WebSocketMessage::Close(None)).unwrap();
        assert!(log_message(close).is_err());
        let error = serde_json::to_value(WebSocketMessage::Text("Websocket error: reset".into())).unwrap();
        assert!(log_message(error).is_err());
    }
}
