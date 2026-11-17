//! Builds and caches Text Assist engines (M5).
//!
//! One engine is kept per provider (and per OpenAI-compatible endpoint id). An engine is
//! rebuilt only when its key or endpoint URL changes, so pooled HTTP connections are reused
//! across jobs. Keys are compared through an in-memory fingerprint keyed with a per-process
//! random seed; the fingerprint is never stored or logged.

use std::{
    collections::HashMap,
    hash::{BuildHasher, Hash, Hasher},
    sync::{Arc, Mutex},
};

use super::{
    anthropic::AnthropicEngine, gemini::GeminiEngine, offline::UppercaseEngine,
    openai::OpenAiEngine, openai_compat::OpenAiCompatEngine, ModelRef, ProviderKind, TextEngine,
    TextError,
};
use crate::{
    credentials::{self, ProviderKey},
    types::CompatEndpoint,
};

/// Where keys come from. Production reads Windows Credential Manager; tests use a map.
pub trait KeySource: Send + Sync {
    /// The saved key, `Ok(None)` when none is saved. `endpoint` is the compatible endpoint id.
    /// Read failures map to [`TextError::MissingKey`] without any detail.
    fn key(
        &self,
        provider: ProviderKind,
        endpoint: Option<&str>,
    ) -> Result<Option<String>, TextError>;
}

/// Keys from Windows Credential Manager (`credentials.rs`).
#[derive(Debug, Default)]
pub struct CredentialKeys;

impl KeySource for CredentialKeys {
    fn key(
        &self,
        provider: ProviderKind,
        endpoint: Option<&str>,
    ) -> Result<Option<String>, TextError> {
        let result = match provider {
            ProviderKind::Openai => credentials::get_openai_key(),
            ProviderKind::Anthropic => credentials::get_provider_key(&ProviderKey::Anthropic),
            ProviderKind::Gemini => credentials::get_provider_key(&ProviderKey::Gemini),
            ProviderKind::OpenaiCompatible => {
                let id = endpoint.ok_or(TextError::Unsupported)?;
                let key = ProviderKey::compat(id).map_err(|_| TextError::Unsupported)?;
                credentials::get_provider_key(&key)
            }
            _ => Ok(None),
        };
        // The error text is dropped: callers only need "could not read".
        result.map_err(|_| TextError::MissingKey)
    }
}

#[derive(Clone)]
enum CachedEngine {
    OpenAi(Arc<OpenAiEngine>),
    Anthropic(Arc<AnthropicEngine>),
    Gemini(Arc<GeminiEngine>),
    Compat(Arc<OpenAiCompatEngine>),
}

impl CachedEngine {
    fn as_dyn(&self) -> Arc<dyn TextEngine> {
        match self {
            CachedEngine::OpenAi(engine) => engine.clone(),
            CachedEngine::Anthropic(engine) => engine.clone(),
            CachedEngine::Gemini(engine) => engine.clone(),
            CachedEngine::Compat(engine) => engine.clone(),
        }
    }

    async fn warm_up(&self) {
        match self {
            CachedEngine::OpenAi(engine) => engine.warm_up().await,
            CachedEngine::Anthropic(engine) => engine.warm_up().await,
            CachedEngine::Gemini(engine) => engine.warm_up().await,
            CachedEngine::Compat(engine) => engine.warm_up().await,
        }
    }
}

/// Cache slot: provider plus compatible endpoint id.
type Slot = (ProviderKind, Option<String>);

struct Entry {
    /// Fingerprint of the key and endpoint URL the engine was built with.
    fingerprint: u64,
    engine: CachedEngine,
}

/// Engine builder used by the factory. Tests replace it to point engines at loopback.
type Builder = dyn Fn(ProviderKind, Option<&str>, Option<String>) -> Result<CachedEngine, TextError>
    + Send
    + Sync;

pub struct EngineFactory {
    keys: Box<dyn KeySource>,
    build: Box<Builder>,
    seed: std::collections::hash_map::RandomState,
    cache: Mutex<HashMap<Slot, Entry>>,
}

impl std::fmt::Debug for EngineFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineFactory").finish_non_exhaustive()
    }
}

