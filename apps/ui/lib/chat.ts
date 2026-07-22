import { AGENT_URL } from "./config";
import { chatViaHubWs } from "./hubWs";
import { chatReplyText, type ChatResponse } from "./types";

export type ChatHistoryTurn = {
  role: "user" | "assistant";
  content: string;
};

export type SendChatOptions = {
  /** P5: re-send after Agent returned requires_confirmation. */
  confirm?: boolean;
  pending_action?: string;
  /** Prior turns for multi-turn LLM context. */
  conversation_history?: ChatHistoryTurn[];
};

export type SendChatResult = {
  reply: string;
  via: "ws" | "http";
  requires_confirmation?: boolean;
  pending_action?: string | null;
};

export type SendChatError = {
  error: string;
  via?: "ws" | "http";
};

function parseAgentJson(json: ChatResponse | string): SendChatResult {
  if (typeof json === "string") {
    return { reply: json, via: "http" };
  }
  const pending =
    typeof json.pending_action === "string" ? json.pending_action : null;
  return {
    reply: chatReplyText(json),
    via: "http",
    requires_confirmation: json.requires_confirmation === true,
    pending_action: pending,
  };
}

/**
 * Direct Agent `POST /v1/chat`.
 * Used as WS fallback when Hub WS is unavailable.
 */
async function chatViaAgentHttp(
  message: string,
  options?: SendChatOptions,
): Promise<SendChatResult> {
  const body: Record<string, unknown> = { message };
  if (options?.confirm) body.confirm = true;
  if (options?.pending_action) body.pending_action = options.pending_action;
  if (options?.conversation_history?.length) {
    body.context = { conversation_history: options.conversation_history };
  }

  const res = await fetch(`${AGENT_URL}/v1/chat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

  if (res.status === 404) {
    throw new Error(
      `Chat route missing — Agent has no POST /v1/chat at ${AGENT_URL}. Start or update the Agent service.`,
    );
  }

  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(
      `Agent returned ${res.status}${detail ? `: ${detail.slice(0, 200)}` : ""}`,
    );
  }

  const contentType = res.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    const json = (await res.json()) as ChatResponse | string;
    return parseAgentJson(json);
  }
  return { reply: await res.text(), via: "http" };
}

/**
 * Prefer Hub WebSocket `agent:message` → `agent:stream` (supports confirm + history).
 * Fall back to direct Agent `POST /v1/chat` if WS is unavailable.
 */
export async function sendChat(
  message: string,
  options?: SendChatOptions,
): Promise<SendChatResult> {
  const wsResult = await chatViaHubWs(message, {
    confirm: options?.confirm,
    pending_action: options?.pending_action,
    conversation_history: options?.conversation_history,
  });

  if (wsResult.ok) {
    return {
      reply: wsResult.reply,
      via: "ws",
      requires_confirmation: wsResult.requires_confirmation,
      pending_action: wsResult.pending_action,
    };
  }

  const shouldFallback =
    wsResult.code === "ws_error" ||
    wsResult.code === "ws_closed" ||
    wsResult.code === "timeout" ||
    wsResult.code === "agent_unreachable" ||
    wsResult.code === "agent_not_configured" ||
    wsResult.code === "agent_error";

  if (!shouldFallback) {
    throw new Error(wsResult.error);
  }

  try {
    return await chatViaAgentHttp(message, options);
  } catch (e) {
    const httpErr = e instanceof Error ? e.message : "HTTP chat failed";
    throw new Error(`${wsResult.error} — HTTP fallback: ${httpErr}`);
  }
}
