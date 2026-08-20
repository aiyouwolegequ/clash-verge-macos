use super::{CoreManager, RunningMode};
use crate::{
    config::{Config, ConfigType, runtime::IRuntime},
    constants::timing,
    core::{
        handle,
        service::{self, StageRequest},
        validate::{CoreConfigValidator, ValidationOutcome, ValidationSkipReason},
    },
    utils::{dirs, help},
};
use anyhow::{Result, anyhow};
use clash_verge_logging::{Type, logging};
use clash_verge_service_ipc::StageRuntimeOutcome;
use smartstring::alias::String;
use std::{collections::HashSet, time::Instant};
use tauri_plugin_mihomo::Error as MihomoError;

impl CoreManager {
    pub async fn use_default_config(&self, error_key: &str, error_msg: &str) -> Result<()> {
        use crate::constants::files::RUNTIME_CONFIG;

        let runtime_path = dirs::app_home_dir()?.join(RUNTIME_CONFIG);
        let clash_config = &Config::clash().await.latest_arc().0;

        Config::runtime().await.edit_draft(|d| {
            *d = IRuntime {
                config: Some(clash_config.to_owned()),
                exists_keys: HashSet::new(),
                chain_logs: Default::default(),
            }
        });

        help::save_yaml(&runtime_path, &clash_config, Some("# Clash Verge Runtime")).await?;
        handle::Handle::notice_message(error_key, error_msg);
        Ok(())
    }

    pub async fn update_config_forced(&self) -> Result<ValidationOutcome> {
        self.update_config_with_force(true).await
    }

    pub async fn update_config_with_force(&self, force: bool) -> Result<ValidationOutcome> {
        let _operation = self.operation_lock.lock().await;
        self.update_config_with_force_unlocked(force).await
    }

    async fn update_config_with_force_unlocked(&self, force: bool) -> Result<ValidationOutcome> {
        if handle::Handle::global().is_exiting() {
            return Ok(ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Exiting,
            });
        }

