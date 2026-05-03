use crate::{
    config::{Config, IClashTemp},
    core::{logger::Logger, tray::Tray},
    utils::dirs,
};
use anyhow::{Context as _, Result, anyhow, bail};
use backon::{ConstantBuilder, Retryable as _};
use clash_verge_logging::{Type, logging, logging_error};
use clash_verge_service_ipc::CoreConfig;
use compact_str::CompactString;
use once_cell::sync::Lazy;
use std::{
    borrow::Cow,
    env::current_exe,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ServiceStatus {
    Ready,
    NeedsReinstall,
    InstallRequired,
    UninstallRequired,
    ReinstallRequired,
    ForceReinstallRequired,
    Unavailable(String),
}

#[derive(Clone)]
pub struct ServiceManager(ServiceStatus);

#[cfg(target_os = "windows")]
async fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall.exe");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;
    let status = match level {
        PrivilegeLevel::NotPrivileged => RunasCommand::new(uninstall_path).show(false).status()?,
        _ => StdCommand::new(uninstall_path).creation_flags(0x08000000).status()?,
    };

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn install_service() -> Result<()> {
    use std::process::Output;
    logging!(info, Type::Service, "install service");

    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let binary_path = dirs::service_path()?;
    let install_path = binary_path.with_file_name("clash-verge-service-install.exe");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;
    let output = match level {
        PrivilegeLevel::NotPrivileged => {
            let status = RunasCommand::new(&install_path).show(false).status()?;
            Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        }
        _ => {
            // StdCommand returns Output directly
            StdCommand::new(&install_path).creation_flags(0x08000000).output()?
        }
    };

    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to install service code: {}, details: {}", code, err);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let uninstall_path = tauri::utils::platform::current_exe()?.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let uninstall_shell: String = uninstall_path.to_string_lossy().replace(" ", "\\ ");

    let elevator = crate::utils::help::linux_elevator();
    let status = if linux_running_as_root() {
        StdCommand::new(&uninstall_path).status()?
    } else {
        let result = StdCommand::new(&elevator)
            .arg("sh")
            .arg("-c")
            .arg(&uninstall_shell)
            .status()?;

        // 如果 pkexec 执行失败，回退到 sudo
        if !result.success() && elevator.contains("pkexec") {
            logging!(
                warn,
                Type::Service,
                "pkexec failed with code {}, falling back to sudo",
                result.code().unwrap_or(-1)
            );
            StdCommand::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg(&uninstall_shell)
                .status()?
        } else {
            result
        }
    };
    logging!(
        info,
        Type::Service,
        "uninstall status code:{}",
        status.code().unwrap_or(-1)
    );

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn install_service() -> Result<()> {
    logging!(info, Type::Service, "install service");

    let install_path = tauri::utils::platform::current_exe()?.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let install_shell: String = install_path.to_string_lossy().replace(" ", "\\ ");

    let elevator = crate::utils::help::linux_elevator();
    let output = if linux_running_as_root() {
        StdCommand::new(&install_path).output()?
    } else {
        let result = StdCommand::new(&elevator)
            .arg("sh")
            .arg("-c")
            .arg(&install_shell)
            .output()?;

        // 如果 pkexec 执行失败，回退到 sudo
        if !result.status.success() && elevator.contains("pkexec") {
            logging!(
                warn,
                Type::Service,
                "pkexec failed with code {}, falling back to sudo",
                result.status.code().unwrap_or(-1)
            );
            StdCommand::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg(&install_shell)
                .output()?
        } else {
            result
        }
    };

    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to install service code: {}, details: {}", code, err);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_running_as_root() -> bool {
    use crate::core::handle;
    use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;
    let app_handle = handle::Handle::app_handle();
    is_current_app_handle_admin(app_handle)
}

#[cfg(target_os = "macos")]
async fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let uninstall_shell: String = uninstall_path.to_string_lossy().into_owned();

    let prompt = escape_osascript_string(&clash_verge_i18n::t!("service.adminUninstallPrompt"));
    let command =
        format!(r#"do shell script "sudo '{uninstall_shell}'" with administrator privileges with prompt "{prompt}""#);

    let status = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("osascript")
            .args(vec!["-e", &command])
            .status(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!("osascript 超时：请在系统设置 > 隐私与安全性 > 自动化中允许 Clash Verge 控制 System Events")
    })??;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        if code == -128 {
            bail!("用户取消了授权");
        }
        bail!("failed to uninstall service with status {}", code);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn install_service() -> Result<()> {
    logging!(info, Type::Service, "install service");

    let binary_path = dirs::service_path()?;
    let install_path = binary_path.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let install_shell: String = install_path.to_string_lossy().into_owned();

    let gid = tauri_plugin_clash_verge_sysinfo::current_gid();
    let prompt = escape_osascript_string(&clash_verge_i18n::t!("service.adminInstallPrompt"));
    let command = format!(
        r#"do shell script "sudo CLASH_VERGE_SERVICE_GID={gid} '{install_shell}'" with administrator privileges with prompt "{prompt}""#
    );

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("osascript")
            .args(vec!["-e", &command])
            .output(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!("osascript 超时：请在系统设置 > 隐私与安全性 > 自动化中允许 Clash Verge 控制 System Events")
    })??;

    if let Some((code, err)) = check_output_error(&output) {
        if code == -128 {
            bail!("用户取消了授权");
        }
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to install service code: {}, details: {}", code, err);
    }

    logging!(info, Type::Service, "service binary installed successfully");
    Ok(())
}

/// 转义 osascript 字符串中的特殊字符，防止命令注入
#[cfg(target_os = "macos")]
fn escape_osascript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn check_output_error(output: &std::process::Output) -> Option<(i32, Cow<'_, str>)> {
    if output.status.success() {
        return None;
    }
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        return Some((code, stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        return Some((code, stdout));
    }
    Some((code, Cow::Borrowed("Unknown error")))
}

async fn reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "reinstall service");

    // 先卸载服务
    if let Err(err) = uninstall_service().await {
        logging!(warn, Type::Service, "failed to uninstall service: {}", err);
    }

    // 再安装服务
    match install_service().await {
        Ok(_) => Ok(()),
        Err(err) => {
            bail!(format!("failed to install service: {err}"))
        }
    }
}

