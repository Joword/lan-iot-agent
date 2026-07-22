import { HUB_URL, hubWsUrl } from "./config";
import { getHubToken } from "./auth";
import type { Device } from "./types";

export type HubWsStatus = "connecting" | "open" | "closed";

export type HubFrame = {
  type: string;
  [key: string]: unknown;
};

export type AgentStreamFrame = HubFrame & {
  type: "agent:stream";
  reply?: unknown;
  status?: unknown;
  errors?: unknown;
  requires_confirmation?: unknown;
  pending_action?: unknown;
};

export type ErrorFrame = HubFrame & {
  type: "error";
  code?: string;
  message?: unknown;
  detail?: unknown;
};

export type StateChangedFrame = HubFrame & {
  type: "device:state_changed";
  device?: Device;
};

function parseFrame(raw: string): HubFrame | null {
  try {
    const v = JSON.parse(raw) as unknown;
    if (v && typeof v === "object" && "type" in v && typeof (v as HubFrame).type === "string") {
      return v as HubFrame;
    }
  } catch {
    /* ignore */
  }
  return null;
}

function replyText(reply: unknown): string {
  if (typeof reply === "string") return reply;
  if (reply == null) return "";
  return String(reply);
}

/** Append `?token=` when a paired Hub token exists (AUTH_REQUIRED WS path). */
export function hubWsUrlWithAuth(hubUrl?: string): string {
  const base = hubWsUrl(hubUrl ?? HUB_URL);
  const token = getHubToken();
  if (!token) return base;
  const sep = base.includes("?") ? "&" : "?";
  return `${base}${sep}token=${encodeURIComponent(token)}`;
}

export type ChatViaWsResult =
  | {
      ok: true;
      reply: string;
      via: "ws";
      requires_confirmation?: boolean;
      pending_action?: string | null;
    }
  | { ok: false; error: string; code?: string; via: "ws" };

export type ChatViaWsOptions = {
  timeoutMs?: number;
  hubUrl?: string;
  confirm?: boolean;
  pending_action?: string;
  conversation_history?: Array<{ role: "user" | "assistant"; content: string }>;
};

/**
 * Open Hub WS, send `agent:message`, wait for `agent:stream` or agent-related `error`.
 */
export function chatViaHubWs(
  content: string,
  options?: ChatViaWsOptions,
): Promise<ChatViaWsResult> {
  const timeoutMs = options?.timeoutMs ?? 45_000;
  const url = hubWsUrlWithAuth(options?.hubUrl);

  return new Promise((resolve) => {
    let settled = false;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (e) {
      resolve({
        ok: false,
        error: e instanceof Error ? e.message : "WebSocket constructor failed",
        via: "ws",
      });
      return;
    }

    const finish = (result: ChatViaWsResult) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        ws.close();
      } catch {
        /* ignore */
      }
      resolve(result);
    };

    const timer = setTimeout(() => {
      finish({
        ok: false,
        error: `Timed out waiting for Agent reply over Hub WS (${timeoutMs}ms)`,
        code: "timeout",
        via: "ws",
      });
    }, timeoutMs);

    ws.onopen = () => {
      const frame: Record<string, unknown> = {
        type: "agent:message",
        content,
      };
      if (options?.confirm) frame.confirm = true;
      if (options?.pending_action) frame.pending_action = options.pending_action;
      if (options?.conversation_history?.length) {
        frame.context = { conversation_history: options.conversation_history };
      }
      ws.send(JSON.stringify(frame));
    };

    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      const frame = parseFrame(ev.data);
      if (!frame) return;

      if (frame.type === "agent:stream") {
        const stream = frame as AgentStreamFrame;
        const pending =
          typeof stream.pending_action === "string"
            ? stream.pending_action
            : null;
        finish({
          ok: true,
          reply: replyText(stream.reply) || "(empty reply)",
          via: "ws",
          requires_confirmation: stream.requires_confirmation === true,
          pending_action: pending,
        });
        return;
      }

      if (frame.type === "error") {
        const err = frame as ErrorFrame;
        const code = typeof err.code === "string" ? err.code : undefined;
        if (code === "event_lagged") return;
        const message =
          typeof err.message === "string"
            ? err.message
            : err.message != null
              ? String(err.message)
              : "Hub returned an error";
        finish({ ok: false, error: message, code, via: "ws" });
      }
    };

    ws.onerror = () => {
      finish({
        ok: false,
        error: `Hub WebSocket error (${url})`,
        code: "ws_error",
        via: "ws",
      });
    };

    ws.onclose = () => {
      finish({
        ok: false,
        error: `Hub WebSocket closed before reply (${url})`,
        code: "ws_closed",
        via: "ws",
      });
    };
  });
}

export type HubSocketHandlers = {
  onFrame?: (frame: HubFrame) => void;
  onStatus?: (status: HubWsStatus) => void;
};

/**
 * Persistent Hub WS with light reconnect. Used by Devices for `device:state_changed`.
 */
export function connectHubSocket(
  handlers: HubSocketHandlers,
  options?: { hubUrl?: string },
): { close: () => void; send?: (frame: Record<string, unknown>) => void } {
  const url = hubWsUrlWithAuth(options?.hubUrl);
  let closed = false;
  let ws: WebSocket | null = null;
  let retryMs = 1000;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  const setStatus = (s: HubWsStatus) => handlers.onStatus?.(s);

  const scheduleReconnect = () => {
    if (closed) return;
    if (retryTimer) clearTimeout(retryTimer);
    retryTimer = setTimeout(connect, retryMs);
    retryMs = Math.min(retryMs * 2, 15_000);
  };

  const connect = () => {
    if (closed) return;
    setStatus("connecting");
    try {
      ws = new WebSocket(url);
    } catch {
      setStatus("closed");
      scheduleReconnect();
      return;
    }

    ws.onopen = () => {
      retryMs = 1000;
      setStatus("open");
    };

    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      const frame = parseFrame(ev.data);
      if (frame) handlers.onFrame?.(frame);
    };

    ws.onerror = () => {
      /* onclose will follow */
    };

    ws.onclose = () => {
      setStatus("closed");
      ws = null;
      scheduleReconnect();
    };
  };

  connect();

  return {
    close: () => {
      closed = true;
      if (retryTimer) clearTimeout(retryTimer);
      try {
        ws?.close();
      } catch {
        /* ignore */
      }
      setStatus("closed");
    },
    send: (frame: Record<string, unknown>) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(frame));
      }
    },
  };
}
