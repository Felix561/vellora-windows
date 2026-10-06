import { useEffect, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, Clock3, Mic, ReceiptText, Sparkles } from "lucide-react";
import { getDashboardStats } from "../lib/api";
import { BRAND } from "../lib/brand";
import type {
  AppHealthSnapshot,
  AppSettings,
  AppSnapshot,
  DashboardStats,
  DiagnosticEvent,
  TranscriptHistoryItem,
} from "../lib/types";
import velloraMark from "../assets/brand/vellora-mark-paper.png";
import { TranscriptActions } from "./TranscriptActions";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  keyStatusKnown?: boolean;
  snapshot: AppSnapshot;
  history: TranscriptHistoryItem[];
  health: AppHealthSnapshot | null;
  diagnostics: DiagnosticEvent[];
};

const emptyStats: DashboardStats = {
  totalWords: 0,
  totalTranscripts: 0,
  transcriptsToday: 0,
  wordsToday: 0,
  estimatedMinutesSaved: 0,
  averageWordsPerTranscript: 0,
  mostActiveDay: null,
  week: [],
  modelUsage: [],
  cost: {
    pricingVersion: "openai-pricing-2026-05-29",
    estimatedTotalUsd: 0,
    estimatedTodayUsd: 0,
    estimatedWeekUsd: 0,
    estimatedTranscriptionUsd: 0,
    estimatedCleanupUsd: 0,
    billableAudioSeconds: 0,
    measuredAudioSeconds: 0,
    estimatedAudioSeconds: 0,
    estimatedFromWordsCount: 0,
    unknownModelCount: 0,
    week: [],
    modelBreakdown: [],
    note: "Estimated from OpenAI public pricing and local Vellora history. Not official billing data.",
  },
};

