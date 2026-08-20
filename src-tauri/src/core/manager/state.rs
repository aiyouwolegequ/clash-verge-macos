use super::{CoreManager, RunningMode};
use crate::{
    AsyncHandler,
    config::{Config, IClashTemp},
    core::{handle, logger::Logger, manager::CLASH_LOGGER, service},
    logging,
    utils::dirs,
};
use anyhow::{Context as _, Result};
use clash_verge_logging::Type;
use compact_str::CompactString;
use log::Level;
use std::{future::Future, time::Duration};
use tauri_plugin_shell::ShellExt as _;

const CORE_API_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const CORE_API_PROBE_INTERVAL: Duration = Duration::from_millis(150);
const CORE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const TUN_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

enum TunStartupState {
    Pending,
    Ready,
    Failed(CompactString),
}

fn inspect_tun_startup_logs(logs: &[CompactString]) -> TunStartupState {
    for log in logs.iter().rev() {
        let normalized = log.to_ascii_lowercase();
        if normalized.contains("start tun listening error") || normalized.contains("configure tun interface") {
            return TunStartupState::Failed(log.clone());
        }
        if normalized.contains("[tun] tun adapter listening at:") {
            return TunStartupState::Ready;
        }
    }
    TunStartupState::Pending
}

async fn wait_for_tun_ready_with<F, Fut>(mut logs: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<CompactString>>>,
{
    let mut last_error = None;
    let outcome = tokio::time::timeout(TUN_STARTUP_TIMEOUT, async {
        loop {
            match logs().await {
                Ok(logs) => match inspect_tun_startup_logs(&logs) {
                    TunStartupState::Ready => return Ok(()),
                    TunStartupState::Failed(message) => anyhow::bail!("TUN 启动失败: {message}"),
                    TunStartupState::Pending => {}
                },
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(CORE_API_PROBE_INTERVAL).await;
        }
    })
    .await;

    match outcome {
        Ok(result) => result,
        Err(_) => {
            let detail = last_error.map_or_else(|| "未观察到 TUN 就绪日志".to_owned(), |error| format!("{error:#}"));
            Err(anyhow::anyhow!(
                "TUN 未在 {} 秒内确认就绪: {detail}",
                TUN_STARTUP_TIMEOUT.as_secs()
            ))
        }
    }
}

async fn wait_for_core_api_ready_with<F, Fut>(mut probe: F, timeout: Duration, interval: Duration) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut last_error = None;
    let outcome = tokio::time::timeout(timeout, async {
        loop {
            match probe().await {
                Ok(()) => return,
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(interval).await;
        }
    })
    .await;

    match outcome {
        Ok(()) => Ok(()),
        Err(_) => {
            let detail = last_error.map_or_else(|| "无探测结果".to_owned(), |error| format!("{error:#}"));
            Err(anyhow::anyhow!(
                "Mihomo API 未在 {} 秒内就绪，最后一次错误: {detail}",
                timeout.as_secs()
            ))
        }
    }
}

async fn wait_for_core_api_ready() -> Result<()> {
    wait_for_core_api_ready_with(
        || async {
            handle::Handle::mihomo()
                .await
                .get_version()
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        },
        CORE_API_STARTUP_TIMEOUT,
        CORE_API_PROBE_INTERVAL,
    )
    .await
}

async fn wait_for_core_api_unavailable() -> Result<()> {
    tokio::time::timeout(CORE_SHUTDOWN_TIMEOUT, async {
        loop {
            if handle::Handle::mihomo().await.get_version().await.is_err() {
                return;
            }
            tokio::time::sleep(CORE_API_PROBE_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("已有 Mihomo 核心仍占用 API socket"))
}

fn process_is_running(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    let result = unsafe { tauri_plugin_clash_verge_sysinfo::libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(tauri_plugin_clash_verge_sysinfo::libc::EPERM)
}

async fn wait_for_process_exit(pid: u32) -> Result<()> {
    tokio::time::timeout(CORE_SHUTDOWN_TIMEOUT, async {
        while process_is_running(pid) {
            tokio::time::sleep(CORE_API_PROBE_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Sidecar 进程 {pid} 未在 {} 秒内退出", CORE_SHUTDOWN_TIMEOUT.as_secs()))
}

impl CoreManager {
    pub async fn get_clash_logs(&self) -> Result<Vec<CompactString>> {
        match *self.get_running_mode() {
            RunningMode::Service => service::get_clash_logs_by_service().await,
            RunningMode::Sidecar => Ok(CLASH_LOGGER.get_logs().await),
            RunningMode::NotRunning => Ok(Vec::new()),
        }
    }

    pub(super) async fn start_core_by_sidecar(&self) -> Result<()> {
        logging!(info, Type::Core, "Starting core in sidecar mode");

        // Kill any stale Mihomo processes to free ports 7897 and 53 before starting.
        {
            use std::process::Command;
            let _ = Command::new("pkill").arg("-f").arg("verge-mihomo").output();
            wait_for_core_api_unavailable().await?;
        }

        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;
        let app_handle = handle::Handle::app_handle();
        let clash_core = Config::verge().await.latest_arc().get_valid_clash_core();
        let config_dir = dirs::app_home_dir()?;

        let previous_mask = unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(0o007) };
        let (mut rx, child) = app_handle
            .shell()
            .sidecar(clash_core.as_str())?
            .args([
                "-d",
                dirs::path_to_str(&config_dir)?,
                "-f",
                dirs::path_to_str(&config_file)?,
                "-ext-ctl-unix",
                &IClashTemp::guard_external_controller_ipc(),
            ])
            .spawn()?;
        unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(previous_mask) };

        let pid = child.pid();
        logging!(trace, Type::Core, "Sidecar started with PID: {}", pid);

        self.set_running_child_sidecar(child);
        self.set_running_mode(RunningMode::Sidecar);

        AsyncHandler::spawn(move || async move {
            while let Some(event) = rx.recv().await {
                match event {
                    tauri_plugin_shell::process::CommandEvent::Stdout(line)
                    | tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                        let message = CompactString::from(&*String::from_utf8_lossy(&line));
                        Logger::global().writer_sidecar_log(Level::Error, &message);
                        CLASH_LOGGER.append_log(message).await;
                    }
                    tauri_plugin_shell::process::CommandEvent::Terminated(term) => {
                        let message = if let Some(code) = term.code {
                            CompactString::from(format!("Process terminated with code: {}", code))
                        } else if let Some(signal) = term.signal {
                            CompactString::from(format!("Process terminated by signal: {}", signal))
                        } else {
                            CompactString::from("Process terminated")
                        };
                        Logger::global().writer_sidecar_log(Level::Info, &message);
                        let manager = Self::global();
                        if manager.get_child_sidecar_pid() == Some(pid) {
                            let _ = manager.take_child_sidecar();
                            manager.set_running_mode(RunningMode::NotRunning);
                            tauri_plugin_clash_verge_sysinfo::set_app_core_mode(
                                handle::Handle::app_handle(),
                                RunningMode::NotRunning.to_string(),
                            );
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });

        if let Err(error) = wait_for_core_api_ready().await {
            let _ = self.stop_core_by_sidecar().await;
            return Err(error.context("Sidecar 已启动，但 Mihomo API 不可用"));
        }

        if Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
            && let Err(error) = wait_for_tun_ready_with(|| async { Ok(CLASH_LOGGER.get_logs().await) }).await
        {
            let _ = self.stop_core_by_sidecar().await;
            return Err(error.context("Sidecar API 已就绪，但 TUN 不可用"));
        }

        logging!(info, Type::Core, "Sidecar Mihomo API 已就绪");
        Ok(())
    }

    pub(super) async fn stop_core_by_sidecar(&self) -> Result<()> {
        logging!(info, Type::Core, "Stopping sidecar");
        if let Some(child) = self.take_child_sidecar() {
            let pid = child.pid();
            let result = child.kill();
            logging!(
                trace,
                Type::Core,
                "Sidecar stopped (PID: {:?}, Result: {:?})",
                pid,
                result
            );
            result.context("发送 Sidecar 终止信号失败")?;
            wait_for_process_exit(pid).await?;
        }
        self.set_running_mode(RunningMode::NotRunning);
        Ok(())
    }

    pub(super) async fn start_core_by_service(&self) -> Result<()> {
        logging!(info, Type::Core, "Starting core in service mode");
        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;
        service::run_core_by_service(&config_file).await?;

        if let Err(error) = wait_for_core_api_ready().await {
            logging!(error, Type::Core, "Service 已接受启动，但 Mihomo API 未就绪: {error:#}");
            if let Err(stop_error) = service::stop_core_by_service().await {
                logging!(warn, Type::Service, "清理未就绪的 Service 核心失败: {stop_error:#}");
            }
            return Err(error.context("Service 已接受启动，但 Mihomo API 不可用"));
        }

        if Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
            && let Err(error) = wait_for_tun_ready_with(service::get_clash_logs_by_service).await
        {
            logging!(error, Type::Core, "Service API 已就绪，但 TUN 未就绪: {error:#}");
            if let Err(stop_error) = service::stop_core_by_service().await {
                logging!(
                    warn,
                    Type::Service,
                    "清理 TUN 启动失败的 Service 核心失败: {stop_error:#}"
                );
            }
            return Err(error.context("Service API 已就绪，但 TUN 不可用"));
        }

        self.set_running_mode(RunningMode::Service);
        logging!(info, Type::Core, "Service Mihomo API 已就绪");
        Ok(())
    }

    pub(super) async fn stop_core_by_service(&self) -> Result<()> {
        logging!(info, Type::Core, "Stopping service");
        service::stop_core_by_service().await?;
        self.set_running_mode(RunningMode::NotRunning);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{TunStartupState, inspect_tun_startup_logs, wait_for_core_api_ready_with};
    use anyhow::anyhow;
    use compact_str::CompactString;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test]
    async fn core_api_probe_retries_until_ready() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let probe_attempts = Arc::clone(&attempts);
        let result = wait_for_core_api_ready_with(
            move || {
                let attempt = probe_attempts.fetch_add(1, Ordering::SeqCst);
                async move { if attempt < 2 { Err(anyhow!("not ready")) } else { Ok(()) } }
            },
            Duration::from_secs(1),
            Duration::ZERO,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn core_api_probe_has_an_outer_deadline() {
        let result = wait_for_core_api_ready_with(
            std::future::pending::<anyhow::Result<()>>,
            Duration::from_millis(10),
            Duration::ZERO,
        )
        .await;

        assert!(result.is_err_and(|error| error.to_string().contains("Mihomo API")));
    }

    #[test]
    fn tun_log_parser_distinguishes_ready_and_failed_startup() {
        let ready = vec![CompactString::from("[TUN] Tun adapter listening at: utun9")];
        assert!(matches!(inspect_tun_startup_logs(&ready), TunStartupState::Ready));

        let failed = vec![CompactString::from(
            "Start TUN listening error: configure tun interface: Connect: operation not permitted",
        )];
        assert!(matches!(inspect_tun_startup_logs(&failed), TunStartupState::Failed(_)));
    }
}
