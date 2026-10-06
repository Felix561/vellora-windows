# Reference Projects

Vellora is implemented as a fresh Windows Tauri app. It was originally developed under the OpenFlow name. These MIT-licensed projects were cloned as implementation references:

- OpenWhispr: https://github.com/OpenWhispr/openwhispr
- FreeFlow: https://github.com/zachlatta/freeflow

MVP ideas borrowed at the product/design level:

- Hold-to-talk dictation flow
- Raw transcription followed by optional cleanup
- Text-only local history as a base for later dashboard/statistics UX
- Keep cleanup as a separate pipeline step rather than baking it into STT

No source files are vendored into the MVP app. If future work copies substantial code, update `THIRD_PARTY_NOTICES.md`.
