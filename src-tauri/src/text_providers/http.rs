//! Shared HTTP helpers for dictation and Text Assist providers.
//!
//! Moved from `openai.rs` so dictation and Text Assist share one client policy, one retry
//! policy and one redaction rule. Messages carry a provider label; with `"OpenAI"` they are
//! byte-identical to the previous dictation messages (`diagnostic_code` matches on them).
//! Errors never contain prompts, input text, response bodies or keys.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use reqwest::StatusCode;

use super::TextError;
use crate::types::DiagnosticCode;

/// Production HTTPS client: 10 s connect timeout, `timeout` total, HTTPS only, no redirects.
pub fn client(provider: &str, timeout: Duration) -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .with_context(|| format!("Could not create {provider} HTTP client"))
}

/// Sends a request, retrying once on a connection error, timeout or transient status, all
/// within one shared `timeout` budget. Each attempt gets the remaining budget as its total
/// request timeout, which also bounds reading the response body.
pub async fn send_with_retry(
    provider: &str,
    build: impl Fn() -> reqwest::RequestBuilder,
    timeout: Duration,
) -> anyhow::Result<reqwest::Response> {
    let deadline = Instant::now() + timeout;
    for attempt in 0..2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(!remaining.is_zero(), "{provider} request timed out");
        let result = build().timeout(remaining).send().await;
        let delay = retry_delay(&result);
        if attempt == 0 {
            if let Some(delay) = delay {
                if delay < deadline.saturating_duration_since(Instant::now()) {
                    // Drop the failed response before holding its socket open
                    // during the retry delay. Requests have a shared deadline.
                    drop(result);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
        }
        return result.with_context(|| format!("{provider} HTTP request failed"));
    }
    unreachable!("two attempts always return a result")
}

pub fn retry_delay(result: &Result<reqwest::Response, reqwest::Error>) -> Option<Duration> {
    let retry = match result {
        Ok(response) => retryable_status(response.status()),
        Err(err) => err.is_connect() || err.is_timeout(),
    };
    if !retry {
        return None;
    }
    if let Ok(response) = result {
        if let Some(value) = response.headers().get(reqwest::header::RETRY_AFTER) {
            // HTTP dates and very long delays defer to the user's next attempt;
            // never retry earlier than a server-specified delay we cannot honor.
            let seconds = value.to_str().ok()?.parse::<u64>().ok()?;
            if seconds > 5 {
                return None;
            }
            return Some(Duration::from_secs(seconds) + retry_jitter());
        }
    }
    Some(Duration::from_millis(750) + retry_jitter())
}

pub fn retryable_status(status: StatusCode) -> bool {
    // 429 can also indicate exhausted quota or a billing limit. Return it to
    // the GUI for user action rather than blindly repeating a billable call.
    matches!(status.as_u16(), 408 | 409 | 500 | 502 | 503 | 504)
}

fn retry_jitter() -> Duration {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64
        % 200;
    Duration::from_millis(millis)
}

/// Redacted error for a non-success response: provider, operation, status and a validated
/// request ID only. The response body is never read.
pub fn http_error(provider: &str, operation: &str, response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        });
    let request_id = request_id.unwrap_or("unavailable");
    // Response bodies may echo private input. Diagnostics expose only the
    // status and support request ID, never prompts, audio or response bodies.
    anyhow::anyhow!(
        "{provider} {operation} failed [{}] ({status}); request ID: {request_id}",
        status.as_u16()
    )
}

/// Maps a dictation error (from [`send_with_retry`] / [`http_error`]) to a diagnostic code.
pub fn diagnostic_code(err: &anyhow::Error) -> DiagnosticCode {
    if let Some(http_err) = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
    {
        if http_err.is_timeout() {
            return DiagnosticCode::NetworkTimeout;
        }
        if http_err.is_connect() {
            return DiagnosticCode::NetworkUnavailable;
        }
    }
    let text = format!("{err:#}").to_lowercase();
    if text.contains("[401]") || text.contains("[403]") {
        DiagnosticCode::InvalidApiKey
    } else if text.contains("[429]") {
        DiagnosticCode::RateLimited
    } else if text.contains("[500]")
        || text.contains("[502]")
        || text.contains("[503]")
        || text.contains("[504]")
    {
        DiagnosticCode::OpenAiServerError
    } else if text.contains("timed out") || text.contains("timeout") {
        DiagnosticCode::NetworkTimeout
    } else if text.contains("dns")
        || text.contains("network")
        || text.contains("connection")
        || text.contains("failed to lookup address")
    {
        DiagnosticCode::NetworkUnavailable
    } else {
        DiagnosticCode::OpenAiBadResponse
    }
}

/// Text Assist error for a non-success HTTP status.
pub fn status_error(status: StatusCode) -> TextError {
    match status.as_u16() {
        401 | 403 => TextError::InvalidKey,
        429 => TextError::RateLimited,
        code @ 500..=599 => TextError::Server(code),
        code => TextError::Rejected(code),
    }
}

/// Text Assist error for a transport failure.
pub fn transport_error(err: &reqwest::Error) -> TextError {
    if err.is_timeout() {
        TextError::Timeout
    } else {
        // Connection failures and every other transport error (reset, decode, body).
        TextError::Network
    }
}

