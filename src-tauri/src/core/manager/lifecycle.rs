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

const fn should_wait_for_service_startup(
    needs_tun: bool,
    service_ipc_path_exists: bool,
    status: &ServiceStatus,
) -> bool {
    needs_tun && service_ipc_path_exists && matches!(status, ServiceStatus::Checking)
}

const fn should_probe_existing_service(status: &ServiceStatus) -> bool {
    matches!(status, ServiceStatus::Checking)
}

const fn can_fallback_to_sidecar(needs_tun: bool, is_admin: bool) -> bool {
    !needs_tun || is_admin
}

impl CoreManager {
    pub async fn start_core(&self) -> Result<()> {
        let _operation = self.operation_lock.lock().await;
        self.start_core_unlocked().await
    }

    pub(super) async fn start_core_unlocked(&self) -> Result<()> {
        self.prepare_startup().await?;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => match self.start_core_by_service().await {
                Ok(()) => Ok(()),
                Err(e) => {
                    logging!(warn, Type::Core, "Service mode failed: {}", e);
                    if service::has_active_service_session() {
                        return Err(e.context("Service 核心会话仍处于活动状态；为避免双核心冲突，已停止自动回退"));
                    }
                    if Handle::mihomo().await.get_version().await.is_ok() {
                        return Err(e.context("Mihomo API 仍可访问但 Service 会话状态未知；已停止自动回退"));
                    }
                    let needs_tun = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
                    let is_admin = tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin(Handle::app_handle());
                    if !can_fallback_to_sidecar(needs_tun, is_admin) {
                        self.set_running_mode(RunningMode::NotRunning);
                        return Err(
                            e.context("TUN 模式要求可用的 macOS Service；为避免无权限 Sidecar 假启动，已停止自动回退")
                        );
                    }
                    logging!(warn, Type::Core, "Falling back to sidecar mode");
                    self.set_running_mode(RunningMode::Sidecar);
                    self.start_core_by_sidecar().await
                }
            },
            RunningMode::NotRunning | RunningMode::Sidecar => self.start_core_by_sidecar().await,
        }
    }

    pub async fn stop_core(&self) -> Result<()> {
        let _operation = self.operation_lock.lock().await;
        self.stop_core_unlocked().await
    }

    pub(super) async fn stop_core_unlocked(&self) -> Result<()> {
        CLASH_LOGGER.clear_logs().await;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => self.stop_core_by_service().await,
            RunningMode::Sidecar => self.stop_core_by_sidecar().await,
            RunningMode::NotRunning => Ok(()),
        }
    }

    pub async fn restart_core(&self) -> Result<()> {
        let _operation = self.operation_lock.lock().await;
        self.restart_core_unlocked().await
    }

    pub(super) async fn restart_core_unlocked(&self) -> Result<()> {
        logging!(info, Type::Core, "Restarting core");
        if let Err(error) = self.stop_core_unlocked().await {
            if !service::is_stale_owner_session_error(&error) {
                return Err(error);
            }

            // The Service has already replaced or restarted its owner session.  Starting through
            // the Service is a transactional takeover: it stops the old core before creating the
            // replacement, so it is safe to reacquire ownership instead of leaving TUN offline.
            logging!(
                warn,
                Type::Service,
                "Service owner session is stale during restart; reacquiring the session"
            );
            service::clear_active_service_session();
            return self.start_core_unlocked().await;
        }
        self.start_core_unlocked().await
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
        let service_ipc_path_exists = service::is_service_ipc_path_exists();
        if service_ipc_path_exists && should_probe_existing_service(&current) {
            logging!(info, Type::Core, "发现服务 IPC，尝试初始化服务管理器");
            if manager.init().await.is_ok() {
                let _ = manager.refresh().await;
            }
        }

        // TUN 开启时额外尝试等待服务启动
        {
            let needs_tun = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
            if should_wait_for_service_startup(needs_tun, service_ipc_path_exists, &manager.current()) {
                logging!(info, Type::Core, "TUN 模式需要服务，等待服务 IPC 就绪...");
                // 仅在状态尚未解析时等待。连接已被拒绝时必须立即回退，避免陈旧
                // socket 阻塞核心启动；安装/修复流程会自行等待服务 IPC 就绪。
                let _ = service::wait_and_check_service_available_extended(&mut manager).await;
            }

            let is_admin = tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin(Handle::app_handle());
            if needs_tun && !is_admin && !matches!(manager.current(), ServiceStatus::Ready) {
                logging!(
                    warn,
                    Type::Core,
                    "TUN requires an elevated process or a compatible service; disabling unavailable TUN mode"
                );
                let verge = Config::verge().await;
                verge.edit_draft(|draft| draft.enable_tun_mode = Some(false));
                verge.apply();
                let verge_data = Config::verge().await.latest_arc();
                verge_data.save_file().await?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_waits_for_an_unresolved_existing_service() {
        assert!(should_wait_for_service_startup(true, true, &ServiceStatus::Checking));
        assert!(!should_wait_for_service_startup(
            true,
            true,
            &ServiceStatus::Unavailable("connection refused".into())
        ));
        assert!(!should_wait_for_service_startup(true, false, &ServiceStatus::Checking));
        assert!(!should_wait_for_service_startup(
            true,
            true,
            &ServiceStatus::NotInstalled
        ));
        assert!(!should_wait_for_service_startup(false, true, &ServiceStatus::Checking));
    }

    #[test]
    fn only_probes_an_unresolved_service() {
        assert!(should_probe_existing_service(&ServiceStatus::Checking));
        assert!(!should_probe_existing_service(&ServiceStatus::Unavailable(
            "connection refused".into()
        )));
    }

    #[test]
    fn non_admin_tun_never_falls_back_to_sidecar() {
        assert!(!can_fallback_to_sidecar(true, false));
        assert!(can_fallback_to_sidecar(false, false));
        assert!(can_fallback_to_sidecar(true, true));
    }
}