export function DashboardView({ settings, hasKey, keyStatusKnown = true, snapshot, history, health, diagnostics }: Props) {
  const [stats, setStats] = useState<DashboardStats>(emptyStats);
  const [statsError, setStatsError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getDashboardStats().then((value) => { if (active) { setStats(value); setStatsError(null); } }).catch((error) => { if (active) setStatsError(`Could not load statistics: ${String(error)}`); });
    return () => { active = false; };
  }, [history]);

  const recent = history.slice(0, 5);
  const weekTotal = stats.week.reduce((sum, day) => sum + day.transcriptCount, 0);
  const maxDay = Math.max(1, ...stats.week.map((day) => day.transcriptCount));

  const stateCopy = useMemo(() => {
    if (!keyStatusKnown) return "API key status unavailable";
    if (!hasKey) return "Connect your OpenAI key";
    if (snapshot.state === "idle") return "Ready for dictation";
    if (snapshot.state === "done") return snapshot.lastTranscript?.pasted ? "Transcript pasted" : "Transcript ready";
    if (snapshot.state === "error") return "Action needed";
    return `${snapshot.state[0].toUpperCase()}${snapshot.state.slice(1)}...`;
  }, [hasKey, keyStatusKnown, snapshot.state, snapshot.lastTranscript?.pasted]);

  const recovered = snapshot.lastTranscript;

  return (
    <div className="dashboard-grid">
      {statsError && <p className="error-banner dashboard-wide" role="alert">{statsError} Statistics may be out of date.</p>}
      {recovered && (!recovered.pasted || !settings.saveHistory || !history.some((item) => item.id === recovered.id)) && (
        <section className="panel dashboard-wide recovery-panel">
          <h3>Latest transcript</h3>
          <p className="hint">{recovered.error ? "Your transcript is available below even though the last operation failed." : !settings.saveHistory ? "History saving is off. This transcript is available until you dictate again or quit Vellora." : "Copy this transcript or paste it into the app you choose."}</p>
          {recovered.error && <p className="error-text">{recovered.error}</p>}
          <p className="transcript-preview">{recovered.finalText}</p>
          <TranscriptActions item={recovered} />
        </section>
      )}
      <section className="panel stat-hero">
        <div className="hero-heading">
          <img src={velloraMark} alt="" />
          <p className="eyebrow">Today's page</p>
          <h2>{BRAND.appName}</h2>
          <p className="muted">{BRAND.tagline}</p>
          <div className="hero-number">{stats.totalWords.toLocaleString()}</div>
          <p className="muted">words transcribed</p>
        </div>
        <div className="hero-divider" />
        <div className="metric-grid">
          <Metric icon={<Mic size={18} />} label="Transcriptions today" value={stats.transcriptsToday} />
          <Metric icon={<CheckCircle2 size={18} />} label="Total transcriptions" value={stats.totalTranscripts} />
          <Metric icon={<Clock3 size={18} />} label="Time saved" value={`${stats.estimatedMinutesSaved}m`} />
          <Metric icon={<Sparkles size={18} />} label="Avg words per note" value={stats.averageWordsPerTranscript} />
        </div>
      </section>

      <section className="panel weekly-panel">
        <div className="section-title">
          <div>
            <h3>This week</h3>
            <p>{weekTotal} transcriptions / Most active: {stats.mostActiveDay ?? "none"}</p>
          </div>
        </div>
        <div className="week-chart" role="img" aria-label={stats.week.map((day) => `${day.dayLabel}: ${day.transcriptCount} transcriptions`).join(", ") || "No weekly activity"}>
          {stats.week.map((day) => (
            <div className="day-bar" key={day.date}>
              <span>{day.transcriptCount || ""}</span>
              <div style={{ height: `${Math.max(8, (day.transcriptCount / maxDay) * 108)}px` }} />
              <small>{day.dayLabel}</small>
            </div>
          ))}
        </div>
      </section>

      <CostPanel stats={stats} />

      <section className="panel recent-panel">
        <div className="section-title">
          <div>
            <h3>Recent transcriptions</h3>
            <p>{stats.totalTranscripts} total transcriptions</p>
          </div>
        </div>
        <div className="history-list compact-list">
          {recent.length === 0 && (
            <p className="empty">
              Your first page is still quiet. Hold Ctrl + Win and speak when you're ready.
            </p>
          )}
          {recent.map((item) => (
            <article className="history-item" key={item.id}>
              <p>{item.finalText}</p>
              <footer className="history-meta">
                <div className="history-facts">
                  <span>{formatRelative(item.createdAt)}</span>
                  <span>{item.wordCount} words</span>
                  <span>{formatMs(item.transcriptionDurationMs ?? item.durationMs)}</span>
                  <span className="chip">{item.sttModel}</span>
                </div>
                <TranscriptActions item={item} />
                {item.error && <p className="error-text">{item.error} Your transcript can be copied above.</p>}
              </footer>
            </article>
          ))}
        </div>
      </section>

      <section className="panel pipeline-panel">
        <h3>Pipeline</h3>
        <div className="kv">
          <span>Status</span>
          <strong>{stateCopy}</strong>
          <span>Model</span>
          <strong>{settings.sttModel}</strong>
          <span>Cleanup</span>
          <strong>{settings.cleanupEnabled ? "Light" : "Off"}</strong>
          <span>Output</span>
          <strong>{settings.autoPaste ? settings.copyToClipboard ? "Paste + clipboard" : "Paste only" : settings.copyToClipboard ? "Clipboard only" : "Manual copy"}</strong>
        </div>
      </section>

      <SystemStatusPanel
        hasKey={hasKey}
        keyStatusKnown={keyStatusKnown}
        snapshot={snapshot}
        health={health}
        diagnostics={diagnostics}
        model={settings.sttModel}
      />
    </div>
  );
}

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: React.ReactNode }) {
  return (
    <div className="metric">
      {icon}
      <div>
        <strong>{value}</strong>
        <span>{label}</span>
      </div>
    </div>
  );
}

function formatMs(value?: number | null) {
  if (!value) return "n/a";
  if (value < 1000) return `${value}ms`;
  return `${(value / 1000).toFixed(1)}s`;
}

