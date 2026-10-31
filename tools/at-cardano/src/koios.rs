//! The Koios adaptor: fixed hosts, fixed verbs, no redirects.
//!
//! Everything that can reach the network lives in this module. `query` may name any Koios
//! endpoint, but it cannot choose a host, add a header, or reach the one endpoint that writes
//! to the chain — `/submittx` is refused before a socket is opened.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::contract::Fail;
use crate::key::Auth;

/// A response larger than this is refused rather than buffered. Koios caps a page at 1000
/// rows, so this is already far beyond any legitimate answer.
pub const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Preprod,
    Preview,
    Guild,
}

impl Network {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "mainnet" => Some(Self::Mainnet),
            "preprod" => Some(Self::Preprod),
            "preview" => Some(Self::Preview),
            "guild" => Some(Self::Guild),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Preprod => "preprod",
            Self::Preview => "preview",
            Self::Guild => "guild",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Mainnet => "https://api.koios.rest/api/v1",
            Self::Preprod => "https://preprod.koios.rest/api/v1",
            Self::Preview => "https://preview.koios.rest/api/v1",
            Self::Guild => "https://guild.koios.rest/api/v1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

pub struct Client {
    agent: ureq::Agent,
    network: Network,
    auth: Auth,
    /// Only unit tests use another base URL; the CLI has no flag that can set one.
    #[cfg(test)]
    base_url: String,
}

impl Client {
    pub fn new(network: Network, auth: Auth) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(60)))
                .user_agent(format!("{}/{}", crate::TOOL, crate::VERSION))
                .build(),
        );
        Self {
            agent,
            network,
            auth,
            #[cfg(test)]
            base_url: network.base_url().to_string(),
        }
    }

    #[cfg(test)]
    fn with_base_url(base_url: String, auth: Auth) -> Self {
        let mut client = Self::new(Network::Mainnet, auth);
        client.base_url = base_url.trim_end_matches('/').to_string();
        client
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    pub fn get(&self, endpoint: &str, query: &[(String, String)]) -> Result<Vec<u8>, Fail> {
        self.request(Method::Get, endpoint, query, None)
    }

    pub fn post(
        &self,
        endpoint: &str,
        query: &[(String, String)],
        body: &[u8],
    ) -> Result<Vec<u8>, Fail> {
        self.request(Method::Post, endpoint, query, Some(body))
    }

    fn request(
        &self,
        method: Method,
        endpoint: &str,
        query: &[(String, String)],
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>, Fail> {
        let url = self.url(endpoint, query)?;
        let bearer = self.auth.key().map(|key| format!("Bearer {key}"));

        let mut response = match method {
            Method::Get => {
                let request = self.agent.get(&url).header("accept", "application/json");
                let request = match &bearer {
                    Some(value) => request.header("authorization", value),
                    None => request,
                };
                request.call()
            }
            Method::Post => {
                let request = self
                    .agent
                    .post(&url)
                    .header("accept", "application/json")
                    .header("content-type", "application/json");
                let request = match &bearer {
                    Some(value) => request.header("authorization", value),
                    None => request,
                };
                request.send(body.unwrap_or(&b"{}"[..]))
            }
        }
        .map_err(|error| {
            Fail::environment(format!(
                "{} /{} did not complete: {error}",
                method.as_str(),
                endpoint
            ))
        })?;

        let status = response.status().as_u16();
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|error| {
                Fail::environment(format!(
                    "{} /{} answered {status} but the body could not be read: {error}",
                    method.as_str(),
                    endpoint
                ))
            })?;

        if (200..300).contains(&status) {
            return Ok(bytes);
        }

        let excerpt: String = String::from_utf8_lossy(&bytes).chars().take(400).collect();
        let excerpt = if excerpt.is_empty() {
            "(empty body)".to_string()
        } else {
            excerpt
        };
        let message = format!(
            "Koios answered HTTP {status} for {} /{endpoint}: {excerpt}",
            method.as_str()
        );
        match status {
            401 | 403 => Err(Fail::environment(format!(
                "{message} · the key was rejected; retry with --keyless for the free tier"
            ))),
            429 => Err(Fail::environment(format!(
                "{message} · rate limited; a key in ~/.config/koios/api_key raises the limit"
            ))),
            400 | 404 | 422 => Err(Fail::usage(message)),
            _ => Err(Fail::environment(message)),
        }
    }

    fn url(&self, endpoint: &str, query: &[(String, String)]) -> Result<String, Fail> {
        if refused_endpoint(endpoint) {
            return Err(Fail::refused(format!(
                "/{endpoint} is a write to the chain; at-cardano answers reads only"
            )));
        }
        if !valid_endpoint(endpoint) {
            return Err(Fail::usage(format!(
                "`{endpoint}` is not a Koios endpoint name · use lowercase letters, digits and `_`"
            )));
        }
        #[cfg(not(test))]
        let base = self.network.base_url();
        #[cfg(test)]
        let base = self.base_url.as_str();

        let mut url = format!("{base}/{endpoint}");
        if !query.is_empty() {
            url.push('?');
            for (index, (key, value)) in query.iter().enumerate() {
                if index > 0 {
                    url.push('&');
                }
                url.push_str(&percent_encode(key));
                url.push('=');
                url.push_str(&percent_encode(value));
            }
        }
        Ok(url)
    }
}

