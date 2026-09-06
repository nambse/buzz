use reqwest::{
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client, Method, StatusCode,
};
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use crate::{invalid, rejected, unavailable, MemoryError, ResolvedHonchoToken};

const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_REQUEST: usize = 1152 * 1024;

pub(crate) struct Http {
    client: Client,
    origin: Url,
    authorization: HeaderValue,
}

impl Http {
    pub(crate) fn new(
        origin: Url,
        token: ResolvedHonchoToken,
        timeout: std::time::Duration,
    ) -> Result<Self, MemoryError> {
        if token.secret.is_empty()
            || token.secret.len() > 16 * 1024
            || token
                .secret
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        {
            return Err(invalid("invalid memory authentication material"));
        }
        let material = Zeroizing::new(format!("Bearer {}", token.secret.as_str()));
        let mut authorization = HeaderValue::from_str(&material)
            .map_err(|_| invalid("invalid memory authentication material"))?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .timeout(timeout)
            .connect_timeout(timeout.min(std::time::Duration::from_secs(5)))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(|_| unavailable("memory HTTP client unavailable"))?;
        Ok(Self {
            client,
            origin,
            authorization,
        })
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value), MemoryError> {
        self.request_limited(method, path, body, MAX_REQUEST, MAX_BODY).await
    }

    /// A stricter family can lower both limits without changing legacy callers.
    pub(crate) async fn request_limited(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        request_limit: usize,
        response_limit: usize,
    ) -> Result<(StatusCode, Value), MemoryError> {
        if request_limit == 0 || request_limit > MAX_REQUEST || response_limit == 0 || response_limit > MAX_BODY {
            return Err(invalid("invalid memory wire ceiling"));
        }
        let url = self
            .origin
            .join(path)
            .map_err(|_| invalid("invalid memory operation path"))?;
        if url.origin() != self.origin.origin() {
            return Err(invalid("memory operation changed origin"));
        }
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone());
        if let Some(value) = body {
            let bytes = serde_json::to_vec(&value)
                .map_err(|_| invalid("invalid memory request encoding"))?;
            if bytes.len() > request_limit {
                return Err(invalid("memory request exceeds wire ceiling"));
            }
            request = request.header(CONTENT_TYPE, "application/json").body(bytes);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| unavailable("memory transport failed"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(
                if status.is_server_error()
                    || status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::REQUEST_TIMEOUT
                {
                    unavailable("memory service temporarily unavailable")
                } else if status == StatusCode::CONFLICT {
                    rejected("memory operation conflicts with current resources or receipt")
                } else {
                    rejected("memory service rejected the operation")
                },
            );
        }
        if status != StatusCode::OK && status != StatusCode::CREATED {
            return Err(rejected("unexpected memory response status"));
        }
        if response
            .content_length()
            .is_some_and(|n| n > response_limit as u64)
        {
            return Err(rejected("memory response exceeds wire ceiling"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("memory response interrupted"))?
        {
            if bytes.len().saturating_add(chunk.len()) > response_limit {
                return Err(rejected("memory response exceeds wire ceiling"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| rejected("invalid memory response encoding"))?;
        Ok((status, value))
    }
}
