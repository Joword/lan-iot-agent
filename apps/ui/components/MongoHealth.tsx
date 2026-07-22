"use client";

import { useEffect, useState } from "react";
import { HUB_URL } from "../lib/config";
import type { HubHealthResponse } from "../lib/types";

type MongoState =
  | { status: "loading" }
  | { status: "ok"; database?: string }
  | { status: "down"; database?: string }
  | { status: "error" };

export function MongoHealth() {
  const [state, setState] = useState<MongoState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch(`${HUB_URL}/api/v1/health`);
        if (!res.ok) {
          if (!cancelled) setState({ status: "error" });
          return;
        }
        const json = (await res.json()) as HubHealthResponse;
        if (cancelled) return;
        if (json.mongodb?.ok) {
          setState({
            status: "ok",
            database: json.mongodb.database,
          });
        } else {
          setState({
            status: "down",
            database: json.mongodb?.database,
          });
        }
      } catch {
        if (!cancelled) setState({ status: "error" });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const label =
    state.status === "loading"
      ? "…"
      : state.status === "ok"
        ? "ok"
        : state.status === "down"
          ? "down"
          : "n/a";

  const color =
    state.status === "ok"
      ? "text-[var(--muted)]"
      : state.status === "loading"
        ? "text-[var(--muted)]"
        : "text-[var(--warn)]";

  return (
    <div className="flex gap-3">
      <dt className="w-28 shrink-0 text-[var(--muted)]">MongoDB</dt>
      <dd className={`font-mono text-xs sm:text-sm ${color}`}>
        {label}
        {state.status === "ok" && state.database
          ? ` · ${state.database}`
          : null}
        {state.status === "down" && state.database
          ? ` · ${state.database}`
          : null}
      </dd>
    </div>
  );
}