/// Text Assist error for a failed [`send_with_retry`] call.
pub fn send_error(err: &anyhow::Error) -> TextError {
    match err
        .chain()
        .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
    {
        Some(http_err) => transport_error(http_err),
        // The only error without a transport cause is the exhausted shared deadline.
        None => TextError::Timeout,
    }
}

/// Loopback-only test client. Avoids Windows native TLS initialization and trust-store work
/// in timed tests. None of these test-only settings changes the production client above.
#[cfg(test)]
pub(crate) fn fixture_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .build()
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_transient_statuses() {
        for status in [408, 409, 500, 502, 503, 504] {
            assert!(retryable_status(StatusCode::from_u16(status).unwrap()));
        }
        for status in [200, 400, 401, 403, 413, 429] {
            assert!(!retryable_status(StatusCode::from_u16(status).unwrap()));
        }
    }

    #[test]
    fn http_error_does_not_expose_private_response_body() {
        let response = mock_response(401, "private transcript and API key", None);
        let error = http_error("OpenAI", "transcription", response);
        let message = error.to_string();
        assert!(message.contains("[401]"));
        assert!(message.contains("req_test"));
        assert!(!message.contains("private transcript"));
        assert!(matches!(
            diagnostic_code(&error),
            DiagnosticCode::InvalidApiKey
        ));
    }

    #[test]
    fn dictation_messages_are_unchanged() {
        let response = mock_response(429, "", None);
        assert_eq!(
            http_error("OpenAI", "cleanup", response).to_string(),
            "OpenAI cleanup failed [429] (429 Too Many Requests); request ID: req_test"
        );
    }

    #[test]
    fn status_errors_map_to_text_errors() {
        let cases = [
            (401, TextError::InvalidKey),
            (403, TextError::InvalidKey),
            (429, TextError::RateLimited),
            (500, TextError::Server(500)),
            (503, TextError::Server(503)),
            (400, TextError::Rejected(400)),
            (404, TextError::Rejected(404)),
            (413, TextError::Rejected(413)),
        ];
        for (status, expected) in cases {
            assert_eq!(
                status_error(StatusCode::from_u16(status).unwrap()),
                expected
            );
        }
    }

    #[tokio::test]
    async fn transport_errors_map_to_network_and_timeout() {
        let client = fixture_client();
        let err = client.get("http://127.0.0.1:1/").send().await.unwrap_err();
        assert_eq!(transport_error(&err), TextError::Network);
        assert_eq!(
            send_error(&anyhow::Error::new(err).context("wrapped")),
            TextError::Network
        );
        assert_eq!(
            send_error(&anyhow::anyhow!("OpenAI request timed out")),
            TextError::Timeout
        );
    }

    fn mock_response(status: u16, body: &str, retry_after: Option<&str>) -> reqwest::Response {
        // Status, header and redaction tests need no network transport. Tauri
        // reexports the same HTTP response type accepted by reqwest.
        let mut response = tauri::http::Response::builder()
            .status(status)
            .header("x-request-id", "req_test");
        if let Some(retry_after) = retry_after {
            response = response.header("retry-after", retry_after);
        }
        response.body(body.to_owned()).unwrap().into()
    }

    #[tokio::test]
    async fn retry_honors_retry_after_and_operation_deadline() {
        let response = mock_response(503, "", Some("10"));
        assert!(retry_delay(&Ok(response)).is_none());
        let response = mock_response(503, "", Some("1"));
        assert!(retry_delay(&Ok(response)).unwrap() >= Duration::from_secs(1));
        let response = mock_response(503, "", Some("Fri, 01 Oct 2027 00:00:00 GMT"));
        assert!(retry_delay(&Ok(response)).is_none());
        // A retry wait above the operation budget must return the first error.
        let client = fixture_client();
        let started = Instant::now();
        let response = send_with_retry(
            "OpenAI",
            || client.get("http://127.0.0.1:1/"),
            Duration::from_millis(10),
        )
        .await;
        assert!(response.is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[tokio::test]
    async fn response_body_read_obeys_the_operation_deadline() {
        use std::io::{Read, Write};
        let client = fixture_client();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (body_release, body_wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            // An initial exchange primes the transport before the measured
            // operation. Cold process/OS initialization is not a body timeout.
            let (mut warmup, _) = listener.accept().unwrap();
            warmup
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = [0; 1024];
            assert!(warmup.read(&mut request).unwrap() > 0);
            warmup
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            drop(warmup);

            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert!(stream.read(&mut request).unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();
            // Deliberately withhold the body until the caller has observed its
            // timeout. A channel makes this independent of scheduling sleeps.
            let _ = body_wait.recv_timeout(Duration::from_secs(10));
            let _ = stream.write_all(b"{}");
        });
        client
            .get(format!("http://{address}/warmup"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let response = send_with_retry(
            "OpenAI",
            || client.get(format!("http://{address}/deadline")),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let error = response.json::<serde_json::Value>().await.unwrap_err();
        assert!(error.is_timeout());
        assert_eq!(transport_error(&error), TextError::Timeout);
        assert!(matches!(
            diagnostic_code(&error.into()),
            DiagnosticCode::NetworkTimeout
        ));
        body_release.send(()).unwrap();
        server.join().unwrap();
    }
}
