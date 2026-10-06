# Windows troubleshooting

Start with **Help & About** in Vellora. It includes **Open guided setup**, a local microphone availability check, version information, and **Copy support information**. Reopening setup does not reset your settings or delete your key/history. A saved key is not proof that an OpenAI request will succeed. New users without a saved key see the guide automatically; it can be skipped and reopened later.

The microphone check only asks whether Windows exposes a default input device and usable configuration. It does not capture sound, prove microphone permission, or contact OpenAI. A short dictation is a separate action and incurs normal API usage charges.

## Microphone unavailable, silent, or incorrect

1. Check that the microphone is connected, powered, and unmuted. If you use a headset, check its hardware mute control.
2. Open **Windows Settings > System > Sound > Input**. Select the intended default input and check its input level using Windows' microphone test.
3. Review microphone access for desktop apps: **Windows 11:** Settings > Privacy & security > Microphone; **Windows 10:** Settings > Privacy > Microphone. Guided setup also has a **Windows microphone settings** button.
4. Enable device microphone access and the setting that permits desktop apps to use the microphone. Company policies can prevent changing these settings; ask your administrator if a control is disabled.
5. After changing or reconnecting devices, quit Vellora from its tray menu, reopen it, and retry with a short dummy sentence.

Vellora currently uses the default Windows input; it does not have a device picker or live microphone meter. The setup availability check can succeed while the microphone is muted or permissions prevent recording. Microsoft's [microphone permission guide](https://support.microsoft.com/en-us/windows/privacy/turn-on-app-permissions-for-your-microphone-in-windows) explains the desktop-app setting.

## API key, authentication, quota, or network errors

| Symptom | What to check |
| --- | --- |
| API key missing | Save the key in Settings > General. Wait for the success message; storage failures are shown in the app. |
| Authentication or permission error | Check that your API key is active and belongs to the intended OpenAI project. Save a replacement through Settings if necessary. Never post the key in an issue. |
| Quota/billing error | Check the project's API billing, available credits, and usage/spend limits in the OpenAI Platform. Repeating the same recording does not fix exhausted quota. |
| Rate limit | Pause before retrying and check your project's rate limits. Repeated immediate attempts can make the problem worse. |
| Timeout, connection, or service error | Check internet access and whether your network permits HTTPS to `api.openai.com`. If your organization uses an HTTPS proxy, ask for its trusted certificate configuration. Keep TLS verification enabled. |

