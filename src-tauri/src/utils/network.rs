use crate::config::Config;
use anyhow::{Context as _, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::{
    Client, Proxy, StatusCode,
    header::{HeaderMap, HeaderValue, LOCATION, USER_AGENT},
};
use smartstring::alias::String;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use sysproxy::Sysproxy;
use tauri::Url;
use tokio::net::lookup_host;

#[derive(Debug)]
pub struct HttpResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl HttpResponse {
    pub const fn new(status: StatusCode, headers: HeaderMap, body: String) -> Self {
        Self { status, headers, body }
    }

    pub const fn status(&self) -> StatusCode {
        self.status
    }

    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn text_with_charset(&self) -> Result<&str> {
        Ok(&self.body)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProxyType {
    None,
    Localhost,
    System,
}

#[derive(Debug, Clone, Copy)]
enum TlsRootMode {
    PlatformVerifier,
    StaticWebpkiRoots,
}

pub struct NetworkManager;

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkManager {
    const MAX_REDIRECTS: usize = 10;

    pub const fn new() -> Self {
        Self
    }

    fn build_client(
        &self,
        proxy_url: Option<std::string::String>,
        default_headers: HeaderMap,
        accept_invalid_certs: bool,
        timeout_secs: Option<u64>,
        tls_root_mode: TlsRootMode,
        pinned_destination: Option<&ValidatedDestination>,
    ) -> Result<Client> {
        let mut builder = Client::builder()
            .tls_backend_rustls()
            .redirect(reqwest::redirect::Policy::none())
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(None);

        if matches!(tls_root_mode, TlsRootMode::StaticWebpkiRoots) {
            builder = builder.tls_backend_preconfigured(Self::build_static_webpki_tls_config()?);
        }

        // 设置代理
        if let Some(proxy_str) = proxy_url {
            let proxy = Proxy::all(proxy_str)?;
            builder = builder.proxy(proxy);
        } else {
            builder = builder.no_proxy();
        }

        builder = builder.default_headers(default_headers);

        if let Some(destination) = pinned_destination {
            builder = builder.resolve_to_addrs(destination.host.as_str(), destination.addrs.as_slice());
        }

        // SSL/TLS
        if accept_invalid_certs {
            builder = builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }

        // 超时设置
        if let Some(secs) = timeout_secs {
            builder = builder
                .timeout(Duration::from_secs(secs))
                .connect_timeout(Duration::from_secs(secs.min(30)));
        }

        Ok(builder.build()?)
    }

    fn build_static_webpki_tls_config() -> Result<rustls::ClientConfig> {
        let root_store = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config =
            rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_no_client_auth();

        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(config)
    }

    fn should_retry_with_static_webpki_roots(err: &anyhow::Error) -> bool {
        if err.chain().any(Self::is_legacy_tls_protocol_error) {
            return false;
        }

        err.chain().any(|e| {
            let msg = e.to_string().to_ascii_lowercase();
            [
                "certificate",
                "cert",
                "tls",
                "ssl",
                "rustls",
                "webpki",
                "revocation",
                "ocsp",
                "crl",
                "issuer",
                "unknownissuer",
            ]
            .iter()
            .any(|kw| msg.contains(kw))
        })
    }

    fn context_reqwest_error(err: reqwest::Error, context: &'static str) -> anyhow::Error {
        let legacy_tls = Self::is_legacy_tls_protocol_error(&err);
        let err = anyhow::Error::new(err).context(context);

        if legacy_tls {
            err.context("Subscription server uses legacy TLS; only TLS 1.2/1.3 is supported. TLS 1.0/1.1 is insecure")
        } else {
            err
        }
    }

    fn is_legacy_tls_protocol_error(err: &(dyn std::error::Error + 'static)) -> bool {
        let detail = format!("{err:#?}").to_ascii_lowercase();
        detail.contains("protocolversion") || detail.contains("protocol version")
    }

    pub async fn ensure_public_destination(url: &str) -> Result<()> {
        let parsed = Url::parse(url)?;
        resolve_public_destination(&parsed).await.map(|_| ())
    }

    pub async fn create_request(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
    ) -> Result<Client> {
        self.create_request_with_tls_mode(
            proxy_type,
            timeout_secs,
            user_agent,
            accept_invalid_certs,
            TlsRootMode::PlatformVerifier,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_with_tls_mode(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        tls_root_mode: TlsRootMode,
        pinned_destination: Option<&ValidatedDestination>,
    ) -> Result<HttpResponse> {
        let mut current_url = Url::parse(url)?;
        let mut current_pinned_destination = pinned_destination.cloned();

        for redirect_count in 0..=Self::MAX_REDIRECTS {
            let (request_url, extra_headers) = Self::prepare_request_url_and_headers(&current_url)?;

            let pinned_destination = match proxy_type {
                ProxyType::None => {
                    if let Some(destination) = current_pinned_destination.as_ref() {
                        destination.clone()
                    } else {
                        Self::resolve_public_destination_for_request(current_url.as_str())
                            .await?
                            .ok_or_else(|| anyhow!("URL host could not be resolved"))?
                    }
                }
                ProxyType::Localhost | ProxyType::System => {
                    Self::resolve_public_destination_for_request(current_url.as_str())
                        .await?
                        .ok_or_else(|| anyhow!("URL host could not be resolved"))?
                }
            };

            // 创建请求
            let client = self
                .create_request_with_tls_mode(
                    proxy_type,
                    timeout_secs,
                    user_agent.clone(),
                    accept_invalid_certs,
                    tls_root_mode,
                    Some(&pinned_destination),
                )
                .await?;

            let mut request_builder = client.get(request_url);

            for (key, value) in extra_headers.iter() {
                request_builder = request_builder.header(key, value);
            }

            let response = match request_builder.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    return Err(Self::context_reqwest_error(e, "Request failed"));
                }
            };

            let status = response.status();
            let headers = response.headers().to_owned();

            if status.is_redirection()
                && let Some(location) = headers.get(LOCATION)
            {
                if redirect_count >= Self::MAX_REDIRECTS {
                    bail!("too many redirects while fetching remote profile");
                }

                let location = location.to_str().context("redirect location is not valid UTF-8")?;
                current_url = Self::resolve_redirect_url(&current_url, location)?;
                current_pinned_destination = None;
                continue;
            }

            let body = match response.text().await {
                Ok(text) => text.into(),
                Err(e) => {
                    return Err(Self::context_reqwest_error(e, "Failed to read response body"));
                }
            };

            return Ok(HttpResponse::new(status, headers, body));
        }

        bail!("too many redirects while fetching remote profile")
    }

    fn prepare_request_url_and_headers(url: &Url) -> Result<(Url, HeaderMap)> {
        let mut request_url = url.clone();
        let mut extra_headers = HeaderMap::new();

        if !request_url.username().is_empty()
            && let Some(pass) = request_url.password()
        {
            let username = percent_encoding::percent_decode_str(request_url.username())
                .decode_utf8_lossy()
                .into_owned();
            let password = percent_encoding::percent_decode_str(pass)
                .decode_utf8_lossy()
                .into_owned();
            let auth_str = format!("{}:{}", username, password);
            let encoded = general_purpose::STANDARD.encode(auth_str);
            extra_headers.insert("Authorization", HeaderValue::from_str(&format!("Basic {}", encoded))?);
        }

        request_url.set_username("").ok();
        request_url.set_password(None).ok();

        Ok((request_url, extra_headers))
    }

    fn resolve_redirect_url(current_url: &Url, location: &str) -> Result<Url> {
        let next_url = current_url.join(location.trim())?;
        match next_url.scheme() {
            "http" | "https" => Ok(next_url),
            scheme => bail!("unsupported redirect url scheme: {scheme}"),
        }
    }

    async fn create_request_with_tls_mode(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        tls_root_mode: TlsRootMode,
        pinned_destination: Option<&ValidatedDestination>,
    ) -> Result<Client> {
        let proxy_url: Option<std::string::String> = match proxy_type {
            ProxyType::None => None,
            ProxyType::Localhost => {
                let port = {
                    let verge_port = Config::verge().await.data_arc().verge_mixed_port;
                    match verge_port {
                        Some(port) => port,
                        None => Config::clash().await.data_arc().get_mixed_port(),
                    }
                };
                Some(format!("http://127.0.0.1:{port}"))
            }
            ProxyType::System => {
                if let Ok(p @ Sysproxy { enable: true, .. }) = Sysproxy::get_system_proxy() {
                    Some(format!("http://{}:{}", p.host, p.port))
                } else {
                    None
                }
            }
        };

        let mut headers = HeaderMap::new();

        // 设置 User-Agent
        if let Some(ua) = user_agent {
            headers.insert(USER_AGENT, HeaderValue::from_str(ua.as_str())?);
        } else {
            headers.insert(
                USER_AGENT,
                HeaderValue::from_str(&format!("clash-verge/v{}", env!("CARGO_PKG_VERSION")))?,
            );
        }

        let pinned_destination = if proxy_url.is_some() { None } else { pinned_destination };

        self.build_client(
            proxy_url,
            headers,
            accept_invalid_certs,
            timeout_secs,
            tls_root_mode,
            pinned_destination,
        )
    }

    pub async fn get_with_interrupt(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
    ) -> Result<HttpResponse> {
        let pinned_destination = Self::resolve_public_destination_for_request(url).await?;

        let platform_result = self
            .get_with_tls_mode(
                url,
                proxy_type,
                timeout_secs,
                user_agent.clone(),
                accept_invalid_certs,
                TlsRootMode::PlatformVerifier,
                pinned_destination.as_ref(),
            )
            .await;

        match platform_result {
            Ok(response) => Ok(response),
            Err(err) if !accept_invalid_certs && Self::should_retry_with_static_webpki_roots(&err) => self
                .get_with_tls_mode(
                    url,
                    proxy_type,
                    timeout_secs,
                    user_agent,
                    accept_invalid_certs,
                    TlsRootMode::StaticWebpkiRoots,
                    pinned_destination.as_ref(),
                )
                .await
                .map_err(|fallback_err| {
                    fallback_err.context("static webpki roots fallback failed after platform TLS verifier failed")
                }),
            Err(err) => Err(err),
        }
    }

    pub async fn resolve_public_destination_for_request(url: &str) -> Result<Option<ValidatedDestination>> {
        let parsed = Url::parse(url)?;
        let destination = resolve_public_destination(&parsed).await?;
        Ok(Some(destination))
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedDestination {
    pub(crate) host: std::string::String,
    pub(crate) addrs: Vec<SocketAddr>,
}

async fn resolve_public_destination(url: &Url) -> Result<ValidatedDestination> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => bail!("unsupported url scheme: {scheme}"),
    }

    let host = url.host_str().ok_or_else(|| anyhow!("URL missing host"))?;
    if is_localhost_name(host) {
        bail!("localhost destinations are not allowed");
    }

    let port = url.port_or_known_default().ok_or_else(|| anyhow!("URL missing port"))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            bail!("private or local destinations are not allowed");
        }
        return Ok(ValidatedDestination {
            host: host.into(),
            addrs: vec![SocketAddr::new(ip, port)],
        });
    }

    let mut saw_addr = false;
    let mut addrs = Vec::new();

    for addr in lookup_host((host, port)).await? {
        saw_addr = true;
        if !is_public_ip(addr.ip()) {
            bail!("private or local destinations are not allowed");
        }
        addrs.push(addr);
    }

    if !saw_addr {
        bail!("URL host could not be resolved");
    }

    Ok(ValidatedDestination {
        host: host.into(),
        addrs,
    })
}

#[allow(clippy::missing_const_for_fn)]
fn is_localhost_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.")
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_multicast() {
        return false;
    }

    let [a, b, c, _] = ip.octets();
    if a == 0 || a >= 240 {
        return false;
    }

    if a == 100 && (64..=127).contains(&b) {
        return false;
    }

    if a == 192 && b == 0 && c == 0 {
        return false;
    }

    if a == 192 && b == 0 && c == 2 {
        return false;
    }

    if a == 198 && b == 51 && c == 100 {
        return false;
    }

    if a == 198 && (18..=19).contains(&b) {
        return false;
    }

    if a == 203 && b == 0 && c == 113 {
        return false;
    }

    true
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped()
        && !is_public_ipv4(ipv4)
    {
        return false;
    }

    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
    {
        return false;
    }

    let segments = ip.segments();
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return false;
    }

    if (segments[0] & 0xffc0) == 0xfec0 {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::NetworkManager;
    use anyhow::Result;
    use tauri::Url;

    #[tokio::test]
    async fn ensure_public_destination_rejects_local_targets() {
        assert!(
            NetworkManager::ensure_public_destination("http://127.0.0.1")
                .await
                .is_err()
        );
        assert!(NetworkManager::ensure_public_destination("http://[::1]").await.is_err());
        assert!(
            NetworkManager::ensure_public_destination("https://localhost")
                .await
                .is_err()
        );
        assert!(
            NetworkManager::ensure_public_destination("ftp://example.com")
                .await
                .is_err()
        );
        assert!(
            NetworkManager::ensure_public_destination("http://[::ffff:127.0.0.1]")
                .await
                .is_err()
        );
        assert!(
            NetworkManager::ensure_public_destination("http://198.18.0.1")
                .await
                .is_err()
        );
    }

    #[test]
    fn resolve_redirect_url_limits_scheme_and_supports_relative_location() -> Result<()> {
        let current = Url::parse("https://example.com/sub/path?token=1")?;
        let next = NetworkManager::resolve_redirect_url(&current, "../download/config.yaml")?;
        assert_eq!(next.as_str(), "https://example.com/download/config.yaml");

        assert!(NetworkManager::resolve_redirect_url(&current, "file:///etc/passwd").is_err());
        assert!(NetworkManager::resolve_redirect_url(&current, "ftp://example.com/config.yaml").is_err());

        Ok(())
    }
}
