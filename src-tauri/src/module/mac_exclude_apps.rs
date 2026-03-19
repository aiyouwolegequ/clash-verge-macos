use crate::config::Config;
use crate::process::AsyncHandler;
use anyhow::Result;
use chrono::Local;
use chrono::Timelike as _;
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

    pub async fn init(&self) -> Result<()> {
        // Check if there are any apps to refresh
        let apps = Config::verge()
            .await
            .latest_arc()
            .mac_exclude_apps
            .clone()
            .unwrap_or_default();

        if apps.is_empty() {
            logging!(
                debug,
                Type::Core,
                "No mac exclude apps configured, skipping refresh timer"
            );
            return Ok(());
        }

        self.enabled.store(true, Ordering::SeqCst);
        self.start_scheduler();
        Ok(())
    }

    fn start_scheduler(&self) {
        use tokio::time::{Duration, interval};

        let _ = interval(Duration::from_secs(24 * 60 * 60)); // 24 hours

        // Wait until noon for first run
        let now = Local::now();
        let hours_until_noon = if now.hour() < 12 {
            12 - now.hour()
        } else {
            36 - now.hour()
        };
        let minutes_until_noon = 60 - now.minute();
        let seconds_until_noon =
            (hours_until_noon as u64 * 3600 + minutes_until_noon as u64 * 60) - now.second() as u64;
        let initial_delay = Duration::from_secs(seconds_until_noon);

        logging!(
            info,
            Type::Core,
            "Mac exclude apps refresh: next run in {} seconds (12:00)",
            seconds_until_noon
        );

        let delay = initial_delay;
        AsyncHandler::spawn(move || async move {
            // Initial delay to reach noon
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

                // Wait 24 hours for next run
                tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
            }
        });
    }

    /// Refresh all executables for mac exclude apps
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

            // Always add the app name itself
            if let Some(app_name) = app_bundle.file_stem().and_then(|s| s.to_str()) {
                let app_name_sm = SmartString::from(app_name);
                if !all_executables.contains(&app_name_sm) {
                    all_executables.push(app_name_sm);
                }
            }

            // Get all executables in Contents/MacOS/
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

        // Update the config
        let verge_config = Config::verge().await;
        verge_config.edit_draft(|config| {
            config.mac_exclude_app_executables = Some(all_executables.clone());
        });
        verge_config.apply();

        // Save to file
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

    /// Trigger a manual refresh
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
