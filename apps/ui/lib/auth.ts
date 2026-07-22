/** localStorage key for Hub pairing token (P5 stub). */
export const HUB_TOKEN_KEY = "laniot_hub_token";

export function getHubToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const t = localStorage.getItem(HUB_TOKEN_KEY);
    return t && t.trim() ? t.trim() : null;
  } catch {
    return null;
  }
}

export function setHubToken(token: string): void {
  localStorage.setItem(HUB_TOKEN_KEY, token);
}

export function clearHubToken(): void {
  localStorage.removeItem(HUB_TOKEN_KEY);
}

/** Headers for Hub REST; includes Bearer when a paired token exists. */
export function hubAuthHeaders(
  extra?: HeadersInit,
): Record<string, string> {
  const headers: Record<string, string> = {
    ...(extra as Record<string, string> | undefined),
  };
  const token = getHubToken();
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }
  return headers;
}
