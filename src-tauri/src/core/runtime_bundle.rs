use anyhow::{Context as _, Result, bail};
use clash_verge_service_ipc::{RemoteProvider, RuntimeAsset, RuntimeBundle};
use serde_yaml_ng::Value;
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

const GEO_ASSETS: &[&str] = &[
    "Country.mmdb",
    "geoip.dat",
    "geosite.dat",
    "geoip.metadb",
    "GeoSite.dat",
];

/// Build the complete runtime declaration consumed by service IPC v2.
///
/// Service mode has its own runtime directory, so every local provider and GeoData file must be
/// declared explicitly and provider paths in the generated YAML must be relative to that runtime.
pub(crate) async fn collect_runtime_bundle(config_file: &Path, core_path: &Path) -> Result<RuntimeBundle> {
    let yaml = tokio::fs::read_to_string(config_file)
        .await
        .with_context(|| format!("failed to read runtime config {config_file:?}"))?;
    let mut config: Value =
        serde_yaml_ng::from_str(&yaml).with_context(|| format!("failed to parse runtime config {config_file:?}"))?;
    let config_root = config_file.parent().context("runtime config has no parent directory")?;
    let config_root = std::fs::canonicalize(config_root)?;
    let mut destinations = HashSet::new();
    let mut assets = Vec::new();
    let mut remote_providers = Vec::new();

    for section in ["proxy-providers", "rule-providers"] {
        collect_provider_assets(
            &mut config,
            section,
            &config_root,
            &mut destinations,
            &mut assets,
            &mut remote_providers,
        )?;
    }

    for filename in GEO_ASSETS {
        let source = config_root.join(filename);
        if source.is_file() && destinations.insert((*filename).to_owned()) {
            assets.push(RuntimeAsset {
                source: std::fs::canonicalize(source)?.to_string_lossy().into_owned(),
                destination: (*filename).to_owned(),
            });
        }
    }

    Ok(RuntimeBundle {
        yaml: serde_yaml_ng::to_string(&config).context("failed to serialize service runtime config")?,
        assets,
        remote_providers,
        core_path: core_path.to_string_lossy().into_owned(),
    })
}

fn collect_provider_assets(
    config: &mut Value,
    section: &str,
    config_root: &Path,
    destinations: &mut HashSet<String>,
    assets: &mut Vec<RuntimeAsset>,
    remote_providers: &mut Vec<RemoteProvider>,
) -> Result<()> {
    let Some(providers) = config
        .as_mapping_mut()
        .and_then(|mapping| mapping.get_mut(section))
        .and_then(Value::as_mapping_mut)
    else {
        return Ok(());
    };

    for provider in providers.values_mut() {
        let Some(provider) = provider.as_mapping_mut() else {
            continue;
        };
        let Some(raw_path) = provider.get("path").and_then(Value::as_str) else {
            continue;
        };
        let destination = destination_for(config_root, raw_path)?;
        let is_remote = provider.get("type").and_then(Value::as_str) == Some("http");

        if is_remote {
            if let Some(url) = provider.get("url").and_then(Value::as_str) {
                if let Some(existing) = remote_providers.iter().find(|item| item.destination == destination) {
                    if existing.url != url {
                        bail!("runtime provider destination {destination:?} is declared for two different sources");
                    }
                } else {
                    if !destinations.insert(destination.clone()) {
                        bail!("runtime provider destination {destination:?} is claimed more than once");
                    }
                    remote_providers.push(RemoteProvider {
                        destination: destination.clone(),
                        url: url.to_owned(),
                    });
                }
                provider.insert(Value::String("path".to_owned()), Value::String(destination));
            }
            continue;
        }

        let source = local_provider_source(config_root, raw_path)?;
        if destinations.insert(destination.clone()) {
            assets.push(RuntimeAsset {
                source: source.to_string_lossy().into_owned(),
                destination: destination.clone(),
            });
        }
        provider.insert(Value::String("path".to_owned()), Value::String(destination));
    }
    Ok(())
}

fn local_provider_source(config_root: &Path, raw_path: &str) -> Result<PathBuf> {
    let source = if Path::new(raw_path).is_absolute() {
        PathBuf::from(raw_path)
    } else {
        config_root.join(raw_path)
    };
    let source =
        std::fs::canonicalize(&source).with_context(|| format!("local runtime provider is unavailable: {source:?}"))?;
    source
        .strip_prefix(config_root)
        .map_err(|_| anyhow::anyhow!("local runtime provider is outside the config root: {source:?}"))?;
    Ok(source)
}

fn destination_for(config_root: &Path, raw_path: &str) -> Result<String> {
    let path = Path::new(raw_path);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("runtime provider destination traverses outside the runtime");
    }
    if path.is_absolute() {
        let source =
            std::fs::canonicalize(path).with_context(|| format!("runtime provider path is unavailable: {path:?}"))?;
        let relative = source
            .strip_prefix(config_root)
            .map_err(|_| anyhow::anyhow!("runtime provider path is outside the config root: {source:?}"))?;
        normalized_destination(relative)
    } else {
        normalized_destination(path)
    }
}

fn normalized_destination(path: &Path) -> Result<String> {
    let mut destination = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => destination.push(component),
            Component::CurDir => {}
            _ => bail!("runtime provider destination traverses outside the runtime"),
        }
    }
    if destination.as_os_str().is_empty() {
        bail!("runtime provider destination is empty");
    }
    Ok(destination.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, reason = "tests assert by panicking")]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("clash-verge-runtime-{name}-{}-{nonce}", std::process::id()))
    }

    #[tokio::test]
    async fn bundles_local_assets_and_remote_providers() {
        let root = test_root("assets");
        fs::create_dir_all(root.join("providers")).unwrap();
        fs::write(root.join("providers/local.yaml"), "proxies: []").unwrap();
        fs::write(root.join("Country.mmdb"), "geodata").unwrap();
        let config = root.join("config.yaml");
        fs::write(
            &config,
            "proxy-providers:\n  local:\n    type: file\n    path: providers/local.yaml\n  remote:\n    type: http\n    path: providers/remote.yaml\n    url: https://example.com/remote.yaml\n",
        )
        .unwrap();

        let bundle = collect_runtime_bundle(&config, &root.join("mihomo")).await.unwrap();

        assert!(
            bundle
                .assets
                .iter()
                .any(|asset| asset.destination == "providers/local.yaml")
        );
        assert!(bundle.assets.iter().any(|asset| asset.destination == "Country.mmdb"));
        assert_eq!(bundle.remote_providers.len(), 1);
        assert_eq!(bundle.remote_providers[0].destination, "providers/remote.yaml");
        let rewritten: Value = serde_yaml_ng::from_str(&bundle.yaml).unwrap();
        assert_eq!(
            rewritten["proxy-providers"]["local"]["path"].as_str(),
            Some("providers/local.yaml")
        );
        assert_eq!(
            rewritten["proxy-providers"]["remote"]["path"].as_str(),
            Some("providers/remote.yaml")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rejects_provider_path_traversal() {
        let root = test_root("traversal");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("config.yaml");
        fs::write(
            &config,
            "rule-providers:\n  unsafe:\n    type: file\n    path: ../outside.yaml\n",
        )
        .unwrap();

        let error = collect_runtime_bundle(&config, &root.join("mihomo")).await.unwrap_err();
        assert!(error.to_string().contains("traverses outside"));

        fs::remove_dir_all(root).unwrap();
    }
}
