use crate::utils::dirs;
use anyhow::{Context as _, Result};
use clash_verge_service_ipc::{OwnerCredentials, OwnerIdentity};
use std::path::Path;

pub(crate) fn current_owner_credentials() -> Result<OwnerCredentials> {
    current_owner_credentials_for_root(&dirs::app_home_dir()?)
}

pub(crate) fn current_owner_credentials_for_root(app_root: &Path) -> Result<OwnerCredentials> {
    let app_data_root = std::fs::canonicalize(app_root)
        .with_context(|| format!("failed to canonicalize application data root {app_root:?}"))?;

    Ok(OwnerCredentials {
        identity: OwnerIdentity::Unix {
            uid: unsafe { tauri_plugin_clash_verge_sysinfo::libc::geteuid() },
            gid: unsafe { tauri_plugin_clash_verge_sysinfo::libc::getegid() },
        },
        app_data_dir: app_data_root.to_string_lossy().into_owned(),
        token: None,
    })
}