/// A Koios endpoint is one path segment of lowercase letters, digits and underscores. It is
/// deliberately narrower than a URL: no scheme, host, port, traversal or query string can be
/// expressed, so an approval over `query` is an approval over `/api/v1/<name>`.
pub fn valid_endpoint(endpoint: &str) -> bool {
    let mut characters = endpoint.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// The only write Koios exposes. Refused by name rather than by a method check, because a POST
/// is how most of the read API is queried.
pub fn refused_endpoint(endpoint: &str) -> bool {
    endpoint == "submittx" || endpoint.starts_with("submit")
}

pub fn parse_rows<T: DeserializeOwned>(bytes: &[u8], endpoint: &str) -> Result<Vec<T>, Fail> {
    serde_json::from_slice(bytes).map_err(|error| {
        Fail::environment(format!(
            "Koios /{endpoint} answered JSON this tool could not read: {error}"
        ))
    })
}

pub fn json_body<T: Serialize>(value: &T) -> Result<Vec<u8>, Fail> {
    serde_json::to_vec(value)
        .map_err(|error| Fail::environment(format!("could not encode the request body: {error}")))
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'*') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Auth {
        Auth::for_test(Some("sentinel-key"), None)
    }

    #[test]
    fn endpoint_names_are_one_safe_segment() {
        assert!(valid_endpoint("tx_info"));
        assert!(valid_endpoint("address_utxos"));
        assert!(!valid_endpoint("/tx_info"));
        assert!(!valid_endpoint("tx-info"));
        assert!(!valid_endpoint("tx_info?limit=1"));
        assert!(!valid_endpoint("Tx_info"));
        assert!(!valid_endpoint(""));
    }

    #[test]
    fn transaction_submission_is_refused_by_name() {
        assert!(refused_endpoint("submittx"));
        assert!(refused_endpoint("submit"));
        assert!(!refused_endpoint("tx_status"));
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(percent_encode("block_time.desc"), "block_time.desc");
        assert_eq!(percent_encode("a,b"), "a%2Cb");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("a&b"), "a%26b");
    }

    #[test]
    fn every_network_has_one_https_host() {
        for network in [
            Network::Mainnet,
            Network::Preprod,
            Network::Preview,
            Network::Guild,
        ] {
            assert!(network.base_url().starts_with("https://"));
            assert!(network.base_url().ends_with("/api/v1"));
            assert_eq!(Network::parse(network.label()), Some(network));
        }
    }

    #[test]
    fn a_refused_endpoint_never_reaches_a_socket() {
        let client = Client::with_base_url("http://127.0.0.1:1".to_string(), auth());
        let error = client.get("submittx", &[]).expect_err("refused");
        assert_eq!(error.exit.code(), 4);
        assert!(
            error.message.contains("write to the chain"),
            "{}",
            error.message
        );
    }
}
