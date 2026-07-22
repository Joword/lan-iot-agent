"use client";

import { useState, type FormEvent } from "react";
import { sendChat, type ChatHistoryTurn } from "../../lib/chat";
import { AGENT_URL, hubWsUrl } from "../../lib/config";

type Turn = {
  role: "user" | "assistant" | "error";
  text: string;
  via?: "ws" | "http";
  /** Index of the user turn that triggered this confirmation prompt. */
  confirmUserIndex?: number;
  pendingAction?: string;
  /** True while this turn is waiting for Confirm / Cancel. */
  awaitingConfirm?: boolean;
};

function historyFromTurns(turns: Turn[]): ChatHistoryTurn[] {
  return turns
    .filter((t) => t.role === "user" || t.role === "assistant")
    .map((t) => ({
      role: t.role as "user" | "assistant",
      content: t.text,
    }))
    .slice(-8);
}

export default function ChatPage() {
  const [message, setMessage] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [sending, setSending] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const text = message.trim();
    if (!text || sending) return;

    setMessage("");
    const history = historyFromTurns(turns);
    const userIndex = turns.length;
    setTurns((prev) => [...prev, { role: "user", text }]);
    setSending(true);

    try {
      const result = await sendChat(text, {
        conversation_history: history,
      });
      const needsConfirm = result.requires_confirmation === true;
      setTurns((prev) => [
        ...prev,
        {
          role: "assistant",
          text: result.reply,
          via: result.via,
          ...(needsConfirm
            ? {
                confirmUserIndex: userIndex,
                pendingAction: result.pending_action ?? "shutdown_all",
                awaitingConfirm: true,
              }
            : {}),
        },
      ]);
    } catch (err) {
      setTurns((prev) => [
        ...prev,
        {
          role: "error",
          text: err instanceof Error ? err.message : "Chat failed",
        },
      ]);
    } finally {
      setSending(false);
    }
  }

  async function onConfirm(turnIndex: number) {
    const turn = turns[turnIndex];
    if (!turn?.awaitingConfirm || turn.confirmUserIndex == null || sending) {
      return;
    }
    const userTurn = turns[turn.confirmUserIndex];
    if (!userTurn || userTurn.role !== "user") return;

    const pendingAction = turn.pendingAction ?? "shutdown_all";
    const history = historyFromTurns(turns.slice(0, turn.confirmUserIndex));
    setTurns((prev) =>
      prev.map((t, i) =>
        i === turnIndex ? { ...t, awaitingConfirm: false } : t,
      ),
    );
    setSending(true);

    try {
      const result = await sendChat(userTurn.text, {
        confirm: true,
        pending_action: pendingAction,
        conversation_history: history,
      });
      setTurns((prev) => [
        ...prev,
        { role: "assistant", text: result.reply, via: result.via },
      ]);
    } catch (err) {
      setTurns((prev) => [
        ...prev,
        {
          role: "error",
          text: err instanceof Error ? err.message : "Confirm failed",
        },
      ]);
    } finally {
      setSending(false);
    }
  }

  function onCancel(turnIndex: number) {
    setTurns((prev) =>
      prev
        .map((t, i) =>
          i === turnIndex ? { ...t, awaitingConfirm: false } : t,
        )
        .concat({
          role: "assistant",
          text: "Cancelled — dangerous action was not executed.",
        }),
    );
  }

  return (
    <main className="mx-auto flex max-w-2xl flex-col px-6 py-12">
      <h1 className="mb-2 text-3xl font-semibold tracking-tight">Chat</h1>
      <p className="mb-8 text-sm text-[var(--muted)]">
        Hub WebSocket <code className="text-xs">agent:message</code> (confirm +
        multi-turn history). Falls back to Agent HTTP if WS is down.
      </p>

      <div className="mb-6 min-h-[12rem] space-y-3 rounded border border-[var(--border)] bg-[var(--surface)] p-4">
        {turns.length === 0 ? (
          <p className="text-sm text-[var(--muted)]">
            Try “list my devices”, “打开小米灯”, “sleep mode”, or “关闭所有”
            (Confirm).
          </p>
        ) : (
          turns.map((t, i) => (
            <div
              key={`${i}-${t.role}`}
              className={
                t.role === "error" ? "text-sm text-[var(--danger)]" : "text-sm"
              }
            >
              <span className="mb-0.5 block text-xs uppercase tracking-wide text-[var(--muted)]">
                {t.role === "user"
                  ? "You"
                  : t.role === "error"
                    ? "Error"
                    : t.via === "http"
                      ? "Agent (HTTP)"
                      : t.via === "ws"
                        ? "Agent (WS)"
                        : "Agent"}
              </span>
              <p className="whitespace-pre-wrap text-[var(--fg)]">{t.text}</p>
              {t.awaitingConfirm ? (
                <div className="mt-2 flex flex-wrap gap-2">
                  <button
                    type="button"
                    disabled={sending}
                    onClick={() => onConfirm(i)}
                    className="rounded border border-[var(--danger)] bg-[var(--danger)] px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
                  >
                    Confirm
                  </button>
                  <button
                    type="button"
                    disabled={sending}
                    onClick={() => onCancel(i)}
                    className="rounded border border-[var(--border)] px-3 py-1.5 text-xs font-medium text-[var(--fg)] hover:bg-[var(--border)]/40 disabled:opacity-50"
                  >
                    Cancel
                  </button>
                </div>
              ) : null}
            </div>
          ))
        )}
      </div>

      <form onSubmit={onSubmit} className="space-y-3">
        <label htmlFor="chat-message" className="sr-only">
          Message
        </label>
        <textarea
          id="chat-message"
          rows={3}
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder="Ask the Agent…"
          disabled={sending}
          className="w-full resize-y rounded border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
        />
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="font-mono text-[10px] leading-relaxed text-[var(--muted)] sm:text-xs">
            WS {hubWsUrl()} · fallback POST {AGENT_URL}/v1/chat
          </p>
          <button
            type="submit"
            disabled={sending || !message.trim()}
            className="rounded border border-[var(--accent)] bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {sending ? "Sending…" : "Send"}
          </button>
        </div>
      </form>
    </main>
  );
}
