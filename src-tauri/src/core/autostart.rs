use crate::{config::Config, core::handle::Handle};
use anyhow::Result;
use clash_verge_logging::{Type, logging, logging_error};
use tauri_plugin_autostart::ManagerExt as _;

pub async fn update_launch() -> Result<()> {
    let enable_auto_launch = { Config::verge().await.latest_arc().enable_auto_launch };
    let is_enable = enable_auto_launch.unwrap_or(false);
    logging!(info, Type::System, "Setting auto-launch enabled state to: {is_enable}");

    let app_handle = Handle::app_handle();
    let autostart_manager = app_handle.autolaunch();
    if is_enable {
        logging_error!(Type::System, "{:?}", autostart_manager.enable());
    } else {
        logging_error!(Type::System, "{:?}", autostart_manager.disable());
    }

    Ok(())
}

pub fn get_launch_status() -> Result<bool> {
    let app_handle = Handle::app_handle();
    let autostart_manager = app_handle.autolaunch();
    match autostart_manager.is_enabled() {
        Ok(status) => {
            logging!(info, Type::System, "Auto-launch status: {status}");
            Ok(status)
        }
        Err(e) => {
            logging!(error, Type::System, "Failed to get auto-launch status: {e}");
            Err(anyhow::anyhow!("Failed to get auto-launch status: {}", e))
        }
    }
}
