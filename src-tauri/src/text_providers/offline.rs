//! Developer-only offline engine (M2). Uppercases the input and streams it in small chunks,
//! so the whole Text Assist flow can be exercised without any network request or cost.

use std::time::{Duration, Instant};

use super::{
    CancelToken, DeltaSink, FinishReason, TextEngine, TextError, TextFuture, TextOutcome,
    TextRequest,
};

pub const OFFLINE_MODEL_ID: &str = "offline-uppercase";

/// Streams `input.to_uppercase()` in chunks of `chunk_chars` with `delay` between chunks.
#[derive(Debug, Clone)]
pub struct UppercaseEngine {
    pub chunk_chars: usize,
    pub delay: Duration,
}

impl Default for UppercaseEngine {
    fn default() -> Self {
        Self {
            chunk_chars: 12,
            delay: Duration::from_millis(40),
        }
    }
}

impl TextEngine for UppercaseEngine {
    fn stream<'a>(
        &'a self,
        request: TextRequest,
        cancel: CancelToken,
        on_delta: DeltaSink<'a>,
    ) -> TextFuture<'a> {
        Box::pin(async move {
            let started = Instant::now();
            let deadline = started + request.timeout;
            let output = request.input.to_uppercase();
            let chars: Vec<char> = output.chars().collect();
            let mut first_token_ms = None;
            for chunk in chars.chunks(self.chunk_chars.max(1)) {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(TextError::Cancelled),
                    _ = tokio::time::sleep(self.delay) => {}
                }
                if Instant::now() > deadline {
                    return Err(TextError::Timeout);
                }
                let piece: String = chunk.iter().collect();
                first_token_ms.get_or_insert(started.elapsed().as_millis() as u64);
                on_delta(&piece);
            }
            Ok(TextOutcome {
                text: output,
                finish: FinishReason::Completed,
                usage: None,
                first_token_ms,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::text_providers::{ModelRef, ProviderKind};

    fn request(input: &str) -> TextRequest {
        TextRequest {
            model: ModelRef::new(ProviderKind::Offline, OFFLINE_MODEL_ID, None),
            instructions: String::new(),
            input: input.to_string(),
            max_output_tokens: 100,
            timeout: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn streams_uppercase_in_order() {
        let engine = UppercaseEngine {
            chunk_chars: 3,
            delay: Duration::from_millis(1),
        };
        let received = Mutex::new(String::new());
        let sink = |piece: &str| received.lock().unwrap().push_str(piece);
        let outcome = engine
            .stream(request("Grüße, welt"), CancelToken::new(), &sink)
            .await
            .unwrap();
        assert_eq!(outcome.text, "GRÜSSE, WELT");
        assert_eq!(*received.lock().unwrap(), "GRÜSSE, WELT");
        assert_eq!(outcome.finish, FinishReason::Completed);
    }

    #[tokio::test]
    async fn cancellation_stops_the_stream() {
        let engine = UppercaseEngine {
            chunk_chars: 1,
            delay: Duration::from_millis(50),
        };
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = |_: &str| {};
        let result = engine.stream(request("abc"), cancel, &sink).await;
        assert_eq!(result, Err(TextError::Cancelled));
    }
}