impl Default for EngineFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineFactory {
    /// Factory reading keys from Windows Credential Manager.
    pub fn new() -> Self {
        Self::with_keys(Box::new(CredentialKeys))
    }

    /// Factory with a custom key source (tests).
    pub fn with_keys(keys: Box<dyn KeySource>) -> Self {
        Self::with_parts(keys, Box::new(build_engine))
    }

    fn with_parts(keys: Box<dyn KeySource>, build: Box<Builder>) -> Self {
        Self {
            keys,
            build,
            seed: Default::default(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The engine for `model`, built on first use and reused while its key and endpoint stay
    /// the same. Reads the key on every call (Credential Manager reads are fast and local).
    ///
    /// Errors: `MissingKey` (no key saved, or it could not be read), `Unsupported` (CLI
    /// providers, unknown or invalid compatible endpoint), `Network` (HTTP client creation).
    pub fn engine_for(
        &self,
        model: &ModelRef,
        compat: &[CompatEndpoint],
    ) -> Result<Arc<dyn TextEngine>, TextError> {
        if model.provider == ProviderKind::Offline {
            return Ok(Arc::new(UppercaseEngine::default()));
        }
        Ok(self.cached(model, compat)?.as_dyn())
    }

    /// Pre-connects to the provider for `model` in the background so the next request skips
    /// DNS and the TLS handshake. Sends no key and no text. Does nothing for offline and CLI
    /// providers or without a saved key. Runs on the Tauri async runtime.
    pub fn warm_up(&self, model: &ModelRef, compat: &[CompatEndpoint]) {
        let Ok(engine) = self.cached(model, compat) else {
            return;
        };
        tauri::async_runtime::spawn(async move { engine.warm_up().await });
    }

    /// Like [`EngineFactory::warm_up`], but waits for the warm-up request to finish.
    pub async fn warm_up_now(&self, model: &ModelRef, compat: &[CompatEndpoint]) {
        if let Ok(engine) = self.cached(model, compat) {
            engine.warm_up().await;
        }
    }

    fn cached(
        &self,
        model: &ModelRef,
        compat: &[CompatEndpoint],
    ) -> Result<CachedEngine, TextError> {
        let (endpoint_id, base_url) = match model.provider {
            ProviderKind::Openai | ProviderKind::Anthropic | ProviderKind::Gemini => (None, None),
            ProviderKind::OpenaiCompatible => {
                let id = model.endpoint.as_deref().ok_or(TextError::Unsupported)?;
                let endpoint = compat
                    .iter()
                    .find(|endpoint| endpoint.id == id)
                    .ok_or(TextError::Unsupported)?;
                (Some(id), Some(endpoint.base_url.as_str()))
            }
            // Offline is handled by the caller; CLI bridges arrive in M6.
            ProviderKind::Offline | ProviderKind::CodexCli | ProviderKind::ClaudeCli => {
                return Err(TextError::Unsupported)
            }
        };
        let key = self
            .keys
            .key(model.provider, endpoint_id)?
            .filter(|key| !key.trim().is_empty());
        if key.is_none() && model.provider != ProviderKind::OpenaiCompatible {
            return Err(TextError::MissingKey);
        }
        let fingerprint = self.fingerprint(base_url, key.as_deref());
        let slot = (model.provider, endpoint_id.map(str::to_string));
        let mut cache = self.cache.lock().unwrap();
        if let Some(entry) = cache.get(&slot).filter(|e| e.fingerprint == fingerprint) {
            return Ok(entry.engine.clone());
        }
        let engine = (self.build)(model.provider, base_url, key)?;
        // Replaces the old engine for this slot, so a changed key never leaves a stale engine.
        cache.insert(
            slot,
            Entry {
                fingerprint,
                engine: engine.clone(),
            },
        );
        Ok(engine)
    }

    fn fingerprint(&self, base_url: Option<&str>, key: Option<&str>) -> u64 {
        let mut hasher = self.seed.build_hasher();
        base_url.hash(&mut hasher);
        key.hash(&mut hasher);
        hasher.finish()
    }

    #[cfg(test)]
    fn cached_count(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

/// Production engines with the default provider URLs.
fn build_engine(
    provider: ProviderKind,
    base_url: Option<&str>,
    key: Option<String>,
) -> Result<CachedEngine, TextError> {
    let required = || key.clone().ok_or(TextError::MissingKey);
    Ok(match provider {
        ProviderKind::Openai => CachedEngine::OpenAi(Arc::new(
            OpenAiEngine::new(required()?).map_err(|_| TextError::Network)?,
        )),
        ProviderKind::Anthropic => CachedEngine::Anthropic(Arc::new(
            AnthropicEngine::new(required()?).map_err(|_| TextError::Network)?,
        )),
        ProviderKind::Gemini => CachedEngine::Gemini(Arc::new(
            GeminiEngine::new(required()?).map_err(|_| TextError::Network)?,
        )),
        ProviderKind::OpenaiCompatible => CachedEngine::Compat(Arc::new(OpenAiCompatEngine::new(
            base_url.ok_or(TextError::Unsupported)?,
            key.clone(),
        )?)),
        _ => return Err(TextError::Unsupported),
    })
}

/// Loopback HTTP fixture shared by the provider tests (127.0.0.1 only, no real requests).
#[cfg(test)]
pub(crate) mod test_fixture {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{mpsc, Arc, Mutex},
        thread::JoinHandle,
        time::{Duration, Instant},
    };

    use serde_json::Value;

    use crate::text_providers::{CancelToken, TextEngine, TextError, TextOutcome, TextRequest};

    /// One scripted HTTP response.
    pub struct Reply {
        pub status: u16,
        pub pieces: Vec<Vec<u8>>,
        /// Keep the connection open after the pieces until the sender is dropped or 10 s pass.
        pub hold: Option<mpsc::Receiver<()>>,
    }

    impl Reply {
        pub fn stream(body: &str) -> Self {
            Self::pieces(vec![body.as_bytes().to_vec()])
        }

        pub fn pieces(pieces: Vec<Vec<u8>>) -> Self {
            Self {
                status: 200,
                pieces,
                hold: None,
            }
        }

        /// Error status whose body echoes private input (it must never be surfaced).
        pub fn status(status: u16) -> Self {
            Self::status_body(status, "{\"error\":{\"message\":\"private echoed input\"}}")
        }

        pub fn status_body(status: u16, body: &str) -> Self {
            Self {
                status,
                pieces: vec![body.as_bytes().to_vec()],
                hold: None,
            }
        }
    }

    pub struct Captured {
        pub method: String,
        /// Request line, e.g. `POST /v1/messages HTTP/1.1`.
        pub request_line: String,
        /// Lower-cased header block.
        pub headers: String,
        /// JSON body, `Value::Null` when empty.
        pub body: Value,
    }

    pub struct Fixture {
        pub base_url: String,
        requests: mpsc::Receiver<Captured>,
        server: JoinHandle<()>,
    }

    impl Fixture {
        pub fn finish(self) -> Vec<Captured> {
            self.server.join().unwrap();
            self.requests.try_iter().collect()
        }
    }

    /// Answers one connection per reply, in order. `base_url` ends in `/v1`.
    pub fn serve(replies: Vec<Reply>) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, requests) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let captured = read_request(&mut stream);
                let is_head = captured.method == "HEAD";
                sender.send(captured).unwrap();
                let reason = if reply.status == 200 { "OK" } else { "Error" };
                let content_type = if reply.status == 200 {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                let head = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n",
                    reply.status
                );
                if stream.write_all(head.as_bytes()).is_err() {
                    continue;
                }
                let _ = stream.flush();
                if is_head {
                    continue;
                }
                for piece in &reply.pieces {
                    if stream.write_all(piece).is_err() {
                        break;
                    }
                    let _ = stream.flush();
                    std::thread::sleep(Duration::from_millis(15));
                }
                if let Some(hold) = reply.hold {
                    let _ = hold.recv_timeout(Duration::from_secs(10));
                }
            }
        });
        Fixture {
            base_url: format!("http://{address}/v1"),
            requests,
            server,
        }
    }

    fn read_request(stream: &mut std::net::TcpStream) -> Captured {
        let mut data = Vec::new();
        let mut buf = [0; 4096];
        let header_end = loop {
            let read = stream.read(&mut buf).unwrap();
            assert!(read > 0, "client closed before sending a request");
            data.extend_from_slice(&buf[..read]);
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8(data[..header_end].to_vec()).unwrap();
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while data.len() < header_end + length {
            let read = stream.read(&mut buf).unwrap();
            assert!(read > 0);
            data.extend_from_slice(&buf[..read]);
        }
        let (request_line, headers) = head.split_once("\r\n").unwrap();
        let body = &data[header_end..header_end + length];
        Captured {
            method: request_line.split(' ').next().unwrap().to_string(),
            request_line: request_line.to_string(),
            headers: headers.to_lowercase(),
            body: if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(body).unwrap()
            },
        }
    }

    /// Runs `engine` and collects the streamed pieces.
    pub async fn run_engine(
        engine: &dyn TextEngine,
        request: TextRequest,
    ) -> (Result<TextOutcome, TextError>, Vec<String>) {
        let received: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = {
            let received = received.clone();
            move |piece: &str| received.lock().unwrap().push(piece.to_string())
        };
        let result = engine.stream(request, CancelToken::new(), &sink).await;
        let pieces = received.lock().unwrap().clone();
        (result, pieces)
    }

    pub fn assert_safe(error: &TextError, secrets: &[&str]) {
        let message = error.to_string();
        for secret in secrets.iter().chain(&["private echoed input"]) {
            assert!(!message.contains(secret), "{message}");
        }
    }

    /// Splits inside "ü" (2 bytes), inside "👋" (4 bytes), inside the first blank line and at
    /// byte 7.
    pub fn split_at_tricky_points(body: &str) -> Vec<Vec<u8>> {
        let bytes = body.as_bytes();
        let mut cuts = vec![
            body.find('ü').unwrap() + 1,
            body.find('👋').unwrap() + 2,
            body.find("\r\n\r\n")
                .map(|at| at + 2)
                .or_else(|| body.find("\n\n").map(|at| at + 1))
                .unwrap(),
            7,
        ];
        cuts.sort_unstable();
        cuts.dedup();
        let mut pieces = Vec::new();
        let mut last = 0;
        for cut in cuts {
            pieces.push(bytes[last..cut].to_vec());
            last = cut;
        }
        pieces.push(bytes[last..].to_vec());
        pieces
    }

    /// Cancels 50 ms after the first delta; the call must return within 500 ms of that.
    pub async fn assert_cancel_is_prompt(engine: &dyn TextEngine, request: TextRequest) {
        let cancel = CancelToken::new();
        let first_delta = Arc::new(tokio::sync::Notify::new());
        let cancelled_at = Arc::new(Mutex::new(None));
        let canceller = {
            let cancel = cancel.clone();
            let first_delta = first_delta.clone();
            let cancelled_at = cancelled_at.clone();
            tokio::spawn(async move {
                first_delta.notified().await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                *cancelled_at.lock().unwrap() = Some(Instant::now());
                cancel.cancel();
            })
        };
        let sink = {
            let first_delta = first_delta.clone();
            move |_: &str| first_delta.notify_one()
        };
        let result = engine.stream(request, cancel, &sink).await;
        let returned = Instant::now();
        assert_eq!(result, Err(TextError::Cancelled));
        canceller.await.unwrap();
        let cancelled_at = cancelled_at.lock().unwrap().unwrap();
        assert!(returned.duration_since(cancelled_at) < Duration::from_millis(500));
    }

    /// `request.timeout` must be about 1.5 s; the stalled stream must time out near it.
    pub async fn assert_times_out(engine: &dyn TextEngine, request: TextRequest) {
        let started = Instant::now();
        let (result, received) = run_engine(engine, request).await;
        assert_eq!(result, Err(TextError::Timeout));
        assert_eq!(received, ["Hel"]);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(1400), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::text_providers::{http::fixture_client, Effort};

    #[derive(Default)]
    struct MapKeys(Mutex<HashMap<(ProviderKind, Option<String>), String>>);

    impl MapKeys {
        fn set(&self, provider: ProviderKind, endpoint: Option<&str>, key: &str) {
            self.0
                .lock()
                .unwrap()
                .insert((provider, endpoint.map(str::to_string)), key.to_string());
        }
    }

    impl KeySource for Arc<MapKeys> {
        fn key(
            &self,
            provider: ProviderKind,
            endpoint: Option<&str>,
        ) -> Result<Option<String>, TextError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(provider, endpoint.map(str::to_string)))
                .cloned())
        }
    }

    /// Factory whose builder counts builds and points every engine at loopback.
    fn factory(keys: Arc<MapKeys>, builds: Arc<AtomicUsize>) -> EngineFactory {
        EngineFactory::with_parts(
            Box::new(keys),
            Box::new(move |provider, base_url, key| {
                builds.fetch_add(1, Ordering::SeqCst);
                let base = base_url.unwrap_or("http://127.0.0.1:1/v1").to_string();
                let key_or_empty = key.clone().unwrap_or_default();
                Ok(match provider {
                    ProviderKind::Openai => CachedEngine::OpenAi(Arc::new(
                        OpenAiEngine::with_client(key_or_empty, base, fixture_client()),
                    )),
                    ProviderKind::Anthropic => CachedEngine::Anthropic(Arc::new(
                        AnthropicEngine::with_client(key_or_empty, base, fixture_client()),
                    )),
                    ProviderKind::Gemini => CachedEngine::Gemini(Arc::new(
                        GeminiEngine::with_client(key_or_empty, base, fixture_client()),
                    )),
                    ProviderKind::OpenaiCompatible => CachedEngine::Compat(Arc::new(
                        OpenAiCompatEngine::with_client(&base, key, fixture_client())?,
                    )),
                    _ => return Err(TextError::Unsupported),
                })
            }),
        )
    }

    fn compat_model(endpoint: &str) -> ModelRef {
        let mut model = ModelRef::new(ProviderKind::OpenaiCompatible, "llama3", None);
        model.endpoint = Some(endpoint.to_string());
        model
    }

    fn endpoint(id: &str, base_url: &str) -> CompatEndpoint {
        CompatEndpoint {
            id: id.to_string(),
            name: id.to_string(),
            base_url: base_url.to_string(),
            models: vec!["llama3".to_string()],
        }
    }

    #[test]
    fn engines_are_cached_per_provider_and_rebuilt_on_key_change() {
        let keys = Arc::new(MapKeys::default());
        let builds = Arc::new(AtomicUsize::new(0));
        let factory = factory(keys.clone(), builds.clone());
        let haiku = ModelRef::new(ProviderKind::Anthropic, "claude-haiku-5-5", None);
        let sonnet = ModelRef::new(
            ProviderKind::Anthropic,
            "claude-sonnet-5-5",
            Some(Effort::Low),
        );
        let gemini = ModelRef::new(ProviderKind::Gemini, "gemini-3.1-flash-lite", None);

        assert_eq!(
            factory.engine_for(&haiku, &[]).err(),
            Some(TextError::MissingKey)
        );
        assert_eq!(builds.load(Ordering::SeqCst), 0);

        keys.set(ProviderKind::Anthropic, None, "sk-ant-1");
        keys.set(ProviderKind::Gemini, None, "gem-1");
        let first = factory.engine_for(&haiku, &[]).unwrap();
        // Another model of the same provider shares the engine (and its connection pool).
        let second = factory.engine_for(&sonnet, &[]).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let gem = factory.engine_for(&gemini, &[]).unwrap();
        assert!(!Arc::ptr_eq(&first, &gem));
        assert_eq!(builds.load(Ordering::SeqCst), 2);

        keys.set(ProviderKind::Anthropic, None, "sk-ant-2");
        let third = factory.engine_for(&haiku, &[]).unwrap();
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(builds.load(Ordering::SeqCst), 3);
        // The old engine was replaced, not kept beside the new one.
        assert_eq!(factory.cached_count(), 2);
    }

    #[test]
    fn compat_engines_are_cached_per_endpoint_and_url() {
        let keys = Arc::new(MapKeys::default());
        let builds = Arc::new(AtomicUsize::new(0));
        let factory = factory(keys.clone(), builds.clone());
        let local = [
            endpoint("ollama", "http://127.0.0.1:11434/v1"),
            endpoint("lmstudio", "http://localhost:1234/v1"),
        ];
        // No key is fine for compatible endpoints.
        let a = factory.engine_for(&compat_model("ollama"), &local).unwrap();
        let b = factory.engine_for(&compat_model("ollama"), &local).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = factory
            .engine_for(&compat_model("lmstudio"), &local)
            .unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(builds.load(Ordering::SeqCst), 2);

        keys.set(ProviderKind::OpenaiCompatible, Some("ollama"), "local-key");
        let d = factory.engine_for(&compat_model("ollama"), &local).unwrap();
        assert!(!Arc::ptr_eq(&a, &d));

        let moved = [endpoint("ollama", "http://127.0.0.1:11435/v1")];
        let e = factory.engine_for(&compat_model("ollama"), &moved).unwrap();
        assert!(!Arc::ptr_eq(&d, &e));
        assert_eq!(builds.load(Ordering::SeqCst), 4);

        assert_eq!(
            factory.engine_for(&compat_model("missing"), &local).err(),
            Some(TextError::Unsupported)
        );
        let mut no_endpoint = compat_model("ollama");
        no_endpoint.endpoint = None;
        assert_eq!(
            factory.engine_for(&no_endpoint, &local).err(),
            Some(TextError::Unsupported)
        );
        let insecure = [endpoint("remote", "http://example.com/v1")];
        assert_eq!(
            factory.engine_for(&compat_model("remote"), &insecure).err(),
            Some(TextError::Unsupported)
        );
    }

    #[test]
    fn offline_and_cli_providers() {
        let factory = factory(Arc::default(), Arc::default());
        let offline = ModelRef::new(
            ProviderKind::Offline,
            crate::text_providers::offline::OFFLINE_MODEL_ID,
            None,
        );
        assert!(factory.engine_for(&offline, &[]).is_ok());
        for provider in [ProviderKind::CodexCli, ProviderKind::ClaudeCli] {
            let model = ModelRef::new(provider, "default", None);
            assert_eq!(
                factory.engine_for(&model, &[]).err(),
                Some(TextError::Unsupported)
            );
        }
    }

    #[test]
    fn production_builder_builds_engines_without_network() {
        let keys = Arc::new(MapKeys::default());
        keys.set(ProviderKind::Anthropic, None, "sk-ant");
        keys.set(ProviderKind::Gemini, None, "gem");
        keys.set(ProviderKind::Openai, None, "sk-openai");
        let factory = EngineFactory::with_keys(Box::new(keys));
        for model in [
            ModelRef::new(ProviderKind::Anthropic, "claude-haiku-5-5", None),
            ModelRef::new(ProviderKind::Gemini, "gemini-3.1-flash-lite", None),
            ModelRef::new(ProviderKind::Openai, "gpt-6-luna", None),
        ] {
            assert!(factory.engine_for(&model, &[]).is_ok(), "{model:?}");
        }
        assert_eq!(factory.cached_count(), 3);
    }

    #[tokio::test]
    async fn warm_up_sends_no_key_and_no_text() {
        use test_fixture::{serve, Reply};
        let fixture = serve(vec![Reply::status(404)]);
        let keys = Arc::new(MapKeys::default());
        keys.set(
            ProviderKind::OpenaiCompatible,
            Some("local"),
            "local-secret",
        );
        let factory = factory(keys, Arc::default());
        let endpoints = [endpoint("local", &fixture.base_url)];
        factory
            .warm_up_now(&compat_model("local"), &endpoints)
            .await;
        let requests = fixture.finish();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "HEAD");
        assert_eq!(requests[0].request_line, "HEAD /v1/models HTTP/1.1");
        assert!(!requests[0].headers.contains("local-secret"));
        assert!(!requests[0].headers.contains("authorization"));
        assert_eq!(requests[0].body, serde_json::Value::Null);
    }

    #[test]
    fn warm_up_without_key_does_nothing() {
        let factory = factory(Arc::default(), Arc::default());
        // No key and no runtime: must return without panicking or spawning.
        factory.warm_up(
            &ModelRef::new(ProviderKind::Anthropic, "claude-haiku-5-5", None),
            &[],
        );
        assert_eq!(factory.cached_count(), 0);
    }
}
