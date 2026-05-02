use crate::config::Config;
use crate::process::AsyncHandler;
use anyhow::Result;
use chrono::Local;
use clash_verge_logging::{Type, logging};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use smartstring::alias::String as SmartString;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct MacExcludeAppsManager {
    enabled: AtomicBool,
    last_refresh: RwLock<i64>,
}

impl MacExcludeAppsManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceCell<MacExcludeAppsManager> = OnceCell::new();
        INSTANCE.get_or_init(|| Self {
            enabled: AtomicBool::new(false),
            last_refresh: RwLock::new(0),
        })
    }

    pub fn init(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        self.start_scheduler();
    }

    fn start_scheduler(&self) {
        // First refresh runs after 30s startup delay to avoid racing with core init
        const STARTUP_DELAY_SECS: u64 = 30;
        const DAY_SECS: u64 = 24 * 60 * 60;

        let initial_delay = std::time::Duration::from_secs(STARTUP_DELAY_SECS);

        logging!(
            info,
            Type::Core,
            "Mac exclude apps refresh: first run in {} seconds",
            STARTUP_DELAY_SECS
        );

        let delay = initial_delay;
        AsyncHandler::spawn(move || async move {
            tokio::time::sleep(delay).await;

            loop {
                if !Self::global().enabled.load(Ordering::SeqCst) {
                    logging!(info, Type::Core, "Mac exclude apps refresh scheduler stopped");
                    break;
                }

                if let Err(e) = Self::global().refresh_executables().await {
                    logging!(
                        error,
                        Type::Core,
                        "Failed to refresh mac exclude apps executables: {}",
                        e
                    );
                } else {
                    logging!(info, Type::Core, "Mac exclude apps executables refreshed successfully");
                }

                tokio::time::sleep(std::time::Duration::from_secs(DAY_SECS)).await;
            }
        });
    }

    pub async fn refresh_executables(&self) -> Result<()> {
        let apps = Config::verge()
            .await
            .latest_arc()
            .mac_exclude_apps
            .clone()
            .unwrap_or_default();

        if apps.is_empty() {
            return Ok(());
        }

        let mut all_executables = Vec::<SmartString>::new();

        for app_path in &apps {
            let app_bundle = std::path::Path::new(app_path.as_str());
            let macos_dir = app_bundle.join("Contents/MacOS");

            if let Some(app_name) = app_bundle.file_stem().and_then(|s| s.to_str()) {
                let app_name_sm = SmartString::from(app_name);
                if !all_executables.contains(&app_name_sm) {
                    all_executables.push(app_name_sm);
                }
            }

            #[allow(clippy::collapsible_if)]
            if let Ok(entries) = std::fs::read_dir(&macos_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() || path.symlink_metadata().ok().is_some_and(|m| m.file_type().is_symlink()) {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            let name_sm = SmartString::from(name);
                            if !all_executables.contains(&name_sm) {
                                all_executables.push(name_sm);
                            }
                        }
                    }
                }
            }
        }

        let verge_config = Config::verge().await;
        verge_config.edit_draft(|config| {
            config.mac_exclude_app_executables = Some(all_executables.clone());
        });
        verge_config.apply();

        let verge_data = verge_config.latest_arc();
        verge_data.save_file().await?;

        *self.last_refresh.write() = Local::now().timestamp();

        logging!(
            info,
            Type::Core,
            "Refreshed {} executables for {} mac exclude apps",
            all_executables.len(),
            apps.len()
        );

        Ok(())
    }

    #[allow(dead_code)]
    pub fn trigger_refresh() {
        AsyncHandler::spawn(|| async move {
            if let Err(e) = Self::global().refresh_executables().await {
                logging!(error, Type::Core, "Manual refresh failed: {}", e);
            } else {
                logging!(info, Type::Core, "Manual refresh completed");
            }
        });
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }
}
