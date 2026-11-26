//! Exact-endpoint HTTPS. DNS, transport, credentials and authority remain in the host.
use crate::cancel::Cancel;
use hickory_resolver::{config::LookupIpStrategy, TokioResolver};
use reqwest::{header, Client, Method};
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    net::{IpAddr, SocketAddr},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use url::{Host, Url};
const MAX_BODY: usize = 64 * 1024;
static RATE: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());
static ADMISSION: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(4));
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Endpoint request unavailable or outside its approval.",
    )
}
fn failed() -> io::Error {
    io::Error::other("Approved HTTPS request failed.")
}
/// Host-selected, immutable endpoint. Never deserialize an approval from Lua.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    #[default]
    Text,
    Json,
}
impl ContentType {
    fn header(self) -> &'static str {
        match self {
            Self::Text => "text/plain; charset=utf-8",
            Self::Json => "application/json",
        }
    }
}
pub struct Endpoint {
    url: Url,
    method: Method,
    upload: usize,
    response: usize,
    content_type: ContentType,
}
impl Endpoint {
    pub fn reviewed(url: &str, post: bool, upload: usize, response: usize) -> io::Result<Self> {
        Self::reviewed_content(url, post, upload, response, ContentType::Text)
    }
    pub fn reviewed_content(
        url: &str,
        post: bool,
        upload: usize,
        response: usize,
        content_type: ContentType,
    ) -> io::Result<Self> {
        let parsed = Url::parse(url).map_err(|_| denied())?;
        if url.len() > 2048
            || parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || parsed.port_or_known_default() != Some(443)
            || upload > MAX_BODY
            || response == 0
            || response > MAX_BODY
            || (!post && upload != 0)
        {
            return Err(denied());
        }
        match parsed.host().ok_or_else(denied)? {
            Host::Domain(host)
                if host.contains('.')
                    && !host.ends_with('.')
                    && !host.eq_ignore_ascii_case("localhost") => {}
            Host::Ipv4(ip) if public(IpAddr::V4(ip)) => {}
            Host::Ipv6(ip) if public(IpAddr::V6(ip)) => {}
            _ => return Err(denied()),
        }
        Ok(Self {
            url: parsed,
            method: if post { Method::POST } else { Method::GET },
            upload,
            response,
            content_type,
        })
    }
    pub(crate) fn same_policy(&self, other: &Self) -> bool {
        self.url == other.url
            && self.method == other.method
            && self.upload == other.upload
            && self.response == other.response
            && self.content_type == other.content_type
    }
    pub fn url(&self) -> &str {
        self.url.as_str()
    }
    pub fn method(&self) -> &str {
        self.method.as_str()
    }
    pub fn content_type(&self) -> ContentType {
        self.content_type
    }
}
/// Conservative internet-only policy. Special-purpose and transition prefixes
/// are denied even when an exceptional address within them is globally routable.
/// Sources: IANA special-purpose registries; Azure's host virtual IP is excluded.
pub fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            let excluded = [
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ];
            value != 0xa83f8110
                && !excluded
                    .iter()
                    .any(|(base, bits)| value >> (32 - bits) == base >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let value = u128::from(ip);
            value>>125==1 // 2000::/3 only: excludes mapped/NAT64/local/multicast.
                && !(ip.segments()[0]==0x2001&&ip.segments()[1]<0x0200)
                && !(ip.segments()[0]==0x2001&&ip.segments()[1]==0x0db8)
                && ip.segments()[0]!=0x2002
                && !(ip.segments()[0]==0x3fff&&ip.segments()[1]<0x1000)
        }
    }
}
fn destinations(addresses: impl IntoIterator<Item = IpAddr>) -> io::Result<Vec<SocketAddr>> {
    let mut approved = BTreeSet::new();
    for address in addresses {
        if !public(address) || approved.len() >= 16 {
            return Err(denied());
        }
        approved.insert(SocketAddr::new(address, 443));
    }
    if approved.is_empty() {
        return Err(denied());
    }
    Ok(approved.into_iter().collect())
}
async fn resolve(endpoint: &Endpoint) -> io::Result<Vec<SocketAddr>> {
    match endpoint.url.host().ok_or_else(denied)? {
        Host::Ipv4(ip) => destinations([IpAddr::V4(ip)]),
        Host::Ipv6(ip) => destinations([IpAddr::V6(ip)]),
        Host::Domain(host) => {
            let mut builder = TokioResolver::builder_tokio().map_err(|_| failed())?;
            let options = builder.options_mut();
            options.timeout = Duration::from_secs(2);
            options.attempts = 1;
            options.cache_size = 0;
            options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
            let resolver = builder.build().map_err(|_| failed())?;
            let lookup = tokio::time::timeout(
                Duration::from_secs(3),
                resolver.lookup_ip(format!("{host}.")),
            )
            .await
            .map_err(|_| failed())?
            .map_err(|_| failed())?;
            destinations(lookup.iter())
        }
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Response {
    pub status: u16,
    pub body: String,
}
/// Allocate one rate limiter per authenticated owner, retained across requests.
pub struct Network {
    last: Mutex<Option<Instant>>,
}
impl Default for Network {
    fn default() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }
}
impl Network {
    pub async fn execute<G>(
        &self,
        endpoint: &Endpoint,
        body: String,
        cancel: &Cancel,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Response> {
        if cancel.is_cancelled() || body.len() > endpoint.upload || body.contains('\0') {
            return Err(denied());
        }
        let _permit = ADMISSION
            .try_acquire()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "Endpoint broker is busy."))?;
        {
            let mut last = self.last.lock().map_err(|_| failed())?;
            if last.is_some_and(|last| last.elapsed() < Duration::from_secs(5)) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Endpoint request rate limit reached.",
                ));
            }
            *last = Some(Instant::now());
        }
        {
            let mut rate = RATE.lock().map_err(|_| failed())?;
            while rate
                .front()
                .is_some_and(|at| at.elapsed() >= Duration::from_secs(30))
            {
                rate.pop_front();
            }
            if rate.len() >= 32 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Global endpoint request rate limit reached.",
                ));
            }
            rate.push_back(Instant::now());
        }
        let operation = async {
            {
                let _guard = authority()?;
            }
            let addresses = resolve(endpoint).await?;
            // No pooled connection or subsequent DNS lookup can escape the pinned set.
            let client = Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .referer(false)
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(8))
                .pool_max_idle_per_host(0)
                .http1_only()
                .resolve_to_addrs(endpoint.url.host_str().ok_or_else(denied)?, &addresses)
                .build()
                .map_err(|_| failed())?;
            // Scope revocation cancels the request first; this disclosure guard
            // is retained until transport and response validation finish.
            let _guard = authority()?;
            if cancel.is_cancelled() {
                return Err(denied());
            }
            transfer(endpoint, body, &addresses, &client).await
        };
        tokio::select! {biased;_=cancel.cancelled()=>Err(io::Error::new(io::ErrorKind::Interrupted,"Endpoint request cancelled.")),result=tokio::time::timeout(Duration::from_secs(10),operation)=>result.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"Endpoint request timed out."))?}
    }
}
async fn transfer(
    endpoint: &Endpoint,
    body: String,
    addresses: &[SocketAddr],
    client: &Client,
) -> io::Result<Response> {
    let mut response = client
        .request(endpoint.method.clone(), endpoint.url.clone())
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONTENT_TYPE, endpoint.content_type.header())
        .body(body)
        .send()
        .await
        .map_err(|_| failed())?;
    if !response
        .remote_addr()
        .is_some_and(|address| addresses.contains(&address))
        || response.status().is_redirection()
    {
        return Err(denied());
    }
    validate_headers(
        response.headers(),
        response.content_length(),
        endpoint.response,
    )?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
        if chunk.len() > endpoint.response.saturating_sub(bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Endpoint response exceeds its approval.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(bytes).map_err(|_| failed())?;
    if body.contains('\0') {
        return Err(failed());
    }
    Ok(Response { status, body })
}
fn validate_headers(
    headers: &header::HeaderMap,
    length: Option<u64>,
    maximum: usize,
) -> io::Result<()> {
    if headers.len() > 32
        || headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            > 16 * 1024
    {
        return Err(failed());
    }
    if length.is_some_and(|length| length > maximum as u64)
        || headers
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .any(|encoding| encoding.as_bytes() != b"identity")
    {
        return Err(failed());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn special_use_metadata_and_transition_addresses_are_not_internet_destinations() {
        for address in [
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.31.255.255",
            "192.0.0.9",
            "192.0.2.3",
            "192.168.1.1",
            "198.19.0.1",
            "198.51.100.1",
            "203.0.113.5",
            "224.0.0.1",
            "255.255.255.255",
            "168.63.129.16",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2001::1",
            "2001:db8::1",
            "2001:20::1",
            "2002:808:808::1",
            "3fff::1",
            "fc00::1",
            "fe80::1",
            "ff00::1",
        ] {
            assert!(!public(address.parse().unwrap()), "{address}");
        }
        for address in [
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
        ] {
            assert!(public(address.parse().unwrap()), "{address}");
        }
        assert!(destinations(["8.8.8.8".parse().unwrap(), "127.0.0.1".parse().unwrap()]).is_err());
    }
    #[test]
    fn endpoints_disallow_ambient_authority_and_responses_cannot_expand_or_redirect() {
        for url in [
            "http://example.com/",
            "https://localhost/",
            "https://127.1/",
            "https://2130706433/",
            "https://[::1]/",
            "https://example.com:444/",
            "https://user:secret@example.com/",
            "https://example.com/#secret",
            "file:///tmp/fixture",
        ] {
            assert!(Endpoint::reviewed(url, true, 64, 64).is_err(), "{url}");
        }
        assert!(Endpoint::reviewed("https://example.com/translate", true, 64, 64).is_ok());
        let mut headers = header::HeaderMap::new();
        assert!(validate_headers(&headers, Some(65), 64).is_err());
        headers.insert(
            header::CONTENT_ENCODING,
            header::HeaderValue::from_static("gzip"),
        );
        assert!(validate_headers(&headers, None, 64).is_err());
        headers.clear();
        headers.insert(
            header::HeaderName::from_static("x-long"),
            header::HeaderValue::from_bytes(&vec![b'a'; 16 * 1024]).unwrap(),
        );
        assert!(validate_headers(&headers, None, 64).is_err());
        headers.clear();
        for i in 0..33 {
            headers.append(
                header::HeaderName::from_bytes(format!("x-{i}").as_bytes()).unwrap(),
                header::HeaderValue::from_static("a"),
            );
        }
        assert!(validate_headers(&headers, None, 64).is_err());
        let text = Endpoint::reviewed("https://example.com/translate", true, 64, 64).unwrap();
        let json = Endpoint::reviewed_content(
            "https://example.com/translate",
            true,
            64,
            64,
            ContentType::Json,
        )
        .unwrap();
        assert!(!text.same_policy(&json));
    }
    #[tokio::test]
    async fn cancelled_and_unapproved_uploads_fail_before_dns_or_transport() {
        let network = Network::default();
        let endpoint = Endpoint::reviewed("https://example.com/", true, 8, 64).unwrap();
        let cancel = Cancel::default();
        let mut checked = false;
        assert!(network
            .execute(&endpoint, "too much text".into(), &cancel, || {
                checked = true;
                Ok(())
            })
            .await
            .is_err());
        assert!(!checked);
        cancel.cancel();
        assert!(network
            .execute(&endpoint, "fixture".into(), &cancel, || {
                checked = true;
                Ok(())
            })
            .await
            .is_err());
        assert!(!checked);
    }
}
#[cfg(test)]
mod transport_tests;
