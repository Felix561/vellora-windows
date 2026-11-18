import { useId, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2 } from "lucide-react";
import { formatDuration, formatTinyUsd, formatUsd } from "../lib/format";
import { effortLabel, kindLabel, providerLabel } from "../lib/textAssist";
import {
  billingLabel,
  formatMinutesSaved,
  formatRate,
  reasonLabel,
  USAGE_PERIODS,
  useUsageOverview,
} from "../lib/usageStats";
import type {
  AppHealthSnapshot,
  AppSettings,
  AppSnapshot,
  DiagnosticEvent,
  ReasonCount,
  RewritePreset,
  TextAssistInsights,
  TextAssistPresetUsage,
  TranscriptHistoryItem,
  UsageDay,
  UsageOverview,
  UsagePeriod,
  UsagePeriodKey,
} from "../lib/types";
import { TranscriptActions } from "./TranscriptActions";
import "./dashboard.css";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  keyStatusKnown?: boolean;
  snapshot: AppSnapshot;
  history: TranscriptHistoryItem[];
  health: AppHealthSnapshot | null;
  diagnostics: DiagnosticEvent[];
};

export function DashboardView({ settings, hasKey, keyStatusKnown = true, snapshot, history, health, diagnostics }: Props) {
  // A new transcript changes `history`; Text Assist runs refresh through their event.
  const { overview, error } = useUsageOverview(history);
  const [periodKey, setPeriodKey] = useState<UsagePeriodKey>("last7Days");

  const recent = history.slice(0, 5);

  const stateCopy = useMemo(() => {
    if (!keyStatusKnown) return "API key status unavailable";
    if (!hasKey) return "Connect your OpenAI key";
    if (snapshot.state === "idle") return "Ready for dictation";
    if (snapshot.state === "done") return snapshot.lastTranscript?.pasted ? "Transcript pasted" : "Transcript ready";
    if (snapshot.state === "error") return "Action needed";
    return `${snapshot.state[0].toUpperCase()}${snapshot.state.slice(1)}...`;
  }, [hasKey, keyStatusKnown, snapshot.state, snapshot.lastTranscript?.pasted]);

  const recovered = snapshot.lastTranscript;
  const period = overview?.periods[periodKey] ?? null;
  const periodInfo = USAGE_PERIODS.find((item) => item.key === periodKey) ?? USAGE_PERIODS[0];
  const totalTranscripts = overview?.periods.allTime.dictation.transcripts ?? history.length;

  return (
    <div className="dashboard-grid">
      {error && (
        <p className="error-banner dashboard-wide" role="alert">
          Could not load statistics: {error}. {overview ? "Statistics may be out of date." : ""}
        </p>
      )}
      {recovered && (!recovered.pasted || !settings.saveHistory || !history.some((item) => item.id === recovered.id)) && (
        <section className="panel dashboard-wide recovery-panel">
          <h3>Latest transcript</h3>
          <p className="hint">{recovered.error ? "Your transcript is available below even though the last operation failed." : !settings.saveHistory ? "History saving is off. This transcript is available until you dictate again or quit Vellora." : "Copy this transcript or paste it into the app you choose."}</p>
          {recovered.error && <p className="error-text">{recovered.error}</p>}
          <p className="transcript-preview">{recovered.finalText}</p>
          <TranscriptActions item={recovered} />
        </section>
      )}

      <section className="panel dashboard-wide usage-panel" aria-labelledby="usage-title">
        <div className="usage-header">
          <div>
            <p className="eyebrow">Dictation and Text Assist</p>
            <h2 id="usage-title">Your usage</h2>
          </div>
          <PeriodSwitch value={periodKey} onChange={setPeriodKey} />
        </div>
        {!overview && !error && <p className="hint" role="status">Loading statistics...</p>}
        {overview && period && (
          <UsageSummary overview={overview} period={period} periodDescription={periodInfo.description} settings={settings} />
        )}
      </section>

      {overview && <ActivityPanel days={overview.daily} />}

      {overview && period && (
        <TextAssistInsightsPanel
          insights={period.insights}
          runs={period.textAssist.runs}
          ensembleRuns={period.textAssist.ensembleRuns}
          periodDescription={periodInfo.description}
          presets={settings.textAssist.presets}
          chord={settings.textAssist.chord}
        />
      )}

      <section className="panel recent-panel">
        <div className="section-title">
          <div>
            <h3>Recent transcriptions</h3>
            <p>{totalTranscripts.toLocaleString()} total transcriptions</p>
          </div>
        </div>
        <div className="history-list compact-list">
          {recent.length === 0 && (
            <p className="empty">
              Your first page is still quiet. Hold {settings.hotkey.replace(/\+/g, " + ")} and speak when you're ready.
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

function PeriodSwitch({ value, onChange }: { value: UsagePeriodKey; onChange: (value: UsagePeriodKey) => void }) {
  return (
    <div className="segmented" role="group" aria-label="Statistics period">
      {USAGE_PERIODS.map((item) => (
        <button key={item.key} type="button" aria-pressed={item.key === value} onClick={() => onChange(item.key)}>
          {item.label}
        </button>
      ))}
    </div>
  );
}

function UsageSummary({ overview, period, periodDescription, settings }: {
  overview: UsageOverview;
  period: UsagePeriod;
  periodDescription: string;
  settings: AppSettings;
}) {
  const { dictation, textAssist } = period;
  const all = overview.periods.allTime;
  const nothingYet = all.dictation.transcripts === 0 && all.textAssist.runs === 0;
  const { dictationWordsPerMinute, secondsPerCorrectedWord } = overview.assumptions;
  return (
    <>
      {nothingYet && (
        <p className="usage-empty">
          Nothing recorded yet. Hold {settings.hotkey.replace(/\+/g, " + ")} to dictate, or select text in any app and press {settings.textAssist.chord.replace(/\+/g, " + ")} to fix it.
        </p>
      )}
      <div className="usage-columns">
        <UsageColumn
          title="Dictation"
          series="dictation"
          items={[
            ["Words dictated", dictation.words.toLocaleString()],
            ["Transcriptions", dictation.transcripts.toLocaleString()],
            ["Time saved", formatMinutesSaved(dictation.minutesSaved)],
            ["Estimated spend", formatUsd(dictation.estimatedCostUsd)],
          ]}
        />
        <UsageColumn
          title="Text Assist"
          series="assist"
          items={[
            ["Texts fixed (applied)", `${textAssist.applied.toLocaleString()} of ${textAssist.runs.toLocaleString()} ${textAssist.runs === 1 ? "run" : "runs"}`],
            ["Words corrected", textAssist.wordsCorrected.toLocaleString()],
            ["Time saved", formatMinutesSaved(textAssist.minutesSaved)],
            ["Estimated spend", formatUsd(textAssist.estimatedCostUsd)],
          ]}
          footnote={`${textAssist.wordsProcessed.toLocaleString()} words processed${textAssist.planRuns > 0 ? ` / ${textAssist.planRuns.toLocaleString()} plan ${textAssist.planRuns === 1 ? "run" : "runs"}` : ""}`}
        />
        <UsageColumn
          title="Together"
          series="total"
          items={[
            ["Time saved", formatMinutesSaved(dictation.minutesSaved + textAssist.minutesSaved)],
            ["Estimated spend", formatUsd(dictation.estimatedCostUsd + textAssist.estimatedCostUsd)],
            ["Uses", (dictation.transcripts + textAssist.runs).toLocaleString()],
          ]}
          footnote={`Totals ${periodDescription}`}
        />
      </div>
      <p className="hint usage-basis">
        Time saved is a conservative estimate: 1 minute per {dictationWordsPerMinute} dictated words, and {secondsPerCorrectedWord} seconds per corrected word in applied Text Assist runs.
      </p>
      <p className="cost-note">
        {overview.dictationCostNote}
        {overview.dictationEstimatedFromWordsCount > 0 && ` ${overview.dictationEstimatedFromWordsCount} older ${overview.dictationEstimatedFromWordsCount === 1 ? "note" : "notes"} estimated from word count.`}
        {overview.dictationUnknownCleanupModelCount > 0 && ` Cleanup cost excluded for ${overview.dictationUnknownCleanupModelCount} ${overview.dictationUnknownCleanupModelCount === 1 ? "note" : "notes"} with an unpriced model.`}
        {" "}Text Assist spend is estimated from provider list prices; runs billed to a ChatGPT or Claude plan add no API cost here. These are estimates, not billing data.
      </p>
    </>
  );
}

function UsageColumn({ title, series, items, footnote }: {
  title: string;
  series: "dictation" | "assist" | "total";
  items: [string, string][];
  footnote?: string;
}) {
  const id = useId();
  return (
    <section className={`usage-column series-${series}`} aria-labelledby={id}>
      <h3 id={id}>{series !== "total" && <span className="series-key" aria-hidden="true" />}{title}</h3>
      <dl>
        {items.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      {footnote && <p className="usage-footnote">{footnote}</p>}
    </section>
  );
}

/** Rounds a maximum up to 1, 2 or 5 times a power of ten for the axis. */
function niceMax(value: number) {
  if (value <= 4) return 4;
  const power = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 5, 10]) {
    if (step * power >= value) return step * power;
  }
  return 10 * power;
}

function ActivityPanel({ days }: { days: UsageDay[] }) {
  const [range, setRange] = useState<7 | 30>(7);
  const visible = days.slice(-range);
  const max = niceMax(Math.max(0, ...visible.map((day) => Math.max(day.transcripts, day.textAssistRuns))));
  const totals = visible.reduce((sum, day) => ({ transcripts: sum.transcripts + day.transcripts, runs: sum.runs + day.textAssistRuns }), { transcripts: 0, runs: 0 });
  const busiest = visible.reduce<UsageDay | null>((best, day) => {
    const uses = day.transcripts + day.textAssistRuns;
    return uses > 0 && (!best || uses > best.transcripts + best.textAssistRuns) ? day : best;
  }, null);
  const summary = totals.transcripts + totals.runs === 0
    ? `No activity in the last ${range} days.`
    : `Last ${range} days: ${totals.transcripts} transcriptions and ${totals.runs} Text Assist runs.${busiest ? ` Busiest day ${formatDay(busiest.date)}.` : ""}`;
  return (
    <section className="panel dashboard-wide activity-panel" aria-labelledby="activity-title">
      <div className="section-title">
        <div>
          <h3 id="activity-title">Activity</h3>
          <p>Transcriptions and Text Assist runs per day</p>
        </div>
        <div className="segmented" role="group" aria-label="Activity range">
          {([7, 30] as const).map((value) => (
            <button key={value} type="button" aria-pressed={range === value} onClick={() => setRange(value)}>{value} days</button>
          ))}
        </div>
      </div>
      <ul className="chart-legend" aria-label="Legend">
        <li><span className="series-key series-dictation" aria-hidden="true" />Dictation <b>{totals.transcripts.toLocaleString()}</b></li>
        <li><span className="series-key series-assist" aria-hidden="true" />Text Assist <b>{totals.runs.toLocaleString()}</b></li>
      </ul>
      <div className="activity-chart" role="img" aria-label={summary}>
        <div className="activity-axis" aria-hidden="true">
          {(max % 2 === 0 ? [max, max / 2, 0] : [max, 0]).map((tick) => <span key={tick}>{tick.toLocaleString()}</span>)}
        </div>
        <div className={`activity-plot range-${range}`} aria-hidden="true">
          {visible.map((day, index) => (
            <div className="activity-day" key={day.date}>
              <div className="activity-bars">
                <i className="series-dictation" style={{ height: `${(day.transcripts / max) * 100}%` }} />
                <i className="series-assist" style={{ height: `${(day.textAssistRuns / max) * 100}%` }} />
              </div>
              <small>{range === 7 ? day.dayLabel : (visible.length - 1 - index) % 7 === 0 ? shortDate(day.date) : ""}</small>
              <span className="activity-tip">
                {formatDay(day.date)}<br />
                {day.transcripts} dictation / {day.textAssistRuns} Text Assist
              </span>
            </div>
          ))}
        </div>
      </div>
      <table className="visually-hidden">
        <caption>Daily activity, last {range} days</caption>
        <thead>
          <tr><th scope="col">Day</th><th scope="col">Transcriptions</th><th scope="col">Words dictated</th><th scope="col">Text Assist runs</th><th scope="col">Texts fixed</th><th scope="col">Words corrected</th></tr>
        </thead>
        <tbody>
          {visible.map((day) => (
            <tr key={day.date}>
              <th scope="row">{formatDay(day.date)}</th>
              <td>{day.transcripts}</td><td>{day.dictatedWords}</td><td>{day.textAssistRuns}</td><td>{day.textAssistApplied}</td><td>{day.wordsCorrected}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

const outcomeSegments = [
  { key: "applied", label: "Applied" },
  { key: "noChange", label: "No change needed" },
  { key: "kept", label: "Kept in Vellora" },
  { key: "cancelled", label: "Cancelled" },
  { key: "failed", label: "Failed" },
] as const;

function TextAssistInsightsPanel({ insights, runs, ensembleRuns, periodDescription, presets, chord }: {
  insights: TextAssistInsights;
  runs: number;
  ensembleRuns: number;
  periodDescription: string;
  presets: RewritePreset[];
  chord: string;
}) {
  const { outcomes } = insights;
  const segments = outcomeSegments.map((segment) => ({ ...segment, count: outcomes[segment.key] }));
  const outcomeTotal = segments.reduce((sum, segment) => sum + segment.count, 0);
  return (
    <section className="panel dashboard-wide insights-panel" aria-labelledby="insights-title">
      <div className="section-title">
        <div>
          <h3 id="insights-title">Text Assist insights</h3>
          <p>{runs.toLocaleString()} {runs === 1 ? "run" : "runs"} {periodDescription}{ensembleRuns > 0 ? ` / ${ensembleRuns.toLocaleString()} ${ensembleRuns === 1 ? "ensemble" : "ensembles"}` : ""}</p>
        </div>
      </div>
      {runs === 0 ? (
        <p className="empty">No Text Assist runs {periodDescription}. Select text in any app and press {chord.replace(/\+/g, " + ")} to fix it.</p>
      ) : (
        <div className="insights-grid">
          <div className="insight-block">
            <h4>Outcomes</h4>
            <div className="insight-figures">
              <div className="usage-figure"><strong>{formatRate(insights.applyRate)}</strong><span>Apply rate ({outcomes.applied} of {insights.returnedResults} results)</span></div>
            </div>
            {outcomeTotal > 0 && (
              <div className="outcome-bar" role="img" aria-label={`Outcomes: ${segments.filter((segment) => segment.count > 0).map((segment) => `${segment.label} ${segment.count}`).join(", ")}`}>
                {segments.filter((segment) => segment.count > 0).map((segment) => (
                  <i key={segment.key} className={`outcome-${segment.key}`} style={{ flexGrow: segment.count }} />
                ))}
              </div>
            )}
            <ul className="outcome-legend">
              {segments.map((segment) => (
                <li key={segment.key}><span className={`outcome-key outcome-${segment.key}`} aria-hidden="true" />{segment.label}<b>{segment.count.toLocaleString()}</b></li>
              ))}
            </ul>
          </div>

          <div className="insight-block">
            <h4>Speed</h4>
            <dl className="insight-list">
              <div><dt>Median time to result</dt><dd>{formatOptionalMs(insights.duration.medianMs)}</dd></div>
              <div><dt>Average time to result</dt><dd>{formatOptionalMs(insights.duration.averageMs)}</dd></div>
              <div><dt>Median first token</dt><dd>{formatOptionalMs(insights.firstToken.medianMs)}</dd></div>
              <div><dt>Average first token</dt><dd>{formatOptionalMs(insights.firstToken.averageMs)}</dd></div>
            </dl>
          </div>

          <div className="insight-block insight-wide">
            <h4 id="insights-models">Models</h4>
            {insights.models.length === 0 ? <p className="hint">No model was called {periodDescription}.</p> : (
              <div className="table-scroll">
                <table className="insight-table" aria-labelledby="insights-models">
                  <thead>
                    <tr><th scope="col">Model</th><th scope="col">Billing</th><th scope="col">Runs</th><th scope="col">Applied</th><th scope="col">Avg time</th><th scope="col">Est. cost</th></tr>
                  </thead>
                  <tbody>
                    {insights.models.map((model) => (
                      <tr key={`${model.provider}/${model.model}/${model.effort ?? ""}/${model.billing}`}>
                        <th scope="row"><strong>{model.model}</strong><span>{providerLabel(model.provider)}{model.effort ? ` / effort: ${effortLabel(model.effort)}` : ""}</span></th>
                        <td>{billingLabel(model.billing)}</td>
                        <td>{model.runs.toLocaleString()}</td>
                        <td>{model.applied.toLocaleString()}</td>
                        <td>{formatOptionalMs(model.averageDurationMs)}</td>
                        <td>{model.billing === "api" ? formatTinyUsd(model.estimatedCostUsd) : "Plan / none"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>

          <div className="insight-block">
            <h4 id="insights-apps">Top apps</h4>
            <ul className="meter-list" aria-labelledby="insights-apps">
              {insights.apps.map((app) => (
                <MeterRow
                  key={app.targetApp ?? "unknown"}
                  label={app.targetApp ?? "Unknown app"}
                  value={app.runs}
                  max={insights.apps[0]?.runs ?? 1}
                  detail={`${app.applied} applied${app.failed ? `, ${app.failed} failed` : ""}`}
                />
              ))}
            </ul>
          </div>

          <div className="insight-block">
            <h4 id="insights-presets">Presets</h4>
            {insights.presets.length === 0 ? <p className="hint">No captured text {periodDescription}.</p> : (
              <ul className="meter-list" aria-labelledby="insights-presets">
                {insights.presets.map((preset) => (
                  <MeterRow
                    key={`${preset.kind}/${preset.presetId ?? ""}`}
                    label={presetLabel(preset, presets)}
                    value={preset.runs}
                    max={insights.presets[0]?.runs ?? 1}
                    detail={`${preset.applied} applied`}
                  />
                ))}
              </ul>
            )}
          </div>

          <ReasonList id="insights-failures" title="Why runs failed" reasons={insights.failureReasons} empty={`No failed runs ${periodDescription}.`} />
          <ReasonList id="insights-kept" title="Why results were kept, not pasted" reasons={insights.keptReasons} empty={`Every result was pasted or needed no change ${periodDescription}.`} />
        </div>
      )}
    </section>
  );
}

function MeterRow({ label, value, max, detail }: { label: string; value: number; max: number; detail: string }) {
  return (
    <li className="meter-row">
      <span className="meter-label">{label}</span>
      <b>{value.toLocaleString()} {value === 1 ? "run" : "runs"}</b>
      <span className="meter-track" aria-hidden="true"><i style={{ width: `${Math.max(2, (value / Math.max(1, max)) * 100)}%` }} /></span>
      <small>{detail}</small>
    </li>
  );
}

function ReasonList({ id, title, reasons, empty }: { id: string; title: string; reasons: ReasonCount[]; empty: string }) {
  return (
    <div className="insight-block">
      <h4 id={id}>{title}</h4>
      {reasons.length === 0 ? <p className="hint">{empty}</p> : (
        <ul className="reason-list" aria-labelledby={id}>
          {reasons.map((reason) => (
            <li key={reason.code}><span>{reasonLabel(reason.code)}</span><b>{reason.count.toLocaleString()}</b></li>
          ))}
        </ul>
      )}
    </div>
  );
}

function presetLabel(preset: TextAssistPresetUsage, presets: RewritePreset[]) {
  const name = presets.find((item) => item.id === preset.presetId)?.name;
  if (preset.kind === "polish") return kindLabel("polish");
  const presetName = name ?? (preset.presetId ? humanize(preset.presetId) : null);
  if (preset.kind === "ensemble") return presetName ? `Ensemble: ${presetName}` : "Ensemble";
  return presetName ?? kindLabel(preset.kind);
}

function humanize(value: string) {
  const words = value.replace(/[_-]+/g, " ").trim();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : value;
}

function formatOptionalMs(value: number | null) {
  return value === null ? "n/a" : formatDuration(value);
}

function formatMs(value?: number | null) {
  if (!value) return "n/a";
  if (value < 1000) return `${value}ms`;
  return `${(value / 1000).toFixed(1)}s`;
}

function parseDay(date: string) {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, (month ?? 1) - 1, day ?? 1);
}

function formatDay(date: string) {
  return parseDay(date).toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" });
}

function shortDate(date: string) {
  return parseDay(date).toLocaleDateString([], { month: "short", day: "numeric" });
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

function formatRelative(value: string) {
  const date = new Date(value);
  return date.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