function CostPanel({ stats }: { stats: DashboardStats }) {
  const cost = stats.cost;
  const maxCost = Math.max(0.0001, ...cost.week.map((day) => day.estimatedCostUsd));
  return (
    <section className="panel cost-panel">
      <div className="section-title">
        <div>
          <h3>Estimated spend</h3>
          <p>OpenAI estimate / not official billing</p>
        </div>
        <ReceiptText size={20} />
      </div>
      <div className="cost-total">{formatUsd(cost.estimatedTotalUsd)}</div>
      <div className="cost-metrics">
        <Metric icon={<Clock3 size={18} />} label="Today" value={formatUsd(cost.estimatedTodayUsd)} />
        <Metric icon={<Sparkles size={18} />} label="This week" value={formatUsd(cost.estimatedWeekUsd)} />
        <Metric icon={<Mic size={18} />} label="Audio minutes" value={formatMinutes(cost.billableAudioSeconds)} />
        <Metric icon={<CheckCircle2 size={18} />} label="Cleanup" value={formatUsd(cost.estimatedCleanupUsd)} />
      </div>
      <div className="cost-bars" role="img" aria-label={`Estimated spending over the last seven days: ${cost.week.map((day) => `${day.dayLabel}: ${formatTinyUsd(day.estimatedCostUsd)}`).join(", ") || "No spending"}`}>
        {cost.week.map((day) => (
          <div className="cost-day" key={day.date}>
            <span>{day.estimatedCostUsd > 0 ? formatTinyUsd(day.estimatedCostUsd) : ""}</span>
            <div style={{ height: `${Math.max(8, (day.estimatedCostUsd / maxCost) * 92)}px` }} />
            <small>{day.dayLabel}</small>
          </div>
        ))}
      </div>
      <div className="cost-breakdown">
        {cost.modelBreakdown.slice(0, 4).map((model) => (
          <div className="cost-model-row" key={model.model}>
            <strong>{model.model}</strong>
            <span>{model.transcriptionCount} notes / {formatMinutes(model.audioSeconds)}</span>
            <b>{formatTinyUsd(model.estimatedCostUsd)}</b>
          </div>
        ))}
      </div>
      <p className="cost-note">
        {cost.note}
        {cost.estimatedFromWordsCount > 0 && ` ${cost.estimatedFromWordsCount} older notes estimated from word count.`}
      </p>
    </section>
  );
}

function SystemStatusPanel({
  hasKey,
  keyStatusKnown,
  snapshot,
  health,
  diagnostics,
  model,
}: {
  hasKey: boolean;
  keyStatusKnown: boolean;
  snapshot: AppSnapshot;
  health: AppHealthSnapshot | null;
  diagnostics: DiagnosticEvent[];
  model: string;
}) {
  const last = diagnostics[0] ?? health?.lastError ?? null;
  return (
    <section className="panel system-panel">
      <div className="section-title">
        <div>
          <h3>System status</h3>
          <p>{health?.ready ? "Backend ready" : "Backend starting"} / {snapshot.state}</p>
        </div>
        {last ? <AlertCircle size={20} /> : <CheckCircle2 size={20} />}
      </div>
      <div className="kv compact">
        <span>API key</span>
        <strong>{!keyStatusKnown ? "Unavailable" : hasKey ? "Saved" : "Missing"}</strong>
        <span>Model</span>
        <strong>{model}</strong>
        <span>Recording</span>
        <strong>{snapshot.recordingMode === "locked" ? "Locked" : snapshot.state === "recording" ? "Hold" : "Idle"}</strong>
        <span>Startup</span>
        <strong>{health?.startupPhase ?? "ready"}</strong>
      </div>
      {last && (
        <details className="diagnostic-details">
          <summary>{last.title}</summary>
          <p>{last.message}</p>
          {last.detail && <pre>{last.detail}</pre>}
        </details>
      )}
      {diagnostics.length > 1 && (
        <div className="diagnostic-list">
          {diagnostics.slice(1).map((item) => (
            <span key={item.id}>{item.title}</span>
          ))}
        </div>
      )}
    </section>
  );
}

function formatUsd(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "$0.00";
  if (value < 0.01) return "<$0.01";
  return `$${value.toFixed(2)}`;
}

function formatTinyUsd(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "$0";
  if (value < 0.01) return `$${value.toFixed(4)}`;
  return `$${value.toFixed(2)}`;
}

function formatMinutes(seconds: number) {
  if (!seconds) return "0m";
  return `${Math.max(1, Math.round(seconds / 60))}m`;
}

function formatRelative(value: string) {
  const date = new Date(value);
  return date.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