/// 强制重装服务（UI修复按钮）
/// 与普通 reinstall 不同：先清理残留 IPC socket 文件
async fn force_reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "用户请求强制重装服务");

    // 清理可能残留的 IPC socket 文件
    let ipc_path = Path::new(clash_verge_service_ipc::IPC_PATH);
    if ipc_path.exists() {
        if let Err(e) = std::fs::remove_file(ipc_path) {
            logging!(warn, Type::Service, "清理残留 IPC socket 失败: {}", e);
        } else {
            logging!(info, Type::Service, "已清理残留 IPC socket");
        }
    }

    reinstall_service().await.map_err(|err| {
        logging!(error, Type::Service, "强制重装服务失败: {}", err);
        err
    })
}

/// 尝试使用服务启动core
pub(super) async fn start_with_existing_service(config_file: &PathBuf) -> Result<()> {
    logging!(info, Type::Service, "尝试使用现有服务启动核心");

    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);

    let bin_ext = if cfg!(windows) { ".exe" } else { "" };
    let bin_path = current_exe()?.with_file_name(format!("{clash_core}{bin_ext}"));

    let payload = clash_verge_service_ipc::ClashConfig {
        core_config: CoreConfig {
            config_path: dirs::path_to_str(config_file)?.into(),
            core_path: dirs::path_to_str(&bin_path)?.into(),
            core_ipc_path: IClashTemp::guard_external_controller_ipc(),
            config_dir: dirs::path_to_str(&dirs::app_home_dir()?)?.into(),
        },
        log_config: Logger::global().service_writer_config()?,
    };

    // Retry start_clash with backoff — the service may not be fully initialized
    // immediately after install/restart on macOS.
    use backon::{ExponentialBuilder, Retryable as _};
    let backoff = ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(200))
        .with_max_delay(std::time::Duration::from_secs(3))
        .with_max_times(5);

    let response = (|| async {
        clash_verge_service_ipc::start_clash(&payload).await.map_err(|e| {
            logging!(warn, Type::Service, "start_clash attempt failed: {}", e);
            e
        })
    })
    .retry(backoff)
    .await
    .context("无法连接到Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "启动核心失败: {}", err_msg);
        bail!(err_msg);
    }

    logging!(info, Type::Service, "服务成功启动核心");
    Ok(())
}

