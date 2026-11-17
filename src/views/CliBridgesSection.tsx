import { useEffect, useId, useState } from "react";
import { RefreshCw } from "lucide-react";
import { MAX_CLI_PATH, useTextCliStatus, validateCliPath, type CliBridgeName } from "../lib/textAssist";
import type { CliBridge, CliStatus, TextAssistSettings } from "../lib/types";

type Feedback = { text: string; error: boolean };

type Props = {
  assist: TextAssistSettings;
  busy: boolean;
  /** Saves `patch` and reports `success` in this section's feedback; resolves false on failure. */
  save: (patch: Partial<TextAssistSettings>, success: string) => Promise<boolean>;
  feedback: Feedback | null;
};

const bridges: { name: CliBridgeName; title: string; tool: string; plan: string; signIn: string }[] = [
  { name: "codex", title: "ChatGPT plan via Codex CLI", tool: "Codex CLI", plan: "ChatGPT plan", signIn: "codex login" },
  { name: "claude", title: "Claude plan via Claude Code", tool: "Claude Code", plan: "Claude plan", signIn: "claude, then /login" },
];

/** Models -> Text models -> Subscription bridges: run plan models through the user's own signed-in CLIs. */
export function CliBridgesSection({ assist, busy, save, feedback }: Props) {
  const ids = useId();
  const { status, error, loading, refresh } = useTextCliStatus();

  async function update(bridge: (typeof bridges)[number], change: Partial<CliBridge>, success: string) {
    const next = { ...assist.cli, [bridge.name]: { ...assist.cli[bridge.name], ...change } };
    const ok = await save({ cli: next }, success);
    // A custom path changes which binary is detected, so check again.
    if (ok && "path" in change) void refresh();
    return ok;
  }

  return (
    <section className="panel" aria-labelledby={`${ids}-title`} aria-busy={loading}>
      <div className="ta-bridge-title">
        <h3 id={`${ids}-title`}>Subscription bridges (personal use)</h3>
        <button type="button" className="icon-button" disabled={loading} onClick={() => void refresh()}
          aria-label={loading ? "Checking command-line tools" : "Refresh command-line tool status"}>
          <RefreshCw size={15} aria-hidden="true" className={loading ? "ta-spin" : undefined} /> {loading ? "Checking..." : "Refresh"}
        </button>
      </div>
      <p className="hint ta-bridge-note">
        Vellora starts your own signed-in CLI in an empty temporary folder and sends only the selected text on standard input.
        It never reads or stores your login. Plan limits and the provider's terms apply; for personal use only.
      </p>
      <p className="hint">Runs take several seconds; not recommended for Quick Polish. Plan models cost $0 marginal and appear in the model pickers only while their bridge is on.</p>
      <p className="visually-hidden" role="status">{loading ? "Checking the command-line tools. This can take up to 20 seconds." : ""}</p>
      {error && <p className="error-text" role="alert">Could not check the command-line tools: {error}</p>}
      <FeedbackLine feedback={feedback} />
      <div className="ta-provider-list">
        {bridges.map((bridge) => (
          <BridgeRow key={bridge.name} bridge={bridge} setting={assist.cli[bridge.name]}
            status={status?.[bridge.name] ?? null} loading={loading} busy={busy}
            onToggle={(enabled) => void update(bridge, { enabled }, `${bridge.title} ${enabled ? "turned on" : "turned off"}.`)}
            onSavePath={(path) => update(bridge, { path },
              path ? `${bridge.tool} path saved.` : `${bridge.tool} path cleared; Vellora finds it on PATH.`)} />
        ))}
      </div>
    </section>
  );
}

