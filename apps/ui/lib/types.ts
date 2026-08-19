/** Loose Device shape matching Hub registry / HA entities. */
export type ParamSpec = {
  name: string;
  type?: string;
  required?: boolean;
  minimum?: number;
  maximum?: number;
  enum?: string[];
  description?: string;
};

export type ActionSpec = {
  name: string;
  description?: string;
  params?: ParamSpec[];
};

export type Capability = {
  kind: string;
  actions?: ActionSpec[];
};

export type Device = {
  entity_id: string;
  /** Display name from Hub (`friendly_name`) or HA (`name`). */
  friendly_name?: string;
  name?: string;
  state: string;
  entity_type?: string;
  available?: boolean;
  source?: string;
  /** Inferred brand (`xiaomi` | `gree` | `esp32` | …). */
  brand?: string | null;
  /** Stub / MQTT demo gear — swap with real HA entities later. */
  is_faker?: boolean;
  attributes?: Record<string, unknown>;
  capabilities?: Capability[];
};

export type DeviceListResponse = {
  devices: Device[];
  ha_available?: boolean;
  warning?: string;
};

export type ChatRequest = {
  message: string;
  /** P5 danger-confirm: re-POST after requires_confirmation. */
  confirm?: boolean;
  pending_action?: string;
};

export type ChatResponse = {
  reply?: string;
  message?: string;
  response?: string;
  content?: string;
  /** P5: Agent asks UI to confirm before executing a dangerous action. */
  requires_confirmation?: boolean;
  pending_action?: string;
};

export type SceneAction = {
  entity_id: string;
  action: string;
  params?: Record<string, unknown>;
};

export type Scene = {
  id: string;
  name: string;
  description?: string;
  actions?: SceneAction[];
};

export type SceneListResponse = {
  scenes: Scene[];
  count?: number;
};

export type SceneStepResult = {
  index: number;
  entity_id: string;
  action: string;
  ok: boolean;
  skipped: boolean;
  error?: string;
  result?: unknown;
};

export type SceneRunResult = {
  scene_id: string;
  ok: boolean;
  steps: SceneStepResult[];
  failed: number[];
  skipped_count: number;
};

export type PairResponse = {
  code: string;
  token: string;
  token_type: string;
  expires_in: number;
};

/** Hub Companion registry entry (phone / PC via Hub HTTP). */
export type Companion = {
  id: string;
  name: string;
  base_url: string;
  kind: string;
};

export type CompanionListResponse = {
  companions: Companion[];
  count: number;
};

export type CompanionCommandResult = {
  ok: boolean;
  device_id: string;
  command: string;
  response?: unknown;
  error?: string;
};

export type HubErrorBody = {
  error: string;
  detail?: string;
};

export type HubHealthResponse = {
  status: string;
  service?: string;
  mongodb?: {
    ok: boolean;
    database?: string;
  };
  devices_cached?: number;
};

export function deviceDisplayName(d: Device): string {
  return d.friendly_name || d.name || d.entity_id;
}

export function hasCapability(d: Device, kind: string): boolean {
  return (d.capabilities || []).some((c) => c.kind === kind);
}

export function actionSpec(d: Device, action: string): ActionSpec | undefined {
  for (const cap of d.capabilities || []) {
    const found = (cap.actions || []).find((a) => a.name === action);
    if (found) return found;
  }
  return undefined;
}

export function paramSpec(
  d: Device,
  action: string,
  param: string,
): ParamSpec | undefined {
  return actionSpec(d, action)?.params?.find((p) => p.name === param);
}

export function attrNumber(d: Device, keys: string[]): number | undefined {
  const attrs = d.attributes || {};
  for (const key of keys) {
    const v = attrs[key];
    if (typeof v === "number" && Number.isFinite(v)) return v;
    if (typeof v === "string" && v.trim() !== "") {
      const n = Number(v);
      if (Number.isFinite(n)) return n;
    }
  }
  return undefined;
}

export function chatReplyText(body: ChatResponse | string): string {
  if (typeof body === "string") return body;
  return body.reply ?? body.message ?? body.response ?? body.content ?? JSON.stringify(body);
}
