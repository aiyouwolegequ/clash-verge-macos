use crate::{
    config::Config,
    core::{
        logger::Logger, owner_identity::current_owner_credentials, runtime_bundle::collect_runtime_bundle, tray::Tray,
    },
    utils::dirs,
};
use anyhow::{Context as _, Result, anyhow, bail};
use backon::{ConstantBuilder, Retryable as _};
use clash_verge_logging::{Type, logging};
use clash_verge_service_ipc::{
    OwnerSessionProof, ServiceErrorCode, StageRuntimeOutcome, StartClashRequest, WriterConfig,
};
use compact_str::CompactString;
use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkingMutex;
use std::{
    borrow::Cow,
    env::current_exe,
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;

static ACTIVE_SERVICE_SESSION: Lazy<ParkingMutex<Option<ActiveServiceSession>>> = Lazy::new(|| ParkingMutex::new(None));
const SERVICE_IPC_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const SERVICE_IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SERVICE_IPC_LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Clone)]
struct ActiveServiceSession {
    proof: OwnerSessionProof,
    supports_runtime_staging: bool,
}

fn generate_service_session_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("failed to generate service owner session")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn active_service_session() -> Result<OwnerSessionProof> {
    ACTIVE_SERVICE_SESSION
        .lock()
        .as_ref()
        .map(|session| session.proof.clone())
        .context("service owner session is not active")
}

pub(crate) fn active_service_supports_runtime_staging() -> bool {
    ACTIVE_SERVICE_SESSION
        .lock()
        .as_ref()
        .is_some_and(|session| session.supports_runtime_staging)
}

pub(crate) fn has_active_service_session() -> bool {
    ACTIVE_SERVICE_SESSION.lock().is_some()
}

pub(super) fn mihomo_socket_path_by_service() -> Result<String> {
    let credentials = current_owner_credentials()?;
    Ok(clash_verge_service_ipc::mihomo_ipc_path(&credentials.identity))
}

fn clear_active_service_session() {
    ACTIVE_SERVICE_SESSION.lock().take();
}

fn service_core_path(clash_core: &str) -> Result<PathBuf> {
    let core_path = current_exe()?.with_file_name(clash_core);

    #[cfg(target_os = "macos")]
    if is_macos_temporary_app_path(&core_path) {
        bail!(
            "macOS Service 不能从临时应用路径启动内核；请将 Clash Verge.app 移动到 /Applications 后重新打开再启用 TUN"
        );
    }

    Ok(core_path)
}

#[cfg(target_os = "macos")]
fn is_macos_temporary_app_path(path: &Path) -> bool {
    path.starts_with("/Volumes")
        || path
            .components()
            .any(|component| component.as_os_str() == "AppTranslocation")
}