/// 以服务启动core — 确认服务就绪后启动
pub(super) async fn run_core_by_service(config_file: &PathBuf) -> Result<()> {
    logging!(info, Type::Service, "正在尝试通过服务启动核心");

    // 确保服务可连接，无需 refresh 整个状态机（避免不必要的重装逻辑）
    let manager = SERVICE_MANAGER.lock().await;
    let status = manager.current();
    drop(manager);

    if !matches!(status, ServiceStatus::Ready) {
        // 尝试直接连接一次验证
        if let Err(e) = try_connect_service().await {
            bail!("服务未就绪，无法通过服务启动核心: {}", e);
        }
    }

    start_with_existing_service(config_file).await
}

pub(super) async fn get_clash_logs_by_service() -> Result<Vec<CompactString>> {
    let response = clash_verge_service_ipc::get_clash_logs()
        .await
        .context("无法连接到Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "获取服务模式下的 Clash 日志失败: {}", err_msg);
        bail!(err_msg);
    }

    Ok(response.data.unwrap_or_default())
}

/// 通过服务停止core
pub(super) async fn stop_core_by_service() -> Result<()> {
    logging!(info, Type::Service, "通过服务停止核心 (IPC)");

    let response = clash_verge_service_ipc::stop_clash()
        .await
        .context("无法连接到Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "停止核心失败: {}", err_msg);
        bail!(err_msg);
    }

    logging!(info, Type::Service, "服务成功停止核心");
    Ok(())
}

/// 检查服务是否正在运行（供前端 is_service_available 命令使用）
pub async fn is_service_available() -> Result<()> {
    if let Err(e) = Path::metadata(clash_verge_service_ipc::IPC_PATH.as_ref()) {
        return Err(e.into());
    }
    clash_verge_service_ipc::connect().await?;
    Ok(())
}

/// 尝试连接服务，带重试
async fn try_connect_service() -> Result<()> {
    let backoff = ConstantBuilder::default()
        .with_delay(Duration::from_millis(300))
        .with_max_times(3);

    (|| async {
        clash_verge_service_ipc::connect().await.map_err(|e| {
            logging!(trace, Type::Service, "connect attempt: {}", e);
            e
        })
    })
    .retry(backoff)
    .await
    .context("无法连接到服务 IPC")
    .map(|_| ())
}

pub async fn wait_and_check_service_available(status: &mut ServiceManager) -> Result<()> {
    wait_for_service_ipc(status, "Waiting for service to be available").await
}

/// 等待服务 IPC 就绪（扩展超时版本，用于 macOS TUN 启动）
pub async fn wait_and_check_service_available_extended(status: &mut ServiceManager) -> Result<()> {
    status.0 = ServiceStatus::Unavailable("Waiting for service to be available (extended)".into());

    let backoff = ConstantBuilder::default()
        .with_delay(Duration::from_millis(300))
        .with_max_times(40); // 最多等待 12 秒，给 LaunchDaemon 足够启动时间

    let result = (|| async {
        if Path::new(clash_verge_service_ipc::IPC_PATH).exists() {
            clash_verge_service_ipc::connect().await?;
            Ok(())
        } else {
            Err(anyhow!("IPC path not ready"))
        }
    })
    .retry(backoff)
    .await;

    if result.is_ok() {
        status.0 = ServiceStatus::Ready;
        logging!(info, Type::Service, "服务 IPC 连接就绪（扩展等待）");
    } else {
        logging!(warn, Type::Service, "等待服务 IPC 超时（扩展等待）");
    }

    result
}

async fn wait_for_service_ipc(status: &mut ServiceManager, reason: &str) -> Result<()> {
    status.0 = ServiceStatus::Unavailable(reason.into());
    let config = ServiceManager::config();

    let backoff = ConstantBuilder::default()
        .with_delay(config.retry_delay)
        .with_max_times(config.max_retries);

    let result = (|| async {
        if Path::new(clash_verge_service_ipc::IPC_PATH).exists() {
            clash_verge_service_ipc::connect().await?;
            Ok(())
        } else {
            Err(anyhow!("IPC path not ready"))
        }
    })
    .retry(backoff)
    .await;

    if result.is_ok() {
        status.0 = ServiceStatus::Ready;
        logging!(info, Type::Service, "服务 IPC 连接就绪");
    } else {
        logging!(warn, Type::Service, "等待服务 IPC 超时");
    }

    result
}

pub fn is_service_ipc_path_exists() -> bool {
    Path::new(clash_verge_service_ipc::IPC_PATH).exists()
}