function FeedbackLine({ feedback }: { feedback: Feedback | null }) {
  if (!feedback) return null;
  return <p className={feedback.error ? "error-text ta-feedback" : "success-text ta-feedback"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>;
}

function statusChip(status: CliStatus | null, loading: boolean) {
  if (!status) return { text: loading ? "Checking..." : "Unknown", ok: false };
  if (!status.installed) return { text: "Not installed", ok: false };
  if (status.signedIn === false) return { text: "Not signed in", ok: false };
  if (status.signedIn === true && status.planLogin === false) return { text: "API-key login", ok: false };
  if (status.signedIn === true) return { text: "Ready", ok: true };
  return { text: "Installed", ok: false };
}

function BridgeRow({ bridge, setting, status, loading, busy, onToggle, onSavePath }: {
  bridge: (typeof bridges)[number];
  setting: CliBridge;
  status: CliStatus | null;
  loading: boolean;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
  onSavePath: (path: string | null) => Promise<boolean>;
}) {
  const ids = useId();
  const [draft, setDraft] = useState(setting.path ?? "");
  const [pathError, setPathError] = useState<string | null>(null);

  // The saved path wins whenever it changes.
  useEffect(() => { setDraft(setting.path ?? ""); setPathError(null); }, [setting.path]);

  const chip = statusChip(status, loading);
  const changed = draft.trim() !== (setting.path ?? "");
  const notReady = status !== null && (!status.installed || status.signedIn === false);
  const apiKeyLogin = status?.installed === true && status.signedIn !== false && status.planLogin === false;

  async function submitPath() {
    const result = validateCliPath(draft);
    if (result.error !== null) {
      setPathError(result.error);
      document.getElementById(`${ids}-path`)?.focus();
      return;
    }
    setPathError(null);
    await onSavePath(result.path);
  }

  const details = !status
    ? [loading ? "Checking whether it is installed..." : "Status unknown. Press Refresh to check."]
    : [
      status.installed ? `Installed${status.version ? `, version ${status.version}` : ""}` : "Not installed",
      !status.installed ? null
        : status.signedIn === true ? `Signed in${status.planLogin === true ? ` with your ${bridge.plan}` : status.planLogin === false ? " with an API key" : ""}`
          : status.signedIn === false ? "Not signed in" : "Sign-in status unknown",
    ].filter((item): item is string => item !== null);

  return (
    <article className="ta-provider ta-bridge" aria-labelledby={`${ids}-name`}>
      <div className="ta-provider-head">
        <h4 id={`${ids}-name`}>{bridge.title}</h4>
        <span className={`ta-key-status ${chip.ok ? "saved" : ""}`}>{chip.text}</span>
      </div>
      <p className="hint">{details.join(" · ")}</p>
      <p className="hint ta-endpoint-meta">
        {status?.path ? <>Runs <span className="ta-mono">{status.path}</span></> : setting.path ? "The custom path below was not found." : status ? "Not found on PATH or in the usual install folders." : "Detected path appears after the check."}
      </p>
      {apiKeyLogin && (
        <p className="warning-text ta-key-note" role="note">
          {bridge.tool} is signed in with an API key, so runs are billed to that API account, not your {bridge.plan}. Sign in with your plan account to use the plan.
        </p>
      )}
      <label className="toggle-row">
        <input type="checkbox" checked={setting.enabled} disabled={busy}
          aria-describedby={setting.enabled && notReady ? `${ids}-warning` : undefined}
          onChange={(event) => onToggle(event.target.checked)} />
        <span>Use {bridge.plan} models</span>
      </label>
      {setting.enabled && notReady && (
        <p className="warning-text ta-key-note" id={`${ids}-warning`}>
          {status && !status.installed
            ? `${bridge.tool} is not installed${setting.path ? " at the custom path" : ""}. Runs fail until it is installed.`
            : `${bridge.tool} is not signed in. Run ${bridge.signIn} in a terminal; runs fail until you sign in.`}
        </p>
      )}
      <form className="ta-bridge-path" noValidate aria-label={`${bridge.tool} path`}
        onSubmit={(event) => { event.preventDefault(); void submitPath(); }}>
        <label className="field">
          <span>Custom path (optional)</span>
          <input id={`${ids}-path`} type="text" value={draft} disabled={busy} maxLength={MAX_CLI_PATH + 20}
            spellCheck={false} autoComplete="off" placeholder="Empty: find it on PATH"
            aria-invalid={pathError ? true : undefined}
            aria-describedby={pathError ? `${ids}-path-error` : `${ids}-path-hint`}
            onChange={(event) => { setDraft(event.target.value); setPathError(null); }} />
        </label>
        <button type="submit" className="icon-button" disabled={busy || !changed} aria-label={`Save ${bridge.tool} path`}>Save</button>
      </form>
      {pathError ? <p className="error-text ta-field-error" id={`${ids}-path-error`}>{pathError}</p>
        : <p className="hint ta-field-hint" id={`${ids}-path-hint`}>Full Windows path to the program, up to {MAX_CLI_PATH} characters. Leave empty to auto-detect.</p>}
    </article>
  );
}
