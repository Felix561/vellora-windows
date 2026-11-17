# OpenAI model lifecycle and migration review

Checked against **official OpenAI documentation on 2026-10-04**; the cleanup rows were rechecked on **2026-11-17**. The transcription part is a migration plan, not an implemented or live-tested migration.

## Cleanup migration (implemented locally, November 17, 2026)

OpenAI's [deprecations page](https://developers.openai.com/api/docs/deprecations) now names **`gpt-6-luna`** as the replacement for `gpt-5-nano` and `gpt-5-nano-2025-08-07` (shutdown December 11, 2026). Earlier revisions of this document named `gpt-5.6-luna`, which was the listed replacement at the time.

- New default cleanup model: `gpt-6-luna`. Its [model page](https://developers.openai.com/api/docs/models/gpt-6-luna) lists no dated snapshot, Responses API support, reasoning efforts `none`/`low`/`medium`/`high`/`xhigh`/`max` (default `medium`; `minimal` is not listed) and **$0.10 input / $0.01 cached input / $0.50 output per million tokens**.
- Cleanup requests to `gpt-6-luna` and `gpt-5.6-luna` send `reasoning.effort: "none"`. Other model IDs keep their provider default, because accepted effort values differ per model.
- Saved settings that still name a retired cleanup ID are migrated to `gpt-6-luna` on load and written back. Other preferences, custom model IDs and transcript history are unchanged; history rows keep the model they used.
- Cost estimates use the rate of each row's model (`gpt-6-luna`, `gpt-5.6-luna`, `gpt-5-nano`). Rows with an unpriced cleanup model are counted and excluded instead of being priced as another model.
- The quality/latency comparison of `gpt-6-luna` and `gpt-5.6-luna` at efforts `none` and `low` is an ignored test (`live_cleanup_model_comparison` in [openai.rs](../src-tauri/src/openai.rs)) that makes paid requests only with `VELLORA_LIVE_PAID_CALLS=1` and the owner's consent. Results below.

### Cleanup comparison results (consented live run, November 17, 2026, 14:48 CET)

Six synthetic transcripts (English, German, mixed technical identifiers, an informal message, an injection attempt), 24 requests, total cost about **$0.0013**. Latency is wall time per request from this laptop, including network; the first request also paid connection setup.

| Model / effort | Median latency | Reasoning tokens | Cost per sample | Result quality |
| --- | --- | --- | --- | --- |
| `gpt-6-luna` / `none` | 1.7 s (first request 8.3 s) | 0 | ~$0.000024 | Correct punctuation and German commas; kept "Okay"; formatted code identifiers; injection text treated as data |
| `gpt-6-luna` / `low` | 1.9 s | 0-107 | ~$0.000051 | Same text as `none` |
| `gpt-5.6-luna` / `none` | 1.3 s | 0 | ~$0.000053 | Same quality; dropped the filler "Okay" |
| `gpt-5.6-luna` / `low` | 1.4 s | 0-129 | ~$0.000086 | Same text as `none` |

Decision: keep **`gpt-6-luna` with effort `none`**. It matched `low` on every sample at about half the cost and has the lowest price of the candidates. One sample added quotation marks around a quoted word ("banana"), a harmless style change.

## Text Assist models (feature branch, checked November 17, 2026)

The Text Assist catalog lives in [catalog.rs](../src-tauri/src/text_providers/catalog.rs) with the
check date per entry. No catalog model has a published shutdown date yet. Quick Polish defaults to
`gpt-5.6-luna` (effort `none`); see the benchmark in [TEXT_ASSIST_PLAN.md](TEXT_ASSIST_PLAN.md)
section 9.4. Anthropic (`claude-haiku-5-5`, `claude-sonnet-5-5`, `claude-opus-5-5`; header
`anthropic-version: 2026-10-21`) and Gemini (`gemini-3.1-flash-lite`, `gemini-3.6-flash`,
`gemini-3.8-flash`; Flash prices change on 2027-01-01) entries were verified against the provider
documentation but not yet with a live request. Recheck the deprecation pages before each release.

## Scheduled shutdowns affecting Vellora

| Current exact model ID in Vellora | Purpose | OpenAI shutdown date | OpenAI's documented replacement |
| --- | --- | --- | --- |
| `gpt-5-nano-2025-08-07` | Former default optional cleanup model (migrated, see above) | **2026-12-11** | `gpt-6-luna` |
| `whisper-1` | Transcription; application default | **2027-02-26** | `gpt-transcribe` or `gpt-live-transcribe` |
| `gpt-4o-mini-transcribe` | Transcription option | **2027-02-26** | `gpt-transcribe` or `gpt-live-transcribe` |
| `gpt-4o-transcribe` | Transcription option | **2027-02-26** | `gpt-transcribe` or `gpt-live-transcribe` |
| `gpt-4o-transcribe-diarize` | Transcription option; Vellora displays plain text | **2027-02-26** | `gpt-transcribe` or `gpt-live-transcribe` |

These dates and replacements are from OpenAI's [deprecations page](https://developers.openai.com/api/docs/deprecations). At shutdown, the listed model stops being accessible; this is a scheduled maintenance requirement, not evidence that the current implementation already fails. Vellora's defaults and offered IDs are defined in [types.rs](../src-tauri/src/types.rs). The review did not inspect the maintainer's personal saved settings.

The recommended replacement does not prove equal diarization capabilities, account access, quality, latency, or cost. Do not promise that old installed binaries continue working past these dates.

## Smallest migration to evaluate

Keep the current **record, stop, upload completed WAV, recover text, paste** workflow. OpenAI documents `gpt-transcribe` on `POST /v1/audio/transcriptions`, with a JSON result containing `text` and detected `languages`. It replaces singular `language` with an array of language hints; do not send both. See the [file transcription guide](https://developers.openai.com/api/docs/guides/speech-to-text) and [request/response reference](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create).

`gpt-live-transcribe` is the documented live-audio alternative. Realtime capture/session management would be additional work and is unnecessary merely to preserve Vellora's completed-file workflow. See its [model page](https://developers.openai.com/api/docs/models/gpt-live-transcribe). Streaming is outside this preparation.

The current [OpenAI client](../src-tauri/src/openai.rs) sends multipart WAV with `response_format=json`, a singular `language` when specified, and diarize-specific chunking. It parses a required string `text`. The guide's new JSON shape looks compatible with that text parser because extra fields are ignored, **an inference that still needs request/response tests and an authorized real API check**. Do not copy diarize parameters into the replacement request without confirming support.

For cleanup, see the migration section above. The existing Responses request, output parser, token budget, prompt and timeout are unchanged; the original transcript is still preserved on refusals, empty/incomplete output, billing/access errors, or service failure.

## Pricing implications

Standard published USD rates at this review date:

| Model | Published rate | Implication |
| --- | --- | --- |
| `gpt-4o-mini-transcribe` | Estimated **$0.003/minute** | Current lower-cost offered transcription option, but scheduled for shutdown |
| `gpt-4o-transcribe` | Estimated **$0.006/minute** | Current offered option, also scheduled for shutdown |
| `gpt-transcribe` | **$0.0045/minute** | Nominal rate is 50% above the current mini estimate and 25% below the current larger-model estimate |
| `gpt-live-transcribe` | **$0.017/minute** | A different live-audio workflow, not a cheaper substitute for file upload |

Audio rates are from [OpenAI pricing](https://developers.openai.com/api/docs/pricing). The percentages are arithmetic comparisons of those published rates, not a guarantee about a user's invoice or transcript quality.

The [GPT-5 nano page](https://developers.openai.com/api/docs/models/gpt-5-nano) lists **$0.05 input / $0.005 cached input / $0.40 output per million tokens**. The [GPT-6 Luna page](https://developers.openai.com/api/docs/models/gpt-6-luna) lists **$0.10 / $0.01 / $0.50**; the [GPT-5.6 Luna page](https://developers.openai.com/api/docs/models/gpt-5.6-luna) lists **$0.20 / $0.02 / $1.20**. Nominal `gpt-6-luna` input is twice and output 1.25 times the GPT-5 nano rate; actual token use can differ, and effort `none` avoids reasoning tokens.

Vellora's [pricing code](../src-tauri/src/pricing.rs) uses approximate text tokens, prices each row with its own model's rates, and counts rows with an unpriced cleanup model separately (pricing version `openai-pricing-2026-11-17`). Dashboard estimates are not an API billing ledger.

## Tasks before the deadlines

Suggested planning targets below leave time for a review build; they are not completed work or automatic publication dates.

1. **Cleanup migration reviewed by 2026-11-20**, before the December 11 shutdown. Code, tests, pricing and the saved-setting migration are done locally (November 17). The consented live comparison ran on November 17 (results above). Remaining: an installed-build check before the shutdown.
2. **File transcription migration reviewed by 2027-02-01**, before the February 26 shutdown. Add the selected supported replacement across Rust/TypeScript model definitions, selectors, support-information allowlists, pricing, tests, and documentation. Define how existing saved IDs migrate without losing unrelated preferences or transcript history.
3. Use local fixtures first: multipart language hints, automatic language detection, new/empty/malformed JSON, unexpected content types, authentication/model-access errors, timeout/retry budget, and raw-text recovery. Live provider checks are separate and can incur normal billing.
4. Verify the intended OpenAI project has billing, the selected model, sufficient limits, and required permissions. Public documentation cannot prove access for a particular account. Both [GPT-Transcribe](https://developers.openai.com/api/docs/models/gpt-transcribe) and [GPT-5.6 Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna) list Free API tier as unsupported. Do not add a silent paid fallback or a paid setup check.
5. Ship and review the updated Windows build before the affected shutdown, document the minimum supported app version, and communicate that older binaries require an update. Recheck official deprecations/pricing at release time and periodically thereafter.

For the present publication decision, disclose this maintenance requirement in the README and roadmap. Source publication and installer approval remain separate decisions; neither is performed by this document. See the [publication review](PUBLICATION_REVIEW.md), [roadmap](ROADMAP.md), and [project comparison](PROJECT_COMPARISON.md).