impl ServiceManager {
    pub fn default() -> Self {
        Self(ServiceStatus::Unavailable("Need Checks".into()))
    }

    pub const fn config() -> clash_verge_service_ipc::IpcConfig {
        clash_verge_service_ipc::IpcConfig {
            default_timeout: Duration::from_millis(150),
            retry_delay: Duration::from_millis(250),
            max_retries: 20,
        }
    }

    /// 初始化服务管理器：尝试连接并更新状态
    pub async fn init(&mut self) -> Result<()> {
        if let Err(e) = clash_verge_service_ipc::connect().await {
            self.0 = ServiceStatus::Unavailable(format!("服务连接失败: {e}"));
            return Err(e);
        }
        self.0 = ServiceStatus::Ready;
        logging!(info, Type::Service, "服务管理器初始化完成，服务可连接");
        Ok(())
    }

    pub fn current(&self) -> ServiceStatus {
        self.0.clone()
    }

    /// 刷新服务状态：检查 + 按需处理（仅 refresh 内部调用 handle_service_status）
    pub async fn refresh(&mut self) -> Result<()> {
        let status = self.check_service_comprehensive().await;
        logging!(info, Type::Service, "服务状态检查结果: {:?}", status);
        self.0 = status.clone();
        // refresh 路径使用 logging_error 吞掉错误（Unavailable 时不应阻断启动流程）
        logging_error!(Type::Service, self.handle_service_status(&status).await);
        Ok(())
    }

    /// 综合服务状态检查（一次性完成所有检查）
    pub async fn check_service_comprehensive(&self) -> ServiceStatus {
        // 首先检查 IPC 路径是否存在
        if !is_service_ipc_path_exists() {
            return ServiceStatus::Unavailable("IPC socket not found".into());
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, skip the version check (is_reinstall_service_needed)
            // which causes infinite reinstall loops. Just check if we can connect.
            // Retry a few times to handle transient IPC failures after service install.
            match try_connect_service().await {
                Ok(()) => ServiceStatus::Ready,
                Err(e) => ServiceStatus::Unavailable(format!("macOS 服务连接失败: {e}")),
            }
        }
        #[cfg(not(target_os = "macos"))]
        if clash_verge_service_ipc::is_reinstall_service_needed().await {
            ServiceStatus::NeedsReinstall
        } else {
            ServiceStatus::Ready
        }
    }

    /// 根据服务状态执行相应操作
    /// 注意: 从 cmd/service.rs 直接调用时，Unavailable 会返回错误以通知前端。
    ///       从 refresh() 调用时，错误被 logging_error! 吞掉，不影响启动流程。
    pub async fn handle_service_status(&mut self, status: &ServiceStatus) -> Result<()> {
        match status {
            ServiceStatus::Ready => {
                logging!(info, Type::Service, "服务就绪");
                self.0 = ServiceStatus::Ready;
            }
            ServiceStatus::NeedsReinstall | ServiceStatus::ReinstallRequired => {
                logging!(info, Type::Service, "服务需要重装，执行重装流程");
                reinstall_service().await?;
                wait_and_check_service_available(self).await?;
            }
            ServiceStatus::ForceReinstallRequired => {
                logging!(info, Type::Service, "服务需要强制重装，执行强制重装流程");
                force_reinstall_service().await?;
                wait_and_check_service_available(self).await?;
            }
            ServiceStatus::InstallRequired => {
                logging!(info, Type::Service, "需要安装服务，执行安装流程");
                install_service().await?;
                wait_and_check_service_available(self).await?;
            }
            ServiceStatus::UninstallRequired => {
                logging!(info, Type::Service, "服务需要卸载，执行卸载流程");
                uninstall_service().await?;
                self.0 = ServiceStatus::Unavailable("Service Uninstalled".into());
                // 卸载后核心已停止，跳过 Tray 更新（会因核心不可用而失败）
                return Ok(());
            }
            ServiceStatus::Unavailable(reason) => {
                logging!(warn, Type::Service, "服务不可用: {}", reason);
                self.0 = ServiceStatus::Unavailable(reason.clone());
                bail!("服务不可用: {}", reason);
            }
        }

        // 防止服务安装成功后，内核未完全启动导致系统托盘无法获取代理节点信息
        Tray::global().update_menu().await?;
        Ok(())
    }
}

pub static SERVICE_MANAGER: Lazy<Mutex<ServiceManager>> = Lazy::new(|| Mutex::new(ServiceManager::default()));