        if !force && !self.should_update_config() {
            logging!(debug, Type::Core, "Skipping config update due to debounce");
            return Ok(ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Debounced,
            });
        }

        if force {
            self.set_last_update(Instant::now());
        }

        self.perform_config_update().await
    }

    pub async fn update_config_checked(&self) -> Result<()> {
        let outcome = self.update_config_forced().await?;
        if outcome.is_valid() {
            Ok(())
        } else {
            Err(anyhow!("{outcome}"))
        }
    }

    fn should_update_config(&self) -> bool {
        let now = Instant::now();
        let last = self.get_last_update();

        if let Some(last_time) = last
            && now.duration_since(*last_time) < timing::CONFIG_UPDATE_DEBOUNCE
        {
            return false;
        }

        self.set_last_update(now);
        true
    }

    async fn perform_config_update(&self) -> Result<ValidationOutcome> {
        if let Err(err) = Config::generate().await {
            let message: String = err.to_string().into();
            Config::runtime().await.discard();
            return Ok(ValidationOutcome::invalid_from_message(message));
        }

        self.apply_generate_config_unlocked().await
    }

    #[allow(clippy::cognitive_complexity)]
    pub async fn apply_generate_config(&self) -> Result<ValidationOutcome> {
        let _operation = self.operation_lock.lock().await;
        self.apply_generate_config_unlocked().await
    }

    #[allow(clippy::cognitive_complexity)]
    async fn apply_generate_config_unlocked(&self) -> Result<ValidationOutcome> {
        use crate::constants::files::RUNTIME_CONFIG;

        let run_path = dirs::app_home_dir()?.join(RUNTIME_CONFIG);
        let backup_path = run_path.with_extension("yaml.bak");

        // 1. Back up current config file if it exists
        let has_backup = if run_path.exists() {
            if let Err(err) = tokio::fs::copy(&run_path, &backup_path).await {
                logging!(warn, Type::Core, "Failed to back up runtime config: {err}");
                false
            } else {
                true
            }
        } else {
            false
        };

        // 2. Generate the new config file
        let run_path = match Config::generate_file(ConfigType::Run).await {
            Ok(p) => p,
            Err(e) => {
                if has_backup {
                    let _ = tokio::fs::remove_file(&backup_path).await;
                }
                Config::runtime().await.discard();
                return Err(e);
            }
        };

        let path_str = dirs::path_to_str(&run_path)?;

        // Service IPC v2 can stage the complete runtime generation in place. macOS Mihomo
        // rejects the staged config because the service runtime is outside its allowed home
        // directory, so use the existing API reload/restart flow there instead.
        if !cfg!(target_os = "macos")
            && matches!(*self.get_running_mode(), RunningMode::Service)
            && service::active_service_supports_runtime_staging()
        {
            match service::stage_runtime_by_service(&run_path).await {
                Ok(StageRequest::Answered(StageRuntimeOutcome::Staged { config_path })) => {
                    match self.reload_config(&config_path).await {
                        Ok(()) => {
                            Config::runtime().await.apply();
                            logging!(info, Type::Core, "Configuration staged and hot-reloaded by service");
                            if has_backup {
                                let _ = tokio::fs::remove_file(&backup_path).await;
                            }
                            return Ok(ValidationOutcome::Valid);
                        }
                        Err(error) => logging!(
                            warn,
                            Type::Core,
                            "Staged configuration reload failed: {error}; falling back to restart"
                        ),
                    }
                }
                Ok(StageRequest::Refused { code, message }) if StageRequest::is_about_the_bundle(code) => {
                    if has_backup {
                        let _ = tokio::fs::copy(&backup_path, &run_path).await;
                        let _ = tokio::fs::remove_file(&backup_path).await;
                    }
                    Config::runtime().await.discard();
                    return Err(anyhow!("Service refused the runtime bundle: {message}"));
                }
                Ok(StageRequest::Answered(StageRuntimeOutcome::RestartRequired { reason })) => logging!(
                    info,
                    Type::Core,
                    "Service requested a core restart after staging: {reason:?}"
                ),
                Ok(StageRequest::Refused { code, message }) => logging!(
                    warn,
                    Type::Core,
                    "Service refused runtime staging ({code}): {message}; falling back to restart"
                ),
                Err(error) => logging!(
                    warn,
                    Type::Core,
                    "Runtime staging did not complete: {error:#}; falling back to restart"
                ),
            }
        }

        // 3. A macOS Service keeps its runtime in a private directory. Mihomo therefore rejects
        // an API reload pointing at the app-owned config path, so skip the known-failing request
        // and proceed directly to validation plus a controlled Service restart.
        if cfg!(target_os = "macos") && matches!(*self.get_running_mode(), RunningMode::Service) {
            logging!(
                info,
                Type::Core,
                "Skipping API config reload for macOS Service; validating before controlled restart"
            );
        } else {
            // Try to hot-reload config directly via REST API first (fast path).
            match self.reload_config(path_str).await {
                Ok(_) => {
                    Config::runtime().await.apply();
                    logging!(info, Type::Core, "Configuration hot-reloaded successfully");
                    if has_backup {
                        let _ = tokio::fs::remove_file(&backup_path).await;
                    }
                    return Ok(ValidationOutcome::Valid);
                }
                Err(reload_err) => logging!(
                    warn,
                    Type::Core,
                    "Failed to reload config via API: {reload_err}. Running sidecar validator to check configuration correctness..."
                ),
            }
        }

        // 4. Run sidecar validator before applying the generated config with a restart.
        match CoreConfigValidator::global().validate_config_outcome().await {
            Ok(outcome) if outcome.is_valid() => {
                logging!(
                    info,
                    Type::Core,
                    "Configuration is valid. Attempting core restart to apply configuration..."
                );
                match self.restart_core_unlocked().await {
                    Ok(_) => {
                        Config::runtime().await.apply();
                        logging!(info, Type::Core, "Configuration applied after core restart");
                        if has_backup {
                            let _ = tokio::fs::remove_file(&backup_path).await;
                        }
                        Ok(ValidationOutcome::Valid)
                    }
                    Err(restart_err) => {
                        logging!(error, Type::Core, "Failed to restart core: {restart_err}");
                        if has_backup {
                            if let Err(restore_err) = tokio::fs::copy(&backup_path, &run_path).await {
                                logging!(error, Type::Core, "Failed to restore backup config: {restore_err}");
                            }
                            let _ = tokio::fs::remove_file(&backup_path).await;
                        }
                        Config::runtime().await.discard();
                        Err(anyhow!("Failed to apply config: {restart_err}"))
                    }
                }
            }
            Ok(outcome) => {
                logging!(
                    warn,
                    Type::Core,
                    "Configuration is invalid: {outcome}. Restoring backup..."
                );
                if has_backup {
                    if let Err(restore_err) = tokio::fs::copy(&backup_path, &run_path).await {
                        logging!(error, Type::Core, "Failed to restore backup config: {restore_err}");
                    }
                    let _ = tokio::fs::remove_file(&backup_path).await;
                }
                Config::runtime().await.discard();
                Ok(outcome)
            }
            Err(validate_err) => {
                logging!(
                    error,
                    Type::Core,
                    "Validation process failed: {validate_err}. Restoring backup..."
                );
                if has_backup {
                    if let Err(restore_err) = tokio::fs::copy(&backup_path, &run_path).await {
                        logging!(error, Type::Core, "Failed to restore backup config: {restore_err}");
                    }
                    let _ = tokio::fs::remove_file(&backup_path).await;
                }
                Config::runtime().await.discard();
                Err(validate_err)
            }
        }
    }

    async fn reload_config(&self, path: &str) -> Result<(), MihomoError> {
        handle::Handle::mihomo().await.reload_config(true, path).await
    }
}
