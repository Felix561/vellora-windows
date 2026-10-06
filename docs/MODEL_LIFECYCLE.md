# OpenAI model lifecycle and migration review

Checked against **official OpenAI documentation on 2026-10-04**. This is a migration plan, not an implemented or live-tested migration. No API requests, account inspection, key changes, or model/settings changes were made for this review.

## Scheduled shutdowns affecting Vellora

| Current exact model ID in Vellora | Purpose | OpenAI shutdown date | OpenAI's documented replacement |
| --- | --- | --- | --- |
| `gpt-5-nano-2025-08-07` | Default optional cleanup model; cleanup is off by default | **2026-12-11** | `gpt-5.6-luna` |
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

For cleanup, evaluate the exact documented candidate `gpt-5.6-luna` against the existing Responses request, output parser, token budget, cleanup prompt, and timeout. Preserve the original transcript on refusals, empty/incomplete output, billing/access errors, or service failure. The candidate is recommended by [OpenAI's deprecation table](https://developers.openai.com/api/docs/deprecations); replacement quality and latency have not been benchmarked here.

## Pricing implications

Standard published USD rates at this review date:

| Model | Published rate | Implication |
| --- | --- | --- |
| `gpt-4o-mini-transcribe` | Estimated **$0.003/minute** | Current lower-cost offered transcription option, but scheduled for shutdown |
| `gpt-4o-transcribe` | Estimated **$0.006/minute** | Current offered option, also scheduled for shutdown |
| `gpt-transcribe` | **$0.0045/minute** | Nominal rate is 50% above the current mini estimate and 25% below the current larger-model estimate |
| `gpt-live-transcribe` | **$0.017/minute** | A different live-audio workflow, not a cheaper substitute for file upload |

Audio rates are from [OpenAI pricing](https://developers.openai.com/api/docs/pricing). The percentages are arithmetic comparisons of those published rates, not a guarantee about a user's invoice or transcript quality.

The [GPT-5 nano page](https://developers.openai.com/api/docs/models/gpt-5-nano) lists **$0.05 input / $0.005 cached input / $0.40 output per million tokens**. The [GPT-5.6 Luna page](https://developers.openai.com/api/docs/models/gpt-5.6-luna) lists **$0.20 / $0.02 / $1.20**, respectively. Nominal uncached input is four times and output three times the older rates; actual token use can differ.

Vellora's [pricing code](../src-tauri/src/pricing.rs) uses approximate text tokens and currently applies GPT-5 nano rates even to an unknown cleanup ID. Changing the ID alone would understate the candidate's cost. Update the model rate mapping and unknown-model handling before offering the replacement; keep historical model IDs and historic estimates separate. Dashboard estimates are not an API billing ledger.

## Tasks before the deadlines

Suggested planning targets below leave time for a review build; they are not completed work or automatic publication dates.

1. **Cleanup migration reviewed by 2026-11-20**, before the December 11 shutdown. Verify candidate access with an explicitly authorized small, non-sensitive sample; compare punctuation, German/English, technical identifiers, preserved meaning, latency, and refusal/failure recovery. Confirm Responses parameters and cost accounting. Update defaults, UI/help, tests, pricing, and a deliberate saved-setting migration together.
2. **File transcription migration reviewed by 2027-02-01**, before the February 26 shutdown. Add the selected supported replacement across Rust/TypeScript model definitions, selectors, support-information allowlists, pricing, tests, and documentation. Define how existing saved IDs migrate without losing unrelated preferences or transcript history.
3. Use local fixtures first: multipart language hints, automatic language detection, new/empty/malformed JSON, unexpected content types, authentication/model-access errors, timeout/retry budget, and raw-text recovery. Live provider checks are separate and can incur normal billing.
4. Verify the intended OpenAI project has billing, the selected model, sufficient limits, and required permissions. Public documentation cannot prove access for a particular account. Both [GPT-Transcribe](https://developers.openai.com/api/docs/models/gpt-transcribe) and [GPT-5.6 Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna) list Free API tier as unsupported. Do not add a silent paid fallback or a paid setup check.
5. Ship and review the updated Windows build before the affected shutdown, document the minimum supported app version, and communicate that older binaries require an update. Recheck official deprecations/pricing at release time and periodically thereafter.

For the present publication decision, disclose this maintenance requirement in the README and roadmap. Source publication and installer approval remain separate decisions; neither is performed by this document. See the [publication review](PUBLICATION_REVIEW.md), [roadmap](ROADMAP.md), and [project comparison](PROJECT_COMPARISON.md).