async fn probe_runtime_staging_support() -> bool {
    match run_service_ipc(clash_verge_service_ipc::get_version(), SERVICE_IPC_PROBE_TIMEOUT).await {
        Ok(response) if response.code == 0 => response
            .data
            .as_ref()
            .is_some_and(clash_verge_service_ipc::ProtocolInfo::supports_runtime_staging),
        Ok(response) => {
            logging!(
                warn,
                Type::Service,
                "service protocol query returned {}: {}; runtime staging is unavailable",
                response.code,
                response.message
            );
            false
        }
        Err(error) => {
            logging!(
                warn,
                Type::Service,
                "unable to query service protocol: {error:#}; runtime staging is unavailable"
            );
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ServiceStatus {
    Checking,
    Ready,
    NotInstalled,
    NeedsReinstall,
    InstallRequired,
    UninstallRequired,
    ReinstallRequired,
    ForceReinstallRequired,
    SidecarAllowed,
    Unavailable(String),
}

pub(super) enum StageRequest {
    Refused { code: u16, message: CompactString },
    Answered(StageRuntimeOutcome),
}

impl StageRequest {
    pub(super) const fn is_about_the_bundle(code: u16) -> bool {
        code == ServiceErrorCode::InvalidRuntimeAsset as u16 || code == ServiceErrorCode::InvalidInstallLocation as u16
    }
}

#[derive(Clone)]
pub struct ServiceManager(ServiceStatus);

async fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let uninstall_shell: String = uninstall_path.to_string_lossy().into_owned();

    // On macOS, always use osascript for admin privileges (the service binary handles the rest)
    let prompt = escape_osascript_string(&clash_verge_i18n::t!("service.adminUninstallPrompt"));
    let shell_command = format!("sudo {}", shell_quote(uninstall_shell.as_str()));
    let shell_command = escape_osascript_string(shell_command.as_str());
    let command = format!(r#"do shell script "{shell_command}" with administrator privileges with prompt "{prompt}""#);

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
    let shell_command = format!(
        "sudo CLASH_VERGE_SERVICE_GID={gid} {}",
        shell_quote(install_shell.as_str())
    );
    let shell_command = escape_osascript_string(shell_command.as_str());
    let command = format!(r#"do shell script "{shell_command}" with administrator privileges with prompt "{prompt}""#);

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
fn escape_osascript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn shell_quote(s: &str) -> std::string::String {
    let mut quoted = std::string::String::from("'");
    quoted.push_str(&s.replace('\'', r"'\''"));
    quoted.push('\'');
    quoted
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
pub(super) async fn start_with_existing_service(config_file: &Path) -> Result<()> {
    logging!(info, Type::Service, "尝试使用现有服务启动核心");
    clear_active_service_session();

    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);

    let bin_path = service_core_path(&clash_core)?;

    let credentials = current_owner_credentials()?;
    let runtime = collect_runtime_bundle(config_file, &bin_path).await?;
    let proposed_session_token = generate_service_session_token()?;
    let request = StartClashRequest {
        runtime,
        proposed_session_token: proposed_session_token.clone(),
        macos_proxy: None,
    };

    let response = run_service_ipc(
        clash_verge_service_ipc::start_clash(&credentials, &request),
        SERVICE_IPC_LIFECYCLE_TIMEOUT,
    )
    .await
    .context("无法连接到Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "启动核心失败: {}", err_msg);
        bail!(err_msg);
    }

    let result = response.data.context("Clash Verge Service 未返回会话信息")?;
    *ACTIVE_SERVICE_SESSION.lock() = Some(ActiveServiceSession {
        proof: OwnerSessionProof {
            generation: result.session.generation,
            token: proposed_session_token,
        },
        supports_runtime_staging: false,
    });

    match Logger::global().service_writer_config() {
        Ok(writer) => {
            if let Err(error) = update_writer_by_service(&writer).await {
                logging!(warn, Type::Service, "启动后配置 Service 核心日志失败: {error:#}");
            }
        }
        Err(error) => logging!(warn, Type::Service, "创建 Service 核心日志配置失败: {error:#}"),
    }

    let supports_runtime_staging = probe_runtime_staging_support().await;
    if let Some(session) = ACTIVE_SERVICE_SESSION.lock().as_mut() {
        session.supports_runtime_staging = supports_runtime_staging;
    }

    logging!(info, Type::Service, "服务成功启动核心，已建立所有权会话");
    Ok(())
}

/// 以服务启动core — 确认服务就绪后启动
pub(super) async fn run_core_by_service(config_file: &Path) -> Result<()> {
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
    let credentials = current_owner_credentials()?;
    let response = run_service_ipc(
        clash_verge_service_ipc::get_clash_logs(&credentials),
        SERVICE_IPC_REQUEST_TIMEOUT,
    )
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

    let credentials = current_owner_credentials()?;
    let session = active_service_session()?;
    let response = run_service_ipc(
        clash_verge_service_ipc::stop_clash(&credentials, &session),
        SERVICE_IPC_LIFECYCLE_TIMEOUT,
    )
    .await
    .context("无法连接到Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "停止核心失败: {}", err_msg);
        bail!(err_msg);
    }

    clear_active_service_session();
    logging!(info, Type::Service, "服务成功停止核心");
    Ok(())
}

pub(crate) async fn update_writer_by_service(writer: &WriterConfig) -> Result<()> {
    let credentials = current_owner_credentials()?;
    let session = active_service_session()?;
    let response = run_service_ipc(
        clash_verge_service_ipc::update_writer(&credentials, &session, writer),
        SERVICE_IPC_REQUEST_TIMEOUT,
    )
    .await
    .context("无法连接到Clash Verge Service")?;
    if response.code > 0 {
        bail!(response.message);
    }
    Ok(())
}

pub(super) async fn stage_runtime_by_service(config_file: &Path) -> Result<StageRequest> {
    let credentials = current_owner_credentials()?;
    let session = active_service_session()?;
    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);
    let core_path = service_core_path(&clash_core)?;
    let runtime = collect_runtime_bundle(config_file, &core_path).await?;

    let response = run_service_ipc(
        clash_verge_service_ipc::stage_runtime(&credentials, &session, &runtime),
        SERVICE_IPC_LIFECYCLE_TIMEOUT,
    )
    .await
    .context("无法连接到Clash Verge Service")?;
    if response.code > 0 {
        return Ok(StageRequest::Refused {
            code: response.code,
            message: response.message.into(),
        });
    }
    response
        .data
        .map(StageRequest::Answered)
        .context("Clash Verge Service 未返回运行时暂存结果")
}

/// 检查服务是否正在运行（供前端 is_service_available 命令使用）
pub async fn is_service_available() -> Result<()> {
    Path::metadata(clash_verge_service_ipc::IPC_PATH.as_ref())?;
    let response = run_service_ipc(clash_verge_service_ipc::get_version(), SERVICE_IPC_PROBE_TIMEOUT).await?;
    let protocol = response.data.context("service did not return protocol information")?;
    if response.code != 0
        || !protocol.supports_client(
            clash_verge_service_ipc::ProtocolVersion::current(),
            clash_verge_service_ipc::MIN_REQUIRED_SERVICE_REVISION,
        )
    {
        bail!("service IPC protocol is incompatible: {}", response.message);
    }
    Ok(())
}

/// 尝试连接服务，带重试
async fn try_connect_service() -> Result<()> {
    let backoff = ConstantBuilder::default()
        .with_delay(Duration::from_millis(300))
        .with_max_times(3);

    (|| async {
        is_service_available().await.map_err(|e| {
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
            is_service_available().await?;
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
            is_service_available().await?;
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

async fn run_service_ipc<T>(operation: impl Future<Output = Result<T>>, timeout: Duration) -> Result<T> {
    // The IPC client retries its transport internally, so every direct service request must pass
    // through this outer deadline to keep a stale socket from blocking a caller indefinitely.
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| anyhow!("macOS 服务 IPC 超时（{} 秒）", timeout.as_secs()))?
}

impl ServiceManager {
    pub const fn default() -> Self {
        Self(ServiceStatus::Checking)
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
        self.0 = self.check_service_comprehensive().await;
        if !matches!(self.0, ServiceStatus::Ready) {
            bail!("service is not ready: {:?}", self.0);
        }
        logging!(info, Type::Service, "服务管理器初始化完成，服务可连接");
        Ok(())
    }

    pub fn current(&self) -> ServiceStatus {
        self.0.clone()
    }

    /// 刷新服务状态只记录观察结果；安装和修复必须由前端显式请求。
    pub async fn refresh(&mut self) -> Result<()> {
        let status = self.check_service_comprehensive().await;
        logging!(info, Type::Service, "服务状态检查结果: {:?}", status);
        self.0 = status;
        Ok(())
    }

    /// 综合服务状态检查（一次性完成所有检查）
    pub async fn check_service_comprehensive(&self) -> ServiceStatus {
        // IPC 路径不存在表示 service 未安装；路径存在但协议探测失败则保留为可修复错误。
        if !is_service_ipc_path_exists() {
            return ServiceStatus::NotInstalled;
        }
        match run_service_ipc(clash_verge_service_ipc::get_version(), SERVICE_IPC_PROBE_TIMEOUT).await {
            Ok(response) if response.code == 0 => match response.data {
                Some(protocol)
                    if protocol.supports_client(
                        clash_verge_service_ipc::ProtocolVersion::current(),
                        clash_verge_service_ipc::MIN_REQUIRED_SERVICE_REVISION,
                    ) =>
                {
                    ServiceStatus::Ready
                }
                _ => ServiceStatus::NeedsReinstall,
            },
            Ok(_) => ServiceStatus::NeedsReinstall,
            Err(error) => ServiceStatus::Unavailable(format!("macOS 服务连接失败: {error}")),
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
            ServiceStatus::Checking => {
                self.0 = ServiceStatus::Checking;
            }
            ServiceStatus::NotInstalled | ServiceStatus::SidecarAllowed => {
                self.0 = status.clone();
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

#[cfg(test)]
mod tests {
    use super::{escape_osascript_string, run_service_ipc, shell_quote};
    #[cfg(target_os = "macos")]
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn shell_quote_wraps_argument() {
        assert_eq!(shell_quote("abc"), "'abc'");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn escape_osascript_string_escapes_command_quotes_and_backslashes() {
        assert_eq!(
            escape_osascript_string(r#"sudo '/tmp/A "quoted" \ path'"#),
            r#"sudo '/tmp/A \"quoted\" \\ path'"#
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_temporary_macos_app_paths() {
        assert!(super::is_macos_temporary_app_path(Path::new(
            "/Volumes/Clash Verge/Clash Verge.app/Contents/MacOS/verge-mihomo"
        )));
        assert!(super::is_macos_temporary_app_path(Path::new(
            "/private/var/folders/example/T/AppTranslocation/123/d/Clash Verge.app/Contents/MacOS/verge-mihomo"
        )));
        assert!(!super::is_macos_temporary_app_path(Path::new(
            "/Applications/Clash Verge.app/Contents/MacOS/verge-mihomo"
        )));
    }

    #[tokio::test]
    async fn timed_out_service_ipc_request_returns_an_error() {
        let result = run_service_ipc(std::future::pending::<anyhow::Result<()>>(), Duration::ZERO).await;
        assert!(result.is_err_and(|error| error.to_string().contains("超时")));
    }
}
