"use client";

import { useEffect, useState, type FormEvent } from "react";
import {
  clearHubToken,
  getHubToken,
  setHubToken,
} from "../../lib/auth";
import { HUB_URL } from "../../lib/config";
import type { PairResponse } from "../../lib/types";

export default function PairPage() {
  const [code, setCode] = useState("");
  const [stored, setStored] = useState<string | null>(null);
  const [result, setResult] = useState<PairResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setStored(getHubToken());
  }, []);

  async function onPair(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const body: { code?: string } = {};
      const trimmed = code.trim();
      if (trimmed) body.code = trimmed;

      const res = await fetch(`${HUB_URL}/api/v1/auth/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        const detail = await res.text().catch(() => "");
        throw new Error(
          `Hub returned ${res.status}${detail ? `: ${detail.slice(0, 200)}` : ""}`,
        );
      }
      const json = (await res.json()) as PairResponse;
      setHubToken(json.token);
      setStored(json.token);
      setResult(json);
      setCode(json.code);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Pairing failed");
    } finally {
      setBusy(false);
    }
  }

  async function onLogout() {
    const token = getHubToken();
    if (token) {
      try {
        await fetch(`${HUB_URL}/api/v1/auth/logout`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${token}`,
          },
          body: JSON.stringify({ token }),
        });
      } catch {
        /* local clear still happens */
      }
    }
    clearHubToken();
    setStored(null);
    setResult(null);
  }

  function onClear() {
    void onLogout();
  }

  const masked =
    stored && stored.length > 12
      ? `${stored.slice(0, 8)}…${stored.slice(-4)}`
      : stored;

  return (
    <main className="mx-auto max-w-2xl px-6 py-12">
      <h1 className="mb-2 text-3xl font-semibold tracking-tight">Pair</h1>
      <p className="mb-8 text-sm text-[var(--muted)]">
        Optional Hub pairing stub. Token is stored in localStorage and sent as{" "}
        <code className="text-xs">Authorization: Bearer</code> when present
        (needed if Hub sets <code className="text-xs">AUTH_REQUIRED=true</code>).
      </p>

      <form onSubmit={onPair} className="mb-6 space-y-3">
        <label htmlFor="pair-code" className="block text-sm text-[var(--muted)]">
          Pairing code{" "}
          <span className="text-xs">(optional — omit to mint a new one)</span>
        </label>
        <input
          id="pair-code"
          type="text"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="A1B2C3"
          disabled={busy}
          className="w-full rounded border border-[var(--border)] bg-[var(--surface)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
        />
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="submit"
            disabled={busy}
            className="rounded border border-[var(--accent)] bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {busy ? "Pairing…" : "Pair"}
          </button>
          {stored ? (
            <button
              type="button"
              onClick={onClear}
              className="rounded border border-[var(--border)] px-3 py-2 text-sm text-[var(--muted)] hover:text-[var(--fg)]"
            >
              Clear token
            </button>
          ) : null}
        </div>
      </form>

      {error && (
        <div className="mb-4 rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-sm text-[var(--danger)]">
          {error}
        </div>
      )}

      <dl className="space-y-3 border-t border-[var(--border)] pt-6 text-sm">
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">Stored</dt>
          <dd className="font-mono text-xs sm:text-sm text-[var(--muted)]">
            {masked ?? "none"}
          </dd>
        </div>
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">Endpoint</dt>
          <dd className="font-mono text-xs sm:text-sm text-[var(--muted)]">
            POST {HUB_URL}/api/v1/auth/pair
          </dd>
        </div>
      </dl>

      {result && (
        <pre className="mt-6 max-h-60 overflow-auto whitespace-pre-wrap break-words rounded border border-[var(--border)] bg-[var(--surface)] p-4 font-mono text-xs leading-relaxed">
          {JSON.stringify(
            {
              code: result.code,
              token_type: result.token_type,
              expires_in: result.expires_in,
              token: `${result.token.slice(0, 8)}…`,
            },
            null,
            2,
          )}
        </pre>
      )}
    </main>
  );
}
