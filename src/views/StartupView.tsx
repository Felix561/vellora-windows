import { useEffect, useState } from "react";
import { BRAND } from "../lib/brand";
import { getAppHealth } from "../lib/api";
import type { AppHealthSnapshot } from "../lib/types";

type Props = {
  error?: string | null;
  onRetry: () => Promise<void>;
};

export function StartupView({ error, onRetry }: Props) {
  const [health, setHealth] = useState<AppHealthSnapshot | null>(null);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    let active = true;
    let pending = false;
    const timer = window.setInterval(async () => {
      setElapsed((value) => value + 1);
      if (pending) return;
      pending = true;
      try {
        const [nextHealth] = await Promise.allSettled([getAppHealth(), onRetry()]);
        if (active && nextHealth.status === "fulfilled") setHealth(nextHealth.value);
      } finally { pending = false; }
    }, 1000);
    return () => { active = false; window.clearInterval(timer); };
  }, [onRetry]);

  const stuck = elapsed >= 15;

  return (
    <div className="app-shell loading startup-screen">
      <section className="startup-card">
        <p className="eyebrow">{stuck ? "Still waking up" : "Opening Vellora"}</p>
        <h1>{BRAND.appName}</h1>
        <p className="muted" role="status">
          {stuck
            ? "Vellora could not finish starting yet."
            : "Checking the local app backend."}
        </p>
        <div className="startup-progress" aria-hidden="true" />
        <dl className="startup-facts">
          <div>
            <dt>Startup phase</dt>
            <dd>{formatPhase(health?.startupPhase)}</dd>
          </div>
          <div>
            <dt>Backend</dt>
            <dd>{health?.ready ? "Ready" : "Starting"}</dd>
          </div>
        </dl>
        {(error || health?.lastError) && (
          <details className="diagnostic-details">
            <summary>Technical details</summary>
            <p>{health?.lastError?.message ?? error}</p>
            {health?.lastError?.detail && <pre>{health.lastError.detail}</pre>}
            {error && <pre>{error}</pre>}
          </details>
        )}
        {stuck && (
          <p className="hint">Try quitting Vellora from the tray and starting it again from the desktop icon.</p>
        )}
      </section>
    </div>
  );
}

function formatPhase(phase?: string) {
  if (!phase) return "Starting";
  return phase.replace(/([A-Z])/g, " $1").replace(/^./, (char) => char.toUpperCase());
}
