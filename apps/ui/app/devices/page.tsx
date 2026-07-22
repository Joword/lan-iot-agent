"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { hubAuthHeaders } from "../../lib/auth";
import { HUB_URL, hubWsUrl } from "../../lib/config";
import {
  connectHubSocket,
  type HubWsStatus,
  type StateChangedFrame,
} from "../../lib/hubWs";
import {
  deviceDisplayName,
  type Device,
  type DeviceListResponse,
} from "../../lib/types";

function upsertDevice(list: Device[], device: Device): Device[] {
  const next = [...list];
  const i = next.findIndex((d) => d.entity_id === device.entity_id);
  if (i >= 0) next[i] = { ...next[i], ...device };
  else next.push(device);
  next.sort((a, b) => a.entity_id.localeCompare(b.entity_id));
  return next;
}

function isControllable(d: Device): boolean {
  const t = (d.entity_type || d.entity_id.split(".")[0] || "").toLowerCase();
  return ["light", "switch", "climate", "fan", "cover"].includes(t);
}

export default function DevicesPage() {
  const [data, setData] = useState<DeviceListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [wsStatus, setWsStatus] = useState<HubWsStatus>("connecting");
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const sendRef = useRef<
    ((frame: Record<string, unknown>) => void) | undefined
  >(undefined);

  const load = useCallback(async (opts?: { quiet?: boolean }) => {
    if (!opts?.quiet) {
      setLoading(true);
      setError(null);
    }
    try {
      const res = await fetch(`${HUB_URL}/api/v1/devices`, {
        headers: hubAuthHeaders(),
      });
      if (!res.ok) {
        throw new Error(`Hub returned ${res.status}`);
      }
      const json = (await res.json()) as DeviceListResponse;
      setData(json);
      setError(null);
    } catch (e) {
      if (!opts?.quiet) {
        setError(e instanceof Error ? e.message : "Failed to fetch devices");
        setData(null);
      }
    } finally {
      if (!opts?.quiet) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const { close, send } = connectHubSocket({
      onStatus: setWsStatus,
      onFrame: (frame) => {
        if (frame.type === "device:state_changed") {
          const device = (frame as StateChangedFrame).device;
          if (!device?.entity_id) {
            void load({ quiet: true });
            return;
          }
          setData((prev) =>
            prev
              ? { ...prev, devices: upsertDevice(prev.devices, device) }
              : prev,
          );
          return;
        }
        if (frame.type === "device:command_result" && frame.ok === false) {
          setActionError(String(frame.message ?? "command failed"));
        }
        if (frame.type === "error" && frame.code !== "event_lagged") {
          setActionError(String(frame.message ?? frame.code ?? "error"));
        }
      },
    });
    sendRef.current = send;
    return () => close();
  }, [load]);

  async function runAction(entityId: string, action: string) {
    setActionBusy(entityId);
    setActionError(null);
    try {
      // Prefer live WS command when connected; else REST.
      if (wsStatus === "open" && sendRef.current) {
        sendRef.current({
          type: "device:command",
          entity_id: entityId,
          action,
          params: {},
        });
        // Optimistic refresh shortly after.
        setTimeout(() => void load({ quiet: true }), 400);
        return;
      }
      const res = await fetch(
        `${HUB_URL}/api/v1/devices/${encodeURIComponent(entityId)}/actions`,
        {
          method: "POST",
          headers: hubAuthHeaders({ "Content-Type": "application/json" }),
          body: JSON.stringify({ action, params: {} }),
        },
      );
      if (!res.ok) {
        const detail = await res.text().catch(() => "");
        throw new Error(
          `Action failed ${res.status}${detail ? `: ${detail.slice(0, 160)}` : ""}`,
        );
      }
      await load({ quiet: true });
    } catch (e) {
      setActionError(e instanceof Error ? e.message : "Action failed");
    } finally {
      setActionBusy(null);
    }
  }

  const wsLabel =
    wsStatus === "open"
      ? "live"
      : wsStatus === "connecting"
        ? "connecting…"
        : "reconnect…";

  return (
    <main className="mx-auto max-w-2xl px-6 py-12">
      <div className="mb-8 flex items-end justify-between gap-4">
        <div>
          <h1 className="mb-2 text-3xl font-semibold tracking-tight">Devices</h1>
          <p className="text-sm text-[var(--muted)]">
            Hub registry + On/Off (WS <code className="text-xs">device:command</code>{" "}
            or REST). Live via{" "}
            <code className="text-xs">device:state_changed</code>.
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

      {loading && <p className="text-sm text-[var(--muted)]">Loading…</p>}

      {error && (
        <div className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-sm">
          <p className="text-[var(--danger)]">{error}</p>
          <p className="mt-1 text-[var(--muted)]">
            Hub may be down or auth required — pair at /pair. Endpoint:{" "}
            <span className="font-mono text-xs">
              {HUB_URL}/api/v1/devices
            </span>
          </p>
        </div>
      )}

      {actionError && (
        <p className="mb-4 rounded border border-[var(--border)] px-4 py-3 text-sm text-[var(--danger)]">
          {actionError}
        </p>
      )}

      {data?.warning && (
        <p className="mb-4 rounded border border-[var(--border)] px-4 py-3 text-sm text-[var(--warn)]">
          {data.warning}
        </p>
      )}

      {data && !loading && (
        <>
          <p className="mb-4 text-xs text-[var(--muted)]">
            HA available: {data.ha_available ? "yes" : "no"} ·{" "}
            {data.devices.length} device(s) · WS {wsLabel}
            <span className="mt-1 block font-mono text-[10px] opacity-80">
              {hubWsUrl()}
            </span>
          </p>
          {data.devices.length === 0 ? (
            <p className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-6 text-sm text-[var(--muted)]">
              No devices yet. Configure HA_TOKEN, publish faker MQTT topics, or
              start Hub without HA to seed in-memory faker brands.
            </p>
          ) : (
            <ul className="divide-y divide-[var(--border)] border-t border-[var(--border)]">
              {data.devices.map((d) => (
                <li
                  key={d.entity_id}
                  className="flex flex-wrap items-center justify-between gap-3 py-3"
                >
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">
                      {deviceDisplayName(d)}
                      {d.is_faker ? (
                        <span className="ml-2 align-middle text-[10px] uppercase tracking-wide text-[var(--warn)]">
                          faker
                        </span>
                      ) : null}
                    </p>
                    <p className="font-mono text-xs text-[var(--muted)]">
                      {d.entity_id}
                      {d.brand ? (
                        <span className="ml-2 text-[var(--muted)]">· {d.brand}</span>
                      ) : null}
                    </p>
                  </div>
                  <div className="flex items-center gap-2 text-sm">
                    <span
                      className={
                        d.available === false ? "text-[var(--muted)]" : undefined
                      }
                    >
                      {d.state}
                    </span>
                    {isControllable(d) ? (
                      <>
                        <button
                          type="button"
                          disabled={actionBusy === d.entity_id}
                          onClick={() => void runAction(d.entity_id, "turn_on")}
                          className="rounded border border-[var(--border)] px-2 py-1 text-xs hover:border-[var(--accent)] disabled:opacity-50"
                        >
                          On
                        </button>
                        <button
                          type="button"
                          disabled={actionBusy === d.entity_id}
                          onClick={() => void runAction(d.entity_id, "turn_off")}
                          className="rounded border border-[var(--border)] px-2 py-1 text-xs hover:border-[var(--accent)] disabled:opacity-50"
                        >
                          Off
                        </button>
                      </>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </main>
  );
}
