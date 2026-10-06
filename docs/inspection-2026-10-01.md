# Vellora inspection - 2026-10-01

Historical inspection of the local source checkout before the 0.2.16 update.
For the later publication audit and current findings, see [AUDIT.md](AUDIT.md).
Installed application: `%LOCALAPPDATA%/Vellora/vellora.exe`.
The installed binary was 0.2.14; source was 0.2.15 before this update.

## Changes in 0.2.16

- Overlay uses native click-through hit testing and cannot receive keyboard focus. This applies to idle, recording, processing, and error states.
- Settings exposes a Windows startup checkbox backed by the actual autostart registration. Launch no longer unconditionally enables startup.
- Settings updates report persistence failures and update displayed values after successful saves.
- Updated the misleading Whisper price hint and synchronized package versions.

## Suggested follow-up work

1. Cleanup response parsing (`src-tauri/src/openai.rs`) only examines the first output item and first content block. Reasoning models can put reasoning before the message, causing valid cleanup text to be missed. Parse all message/output_text blocks. Cleanup is currently disabled in the saved settings.
2. Microphone setup/capture failures (`src-tauri/src/audio.rs`) are printed to stderr in a background thread instead of being delivered to the GUI. Propagate them into the existing diagnostic and overlay state system.
3. Clipboard restoration (`src-tauri/src/clipboard.rs`) only preserves text. Images and other clipboard formats are lost when restoration is enabled; the fixed delay can also race slow paste consumers.
4. Cost estimates use successful history entries and exclude failed requests/retries. Treat them as estimates, not a replacement for OpenAI billing.

## Speech model comparison

Saved model: `whisper-1`; cleanup disabled.

| Model | USD/minute | USD/hour | Notes |
| --- | ---: | ---: | --- |
| whisper-1 | 0.006 | 0.36 | Current saved choice |
| gpt-4o-mini-transcribe | ~0.003 | ~0.18 | Cheapest listed OpenAI file transcription option; already supported in Vellora |
| gpt-transcribe | 0.0045 | 0.27 | New high-accuracy option with context/keyword/language hints; not yet supported in Vellora |
| gpt-4o-transcribe | ~0.006 | ~0.36 | Already supported |

Recommendation: try GPT-4o mini Transcribe in AI Models. It halves the estimated cost versus Whisper; OpenAI also reports better recognition than original Whisper. Actual quality for your microphone, accents, and technical words requires an audio comparison. The saved model was left unchanged.

Official sources checked October 1, 2026:
- https://developers.openai.com/api/docs/pricing
- https://developers.openai.com/api/docs/models/whisper-1
- https://developers.openai.com/api/docs/models/gpt-4o-mini-transcribe
- https://developers.openai.com/api/docs/models/gpt-transcribe

## Verification

Frontend TypeScript/Vite build passed. All 10 existing Rust tests passed.
Old build artifacts referenced the former Desktop/OpenFlow location; affected debug/release package artifacts were rebuilt.

The 0.2.16 Windows installer built and installed successfully. The installed GUI checkbox was checked against the actual startup setting, successfully disabled it, and the disabled state survived an application restart. The original enabled setting was restored. Native overlay inspection confirmed WS_EX_TRANSPARENT, WS_EX_LAYERED, and WS_EX_NOACTIVATE on the visible installed indicator. A local executable backup was retained. No paid transcription request was made during verification.
