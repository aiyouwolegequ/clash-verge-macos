use crate::config::Config;
use crate::core::CoreManager;
use crate::process::AsyncHandler;
use anyhow::Result;
use chrono::Local;
use clash_verge_logging::{Type, logging};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use plist::Value as PlistValue;
use serde::Serialize;
use smartstring::alias::String as SmartString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct MacAppInfo {
    pub name: SmartString,
    pub path: SmartString,
    pub bundle_id: Option<SmartString>,
    pub executable_names: Vec<SmartString>,
}

fn plist_string(info: &PlistValue, key: &str) -> Option<SmartString> {
    info.as_dictionary()
        .and_then(|dict| dict.get(key))
        .and_then(PlistValue::as_string)
        .filter(|value| !value.trim().is_empty())
        .map(SmartString::from)
}

pub fn read_bundle_info(app_bundle: &Path) -> (Option<SmartString>, Option<SmartString>, Option<SmartString>) {
    let info_path = app_bundle.join("Contents/Info.plist");
    let Ok(info) = PlistValue::from_file(&info_path) else {
        return (None, None, None);
    };

    let display_name = plist_string(&info, "CFBundleDisplayName")
        .or_else(|| plist_string(&info, "CFBundleName"))
        .or_else(|| plist_string(&info, "CFBundleExecutable"));
    let bundle_id = plist_string(&info, "CFBundleIdentifier");
    let executable = plist_string(&info, "CFBundleExecutable");
    (display_name, bundle_id, executable)
}

fn path_stem(path: &Path) -> Option<SmartString> {
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(SmartString::from)
}

fn path_file_name(path: &Path) -> Option<SmartString> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(SmartString::from)
}

pub fn collect_bundle_executables(app_bundle: &Path) -> Vec<SmartString> {
    let mut executables = Vec::<SmartString>::new();
    let (_, _, bundle_executable) = read_bundle_info(app_bundle);
    if let Some(executable) = bundle_executable {
        executables.push(executable);
    }
    if let Some(app_name) = path_stem(app_bundle)
        && !executables.contains(&app_name)
    {
        executables.push(app_name);
    }

    let macos_dir = app_bundle.join("Contents/MacOS");
    if let Ok(entries) = std::fs::read_dir(&macos_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if (path.is_file() || path.symlink_metadata().ok().is_some_and(|m| m.file_type().is_symlink()))
                && let Some(name) = path_file_name(&path)
                && !executables.contains(&name)
            {
                executables.push(name);
            }
        }
    }

    executables
}

fn is_app_bundle(path: &Path) -> bool {
    path.is_dir()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn collect_installed_macos_apps(dirs: impl IntoIterator<Item = PathBuf>) -> Vec<MacAppInfo> {
    let mut bundles = Vec::new();

    for root in dirs {
        let mut pending_dirs = vec![root];
        while let Some(dir) = pending_dirs.pop() {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if is_app_bundle(&path) {
                    bundles.push(path);
                    continue;
                }

                // Do not follow directory symlinks: they can form cycles outside
                // /Applications. Symlinked .app bundles above are still included.
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    pending_dirs.push(path);
                }
            }
        }
    }

    let mut apps = Vec::new();
    for path in bundles {
        let (display_name, bundle_id, _) = read_bundle_info(&path);
        let Some(name) = display_name.or_else(|| path_stem(&path)) else {
            continue;
        };
        apps.push(MacAppInfo {
            name,
            path: SmartString::from(path.to_string_lossy().as_ref()),
            bundle_id,
            executable_names: collect_bundle_executables(&path),
        });
    }

    apps.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    apps.dedup_by(|a, b| a.path == b.path);
    apps
}

pub fn get_installed_macos_apps() -> Vec<MacAppInfo> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }

    collect_installed_macos_apps(dirs)
}

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
            for executable in collect_bundle_executables(std::path::Path::new(app_path.as_str())) {
                if !all_executables.contains(&executable) {
                    all_executables.push(executable);
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

    pub async fn refresh_and_apply(&self) -> Result<()> {
        self.refresh_executables().await?;
        CoreManager::global().update_config_checked().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_apps_from_application_subdirectories_without_helpers() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("clash-verge-mac-apps-{}", std::process::id()));
        let wechat = root.join("微信.app");
        let nested = root.join("Utilities").join("Nested.app");
        let helper = wechat.join("Contents/Frameworks/Helper.app");

        std::fs::create_dir_all(&helper)?;
        std::fs::create_dir_all(&nested)?;

        let apps = collect_installed_macos_apps(vec![root.clone()]);
        let paths = apps.iter().map(|app| app.path.as_str()).collect::<Vec<_>>();
        assert!(paths.contains(&wechat.to_string_lossy().as_ref()));
        assert!(paths.contains(&nested.to_string_lossy().as_ref()));
        assert!(!paths.contains(&helper.to_string_lossy().as_ref()));

        std::fs::remove_dir_all(root)
    }
}
