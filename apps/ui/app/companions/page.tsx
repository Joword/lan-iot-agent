"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { hubAuthHeaders } from "../../lib/auth";
import { HUB_URL } from "../../lib/config";
import type {
  Companion,
  CompanionCommandResult,
  CompanionListResponse,
  HubErrorBody,
} from "../../lib/types";

type CommandState = {
  companionId: string;
  loading: boolean;
  result: CompanionCommandResult | null;
  error: string | null;
  offline: boolean;
};

async function readHubError(res: Response): Promise<string> {
  const text = await res.text().catch(() => "");
  if (!text) return `Hub returned ${res.status}`;
  try {
    const body = JSON.parse(text) as HubErrorBody;
    const parts = [body.error, body.detail].filter(Boolean);
    if (parts.length) return parts.join(": ");
  } catch {
    /* not JSON */
  }
  return `Hub returned ${res.status}: ${text.slice(0, 200)}`;
}

export default function CompanionsPage() {
  const [companions, setCompanions] = useState<Companion[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [cmd, setCmd] = useState<CommandState | null>(null);

  const [formId, setFormId] = useState("");
  const [formName, setFormName] = useState("");
  const [formBaseUrl, setFormBaseUrl] = useState("http://127.0.0.1:9876");
  const [formKind, setFormKind] = useState("pc");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveOk, setSaveOk] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`${HUB_URL}/api/v1/companions`, {
        headers: hubAuthHeaders(),
      });
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      const json = (await res.json()) as CompanionListResponse;
      setCompanions(Array.isArray(json.companions) ? json.companions : []);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to fetch companions");
      setCompanions([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function registerCompanion(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    setSaveOk(null);
    try {
      const id = formId.trim();
      const name = formName.trim();
      const base_url = formBaseUrl.trim();
      if (!id || !name || !base_url) {
        throw new Error("id, name, and base_url are required");
      }
      const body: {
        id: string;
        name: string;
        base_url: string;
        kind?: string;
      } = { id, name, base_url };
      const kind = formKind.trim();
      if (kind) body.kind = kind;

      const res = await fetch(`${HUB_URL}/api/v1/companions`, {
        method: "POST",
        headers: hubAuthHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      setSaveOk(`Registered “${id}”`);
      await load();
    } catch (err) {
      setSaveError(
        err instanceof Error ? err.message : "Failed to register companion",
      );
    } finally {
      setSaving(false);
    }
  }

  async function sendCommand(
    id: string,
    command: string,
    extra?: Record<string, unknown>,
  ) {
    setCmd({
      companionId: id,
      loading: true,
      result: null,
      error: null,
      offline: false,
    });
    try {
      const res = await fetch(
        `${HUB_URL}/api/v1/companions/${encodeURIComponent(id)}/command`,
        {
          method: "POST",
          headers: hubAuthHeaders({ "Content-Type": "application/json" }),
          body: JSON.stringify({ command, ...(extra || {}) }),
        },
      );
      if (res.status === 503) {
        const msg = await readHubError(res);
        setCmd({
          companionId: id,
          loading: false,
          result: null,
          error: msg,
          offline: true,
        });
        return;
      }
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      const json = (await res.json()) as CompanionCommandResult;
      setCmd({
        companionId: id,
        loading: false,
        result: json,
        error: null,
        offline: false,
      });
    } catch (e) {
      const message =
        e instanceof Error ? e.message : "Failed to send companion command";
      const offlineHint =
        /failed to fetch|networkerror|load failed|connection refused/i.test(
          message,
        );
      setCmd({
        companionId: id,
        loading: false,
        result: null,
        error: message,
        offline: offlineHint,
      });
    }
  }

  return (
    <main className="mx-auto max-w-2xl px-6 py-12">
      <div className="mb-8 flex items-end justify-between gap-4">
        <div>
          <h1 className="mb-2 text-3xl font-semibold tracking-tight">
            Companions
          </h1>
          <p className="text-sm text-[var(--muted)]">
            Hub HTTP companions (phone / PC). Start{" "}
            <code className="text-xs">companions/windows/server.py</code> so
            this PC auto-registers; then Ping / Notify / Lock. Lock really
            locks the session and cannot be undone from this UI (no unlock).
          </p>
        </div>
        <button
          type="button"
          onClick={() => void load()}
          disabled={loading}
          className="shrink-0 rounded border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--muted)] hover:text-[var(--fg)] disabled:opacity-50"
        >
          Refresh
        </button>
      </div>

      <form
        onSubmit={(e) => void registerCompanion(e)}
        className="mb-10 space-y-3 rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-4"
      >
        <p className="text-sm font-medium">Register</p>
        <p className="text-xs text-[var(--muted)]">
          POST{" "}
          <span className="font-mono">{HUB_URL}/api/v1/companions</span>
          {" · "}Bearer from Pair if present
        </p>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label
              htmlFor="companion-id"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              id
            </label>
            <input
              id="companion-id"
              type="text"
              value={formId}
              onChange={(e) => setFormId(e.target.value)}
              placeholder="companion.demo_pc"
              disabled={saving}
              required
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
          <div>
            <label
              htmlFor="companion-name"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              name
            </label>
            <input
              id="companion-name"
              type="text"
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              placeholder="Demo PC"
              disabled={saving}
              required
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label
              htmlFor="companion-base-url"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              base_url
            </label>
            <input
              id="companion-base-url"
              type="url"
              value={formBaseUrl}
              onChange={(e) => setFormBaseUrl(e.target.value)}
              placeholder="http://127.0.0.1:9876"
              disabled={saving}
              required
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
          <div>
            <label
              htmlFor="companion-kind"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              kind{" "}
              <span className="text-[var(--muted)]">(optional, default pc)</span>
            </label>
            <input
              id="companion-kind"
              type="text"
              value={formKind}
              onChange={(e) => setFormKind(e.target.value)}
              placeholder="pc"
              disabled={saving}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="submit"
            disabled={saving}
            className="rounded border border-[var(--accent)] bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {saving ? "Registering…" : "Register companion"}
          </button>
          {saveOk ? (
            <span className="text-xs text-[var(--muted)]">{saveOk}</span>
          ) : null}
        </div>
        {saveError ? (
          <p className="text-sm text-[var(--danger)]">{saveError}</p>
        ) : null}
      </form>

      {loading && <p className="text-sm text-[var(--muted)]">Loading…</p>}

      {error && !loading && (
        <div className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-sm">
          <p className="text-[var(--danger)]">{error}</p>
          <p className="mt-1 text-[var(--muted)]">
            Hub may be down. Expected endpoint:{" "}
            <span className="font-mono text-xs">
              {HUB_URL}/api/v1/companions
            </span>
          </p>
        </div>
      )}

      {!loading && !error && (
        <>
          <p className="mb-4 text-xs text-[var(--muted)]">
            {companions.length} companion(s)
          </p>
          {companions.length === 0 ? (
            <p className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-6 text-sm text-[var(--muted)]">
              No companions registered. Start the Windows listener (it
              registers itself) or use the form above. Hub may also seed{" "}
              <code className="text-xs">companion.demo_pc</code>.
            </p>
          ) : (
            <ul className="divide-y divide-[var(--border)] border-t border-[var(--border)]">
              {companions.map((c) => (
                <li
                  key={c.id}
                  className="flex flex-wrap items-start justify-between gap-4 py-4"
                >
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">{c.name}</p>
                    <p className="font-mono text-xs text-[var(--muted)]">
                      {c.id}
                    </p>
                    <p className="mt-1 text-xs text-[var(--muted)]">
                      <span className="uppercase tracking-wide">{c.kind}</span>
                      {" · "}
                      <span className="font-mono">{c.base_url}</span>
                    </p>
                  </div>
                  <div className="flex shrink-0 flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => void sendCommand(c.id, "ping")}
                      disabled={cmd?.loading === true && cmd.companionId === c.id}
                      className="rounded border border-[var(--accent)] bg-[var(--accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
                    >
                      {cmd?.loading && cmd.companionId === c.id
                        ? "Sending…"
                        : "Ping"}
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        void sendCommand(c.id, "notify", {
                          title: "LanIoT",
                          body: "Hello from the Hub UI",
                        })
                      }
                      disabled={cmd?.loading === true && cmd.companionId === c.id}
                      className="rounded border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--fg)] hover:border-[var(--accent)] disabled:opacity-50"
                    >
                      Notify
                    </button>
                    <button
                      type="button"
                      onClick={() => void sendCommand(c.id, "lock")}
                      disabled={cmd?.loading === true && cmd.companionId === c.id}
                      className="rounded border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--fg)] hover:border-[var(--accent)] disabled:opacity-50"
                    >
                      Lock
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {cmd && (cmd.result || cmd.error) && (
        <div className="mt-8 rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3">
          <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
            <p className="text-sm font-medium">
              Command result ·{" "}
              <span className="font-mono text-xs text-[var(--muted)]">
                {cmd.companionId}
              </span>
            </p>
            {cmd.result?.ok ? (
              <span className="text-xs text-[var(--muted)]">ok</span>
            ) : null}
            {cmd.result && !cmd.result.ok && !cmd.offline ? (
              <span className="text-xs text-[var(--danger)]">rejected</span>
            ) : null}
            {cmd.offline ? (
              <span className="text-xs text-[var(--warn)]">offline / 503</span>
            ) : null}
          </div>
          {cmd.error ? (
            <>
              <p className="text-sm text-[var(--danger)]">{cmd.error}</p>
              {cmd.offline ? (
                <p className="mt-2 text-sm text-[var(--muted)]">
                  Companion listener may be down. Run{" "}
                  <code className="text-xs">companions/windows/server.py</code>{" "}
                  on port 9876, then retry.
                </p>
              ) : null}
            </>
          ) : (
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-[var(--fg)]">
              {JSON.stringify(cmd.result, null, 2)}
            </pre>
          )}
        </div>
      )}
    </main>
  );
}
