use super::{CoreManager, RunningMode};
use crate::cmd::StringifyErr as _;
use crate::config::{Config, IVerge};
use crate::core::handle::Handle;
use crate::core::manager::CLASH_LOGGER;
use crate::core::service;
use crate::core::service::{SERVICE_MANAGER, ServiceStatus};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use tauri_plugin_clash_verge_sysinfo;

impl CoreManager {
    pub async fn start_core(&self) -> Result<()> {
        self.prepare_startup().await?;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => match self.start_core_by_service().await {
                Ok(()) => Ok(()),
                Err(e) => {
                    logging!(warn, Type::Core, "Service mode failed ({}), falling back to sidecar", e);
                    self.set_running_mode(RunningMode::Sidecar);
                    self.start_core_by_sidecar().await
                }
            },
            RunningMode::NotRunning | RunningMode::Sidecar => self.start_core_by_sidecar().await,
        }
    }

    pub async fn stop_core(&self) -> Result<()> {
        CLASH_LOGGER.clear_logs().await;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => self.stop_core_by_service().await,
            RunningMode::Sidecar => {
                self.stop_core_by_sidecar();
                Ok(())
            }
            RunningMode::NotRunning => Ok(()),
        }
    }

    pub async fn restart_core(&self) -> Result<()> {
        logging!(info, Type::Core, "Restarting core");
        self.stop_core().await?;
        self.start_core().await
    }

    pub async fn change_core(&self, clash_core: &String) -> Result<(), String> {
        if !IVerge::VALID_CLASH_CORES.contains(&clash_core.as_str()) {
            return Err(format!("Invalid clash core: {}", clash_core).into());
        }

        Config::verge().await.edit_draft(|d| {
            d.clash_core = Some(clash_core.to_owned());
        });
        Config::verge().await.apply();

        let verge_data = Config::verge().await.latest_arc();
        verge_data.save_file().await.map_err(|e| e.to_string())?;

        self.update_config_checked().await.stringify_err()?;
        Ok(())
    }

    /// 启动前准备：决定以什么模式启动核心
    ///
    /// 策略说明：
    /// - 如果服务已就绪（IPC 可连接），优先使用服务模式（无论 TUN 是否开启）
    /// - 如果服务不可用，回退到 Sidecar 模式
    /// - macOS 上额外的 TUN 特判：TUN 开启时会更积极地初始化服务
    async fn prepare_startup(&self) -> Result<()> {
        let mut manager = SERVICE_MANAGER.lock().await;
        let current = manager.current();

        // 如果服务管理器已经标记为 Ready，直接使用服务模式
        if matches!(current, ServiceStatus::Ready) {
            self.set_running_mode(RunningMode::Service);
            drop(manager);
            logging!(info, Type::Core, "服务已就绪，使用 Service 模式启动");
            return Ok(());
        }

        // 服务不是 Ready 状态，尝试初始化
        if service::is_service_ipc_path_exists() {
            logging!(info, Type::Core, "发现服务 IPC，尝试初始化服务管理器");
            if manager.init().await.is_ok() {
                let _ = manager.refresh().await;
            }
        }

        // TUN 开启时额外尝试等待服务启动
        {
            let needs_tun = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
            if needs_tun && !matches!(manager.current(), ServiceStatus::Ready) {
                logging!(info, Type::Core, "TUN 模式需要服务，等待服务 IPC 就绪...");
                // 给 LaunchDaemon 最多 12 秒启动时间（扩展等待）
                let _ = service::wait_and_check_service_available_extended(&mut manager).await;
            }
        }

        let mode = match manager.current() {
            ServiceStatus::Ready => {
                logging!(info, Type::Core, "使用 Service 模式启动");
                RunningMode::Service
            }
            _ => {
                logging!(info, Type::Core, "服务不可用，使用 Sidecar 模式启动");
                RunningMode::Sidecar
            }
        };

        self.set_running_mode(mode);
        drop(manager);
        Ok(())
    }

    fn after_core_process(&self) {
        let app_handle = Handle::app_handle();
        tauri_plugin_clash_verge_sysinfo::set_app_core_mode(app_handle, self.get_running_mode().to_string());
    }

    #[cfg(target_os = "windows")]
    async fn wait_for_service_if_needed(&self) {
        use crate::{config::Config, constants::timing, core::service};
        use backon::{ConstantBuilder, Retryable as _};

        let needs_service = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);

        if !needs_service {
            return;
        }

        let max_times = timing::SERVICE_WAIT_MAX.as_millis() / timing::SERVICE_WAIT_INTERVAL.as_millis();
        let backoff = ConstantBuilder::default()
            .with_delay(timing::SERVICE_WAIT_INTERVAL)
            .with_max_times(max_times as usize);

        let _ = (|| async {
            let mut manager = SERVICE_MANAGER.lock().await;

            if matches!(manager.current(), ServiceStatus::Ready) {
                return Ok(());
            }

            // If the service IPC path is not ready yet, treat it as transient and retry.
            // Running init/refresh too early can mark service state unavailable and break later config reloads.
            if !service::is_service_ipc_path_exists() {
                return Err(anyhow::anyhow!("Service IPC not ready"));
            }

            manager.init().await?;
            let _ = manager.refresh().await;

            if matches!(manager.current(), ServiceStatus::Ready) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("Service not ready"))
            }
        })
        .retry(backoff)
        .await;
    }
}
