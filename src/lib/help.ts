import type { AppSettings, SupportInfo } from "./types";

export const SETUP_STORAGE_KEY = "vellora.setup.v1";

export function hasDismissedSetup(): boolean {
  try {
    const value = localStorage.getItem(SETUP_STORAGE_KEY);
    return value === "complete" || value === "skipped";
  } catch {
    return false;
  }
}

/** Return whether the nonsecret preference could be persisted. */
export function rememberSetup(choice: "complete" | "skipped"): boolean {
  try {
    localStorage.setItem(SETUP_STORAGE_KEY, choice);
    return true;
  } catch {
    return false;
  }
}

function enumValue(value: unknown, allowed: readonly string[]): string {
  return typeof value === "string" && allowed.includes(value) ? value : "unknown";
}

function flag(value: unknown): boolean | "unknown" {
  return typeof value === "boolean" ? value : "unknown";
}

/** Never spread API objects: unexpected properties and freeform values must not escape. */
export function formatSupportInfo(info: SupportInfo, settings: AppSettings): string {
  return JSON.stringify({
    app: "Vellora",
    version: typeof info.version === "string" && /^\d{1,4}\.\d{1,4}\.\d{1,4}$/.test(info.version) ? info.version : "unknown",
    platform: enumValue(info.platform, ["windows"]),
    architecture: enumValue(info.architecture, ["x86", "x86_64", "aarch64", "arm"]),
    startupPhase: enumValue(info.startupPhase, ["booting", "migrating", "initializingRuntime", "initializingTray", "initializingOverlay", "initializingHotkey", "ready", "degraded"]),
    ready: flag(info.ready),
    singleInstance: flag(info.singleInstance),
    keyConfigured: flag(info.keyConfigured),
    microphoneAvailable: flag(info.microphoneAvailable),
    dictationState: enumValue(info.dictationState, ["idle", "recording", "transcribing", "cleaning", "pasting", "done", "error"]),
    appearance: enumValue(settings.appearance, ["notebook", "classic"]),
    transcriptionModel: enumValue(settings.sttModel, ["whisper-1", "gpt-4o-mini-transcribe", "gpt-4o-transcribe", "gpt-4o-transcribe-diarize"]),
    cleanupEnabled: flag(settings.cleanupEnabled),
    autoPaste: flag(settings.autoPaste),
    copyToClipboard: flag(settings.copyToClipboard),
    saveHistory: flag(settings.saveHistory),
  }, null, 2);
}
