/** Browser-reachable Hub (REST + WS). */
export const HUB_URL =
  process.env.NEXT_PUBLIC_HUB_URL ?? "http://localhost:3000";

/** Browser-reachable Agent (HTTP chat fallback). */
export const AGENT_URL =
  process.env.NEXT_PUBLIC_AGENT_URL ?? "http://localhost:8000";

/** Hub WebSocket endpoint (`/api/v1/ws`, alias `/ws`). */
export function hubWsUrl(hubUrl: string = HUB_URL): string {
  const base = hubUrl.replace(/\/$/, "");
  const wsBase = base.replace(/^http/, "ws");
  return `${wsBase}/api/v1/ws`;
}