Provider error categories and suggested actions are documented in [OpenAI's API error guide](https://developers.openai.com/api/docs/guides/error-codes). A transcript is available for copying only after transcription succeeds; an unsuccessful upload cannot produce recoverable text. If optional cleanup fails after successful transcription, Vellora preserves the raw transcript.

Changing models does not repair an invalid key or exhausted billing quota. To isolate a cleanup problem, turn **Light cleanup after transcription** off and try a new short, non-sensitive dictation. Each real attempt uses your API account.

## Ctrl + Win does not start or stop recording

- Confirm Vellora is running: click its system tray icon to open the Dashboard and check its status. Closing the window hides it; **Quit** in the tray menu exits it.
- For normal dictation, hold Ctrl and Win together while speaking, then release both. For hands-free mode, double-tap the combination, releasing both keys between taps, and press it again to stop. Releasing and re-pressing only one modifier keeps normal hold mode.
- If the Dashboard reports a hotkey initialization problem, quit and reopen Vellora once. Closing the window alone does not restart the keyboard hook.
- Temporarily pause another keyboard-remapping/shortcut tool if it uses the same combination, then retry. The shortcut is currently fixed to Ctrl + Win.
- After sleep/resume or an input-device failure, check the Dashboard for an error before retrying. If the problem repeats, report a small reproduction using dummy speech.

The top status indicator is deliberately click-through. Use the main window or tray menu for controls.

## The text was transcribed but did not paste

1. Find the latest transcript on the Dashboard or in History. **Copy** it before starting another dictation if local history is disabled.
2. Confirm **Automatically paste into the focused app** is enabled in Settings > General if you want automatic delivery.
3. Start dictation while the destination's editable field is focused, and keep it focused until transcription finishes. If the top-level window changed, automatic paste is skipped to avoid delivering into a different window.
4. Use **Paste in 3s**, then switch to the intended field during the countdown. Use **Cancel paste** if needed. Alternatively, use Copy and paste yourself.
5. If the destination is running as administrator, try a normal unelevated destination such as Notepad. Windows can block simulated paste across privilege levels.

Vellora cannot detect browser-tab changes, movement to a different field, or caret movement within the same top-level window. A destination may ignore Ctrl+V even when input injection succeeds. Confirm the result before dictating again.

When clipboard copying is off, Vellora temporarily uses the clipboard for automatic paste and attempts to restore prior plain text. Preservation is best effort: rich formatting is not retained, slow consumers can race restoration, and a non-text-only clipboard causes automatic paste to be skipped. Use manual delivery when preserving clipboard formats matters. See [clipboard privacy and limitations](PRIVACY.md#clipboard-and-destination-apps).

## Windows startup does not behave as expected

Open **Settings > General > Windows startup** and use **Start Vellora when I sign in to Windows**. Wait for any save/status feedback. On sign-in, Vellora normally starts in the tray; the absence of an open window does not mean it failed to start.

If it does not start, review Vellora's entry in Windows' Startup apps settings, including restrictions from your organization. If an old OpenFlow copy also launches, disable that copy's startup separately; Vellora does not manage an older installation's startup entry. Check which version is running using Help & About.

Disabling startup does not close a running app. To stop the current session, use **Quit** in Vellora's tray menu. Do not manually edit the registry or delete your app data to resolve startup problems.

## Settings do not save or the app stays at startup

Wait for **Settings saved** before relying on a change. If saving fails, keep the existing settings and note the visible error category. A Windows security policy or filesystem restriction can prevent saving app data; use Help & About's support information and a bug report rather than repeatedly changing files by hand.

On the startup screen, use **Retry** if offered. If invalid settings were recovered, Vellora preserves a backup and displays a warning. Do not attach `%APPDATA%/Vellora` wholesale to an issue: it contains settings, backups, and unencrypted transcript history. Consult the [privacy document](PRIVACY.md) before reviewing local files.

## Installer, Windows protection, or missing WebView2

This public repository distributes source only; there is no approved public installer or executable download yet. If Windows or antivirus blocks a locally built copy, keep protection enabled and report the app version, source commit, and product's detection name. Do not share private paths or upload personal data with the report. Clean-account installation and public release checks remain open.

If the interface cannot open because WebView2 is unavailable, check the [official Microsoft WebView2 runtime page](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) or ask your Windows administrator. An installation or uninstall issue should include the Windows version and whether this was a fresh install or an upgrade.

## Send a useful bug report

Use the public repository's [**Bug report** form](https://github.com/Felix561/vellora-windows/issues/new/choose). Include the app version, Windows version, appearance, selected model, exact steps, expected behavior, actual behavior, and whether it repeats. Use a dummy sentence to demonstrate recording or text behavior.

**Help & About > Copy support information** generates a small allowlisted summary: app version, platform/architecture, startup/readiness state, single-instance status, dictation state, whether a key is configured, whether the local default-microphone check found a usable configuration, appearance/model choices, and cleanup/paste/clipboard/history flags. It excludes API keys, transcript/audio content, device names, personal paths, and free-form errors. Copying explicitly replaces your Windows clipboard with this summary and displays a preview. Review it before pasting it into a report.

Do not attach keys, database files, recordings, real transcripts, clipboard contents, or full app-data directories. Crop and inspect screenshots before sharing. Report vulnerabilities through [SECURITY.md](../SECURITY.md), not in a normal issue.
