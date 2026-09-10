"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { hubAuthHeaders } from "../../lib/auth";
import { HUB_URL } from "../../lib/config";
import type {
  HubErrorBody,
  Scene,
  SceneAction,
  SceneListResponse,
  SceneRunResult,
} from "../../lib/types";

type RunState = {
  sceneId: string;
  loading: boolean;
  result: SceneRunResult | null;
  error: string | null;
};

const DEFAULT_STEPS_JSON = `[
  { "entity_id": "light.faker_esp32_light", "action": "turn_off" }
]`;

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

function parseSteps(json: string, entityId: string, action: string): SceneAction[] {
  const trimmed = json.trim();
  if (trimmed) {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!Array.isArray(parsed)) {
      throw new Error("Steps must be a JSON array");
    }
    return parsed as SceneAction[];
  }
  const eid = entityId.trim();
  const act = action.trim();
  if (eid && act) {
    return [{ entity_id: eid, action: act }];
  }
  throw new Error("Provide steps JSON or entity_id + action");
}

export default function ScenesPage() {
  const [scenes, setScenes] = useState<Scene[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [loading, setLoading] = useState(true);
  const [run, setRun] = useState<RunState | null>(null);

  const [formId, setFormId] = useState("");
  const [formName, setFormName] = useState("");
  const [formEntityId, setFormEntityId] = useState("");
  const [formAction, setFormAction] = useState("turn_off");
  const [formStepsJson, setFormStepsJson] = useState(DEFAULT_STEPS_JSON);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveOk, setSaveOk] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    setNotFound(false);
    try {
      const res = await fetch(`${HUB_URL}/api/v1/scenes`, {
        headers: hubAuthHeaders(),
      });
      if (res.status === 404) {
        setNotFound(true);
        setScenes([]);
        setError(null);
        return;
      }
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      const json = (await res.json()) as SceneListResponse;
      setScenes(Array.isArray(json.scenes) ? json.scenes : []);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to fetch scenes");
      setScenes([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function upsertScene(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    setSaveOk(null);
    try {
      const id = formId.trim();
      const name = formName.trim();
      if (!id || !name) {
        throw new Error("id and name are required");
      }
      let steps: SceneAction[];
      try {
        steps = parseSteps(formStepsJson, formEntityId, formAction);
      } catch (err) {
        throw new Error(
          err instanceof Error ? err.message : "Invalid steps JSON",
        );
      }
      const res = await fetch(`${HUB_URL}/api/v1/scenes`, {
        method: "POST",
        headers: hubAuthHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ id, name, steps }),
      });
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      setSaveOk(`Saved “${id}”`);
      await load();
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : "Failed to save scene");
    } finally {
      setSaving(false);
    }
  }

  async function runScene(id: string) {
    setRun({ sceneId: id, loading: true, result: null, error: null });
    try {
      const res = await fetch(`${HUB_URL}/api/v1/scenes/${encodeURIComponent(id)}/run`, {
        method: "POST",
        headers: hubAuthHeaders({ "Content-Type": "application/json" }),
      });
      if (res.status === 404) {
        setRun({
          sceneId: id,
          loading: false,
          result: null,
          error: `Scene “${id}” not found (404).`,
        });
        return;
      }
      if (!res.ok) {
        throw new Error(await readHubError(res));
      }
      const json = (await res.json()) as SceneRunResult;
      setRun({ sceneId: id, loading: false, result: json, error: null });
    } catch (e) {
      setRun({
        sceneId: id,
        loading: false,
        result: null,
        error: e instanceof Error ? e.message : "Failed to run scene",
      });
    }
  }

  return (
    <main className="mx-auto max-w-2xl px-6 py-12">
      <div className="mb-8 flex items-end justify-between gap-4">
        <div>
          <h1 className="mb-2 text-3xl font-semibold tracking-tight">Scenes</h1>
          <p className="text-sm text-[var(--muted)]">
            Hub scenes run through AdapterRouter. Each step shows ok / fail / skip.
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
        onSubmit={(e) => void upsertScene(e)}
        className="mb-10 space-y-3 rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-4"
      >
        <p className="text-sm font-medium">Create / upsert</p>
        <p className="text-xs text-[var(--muted)]">
          POST{" "}
          <span className="font-mono">{HUB_URL}/api/v1/scenes</span>
          {" · "}Bearer from Pair if present
        </p>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label htmlFor="scene-id" className="mb-1 block text-xs text-[var(--muted)]">
              id
            </label>
            <input
              id="scene-id"
              type="text"
              value={formId}
              onChange={(e) => setFormId(e.target.value)}
              placeholder="evening_lights"
              disabled={saving}
              required
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
          <div>
            <label htmlFor="scene-name" className="mb-1 block text-xs text-[var(--muted)]">
              name
            </label>
            <input
              id="scene-name"
              type="text"
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              placeholder="Evening lights"
              disabled={saving}
              required
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label
              htmlFor="scene-entity"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              entity_id{" "}
              <span className="text-[var(--muted)]">(if steps JSON empty)</span>
            </label>
            <input
              id="scene-entity"
              type="text"
              value={formEntityId}
              onChange={(e) => setFormEntityId(e.target.value)}
              placeholder="light.faker_esp32_light"
              disabled={saving}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
          <div>
            <label
              htmlFor="scene-action"
              className="mb-1 block text-xs text-[var(--muted)]"
            >
              action
            </label>
            <input
              id="scene-action"
              type="text"
              value={formAction}
              onChange={(e) => setFormAction(e.target.value)}
              placeholder="turn_off"
              disabled={saving}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-sm text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
            />
          </div>
        </div>
        <div>
          <label htmlFor="scene-steps" className="mb-1 block text-xs text-[var(--muted)]">
            steps JSON
          </label>
          <textarea
            id="scene-steps"
            value={formStepsJson}
            onChange={(e) => setFormStepsJson(e.target.value)}
            rows={4}
            disabled={saving}
            spellCheck={false}
            className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-2 font-mono text-xs leading-relaxed text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
          />
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="submit"
            disabled={saving}
            className="rounded border border-[var(--accent)] bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save scene"}
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

      {notFound && !loading && (
        <div className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-sm">
          <p className="text-[var(--muted)]">
            Scenes API is not available yet (404). Start or update Hub with the
            scenes routes.
          </p>
          <p className="mt-1 font-mono text-xs text-[var(--muted)]">
            {HUB_URL}/api/v1/scenes
          </p>
        </div>
      )}

      {error && !loading && (
        <div className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-sm">
          <p className="text-[var(--danger)]">{error}</p>
          <p className="mt-1 text-[var(--muted)]">
            Hub may be down. Expected endpoint:{" "}
            <span className="font-mono text-xs">{HUB_URL}/api/v1/scenes</span>
          </p>
        </div>
      )}

      {!loading && !error && !notFound && (
        <>
          <p className="mb-4 text-xs text-[var(--muted)]">
            {scenes.length} scene(s)
          </p>
          {scenes.length === 0 ? (
            <p className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-6 text-sm text-[var(--muted)]">
              No scenes yet. Use the form above, or Hub may seed demos (
              <code className="text-xs">sleep_mode</code>,{" "}
              <code className="text-xs">away_mode</code>).
            </p>
          ) : (
            <ul className="divide-y divide-[var(--border)] border-t border-[var(--border)]">
              {scenes.map((s) => (
                <li
                  key={s.id}
                  className="flex flex-wrap items-start justify-between gap-4 py-4"
                >
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">{s.name}</p>
                    <p className="font-mono text-xs text-[var(--muted)]">{s.id}</p>
                    {s.description ? (
                      <p className="mt-1 text-sm text-[var(--muted)]">
                        {s.description}
                      </p>
                    ) : null}
                    {s.actions && s.actions.length > 0 ? (
                      <p className="mt-1 text-xs text-[var(--muted)]">
                        {s.actions.length} action
                        {s.actions.length === 1 ? "" : "s"}
                      </p>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    onClick={() => void runScene(s.id)}
                    disabled={run?.loading === true && run.sceneId === s.id}
                    className="shrink-0 rounded border border-[var(--accent)] bg-[var(--accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
                  >
                    {run?.loading && run.sceneId === s.id ? "Running…" : "Run"}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {run && (run.result || run.error) && (
        <div className="mt-8 rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-3">
          <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
            <p className="text-sm font-medium">
              Run result ·{" "}
              <span className="font-mono text-xs text-[var(--muted)]">
                {run.sceneId}
              </span>
            </p>
            {run.result ? (
              <span
                className={
                  run.result.ok
                    ? "text-xs text-[var(--muted)]"
                    : "text-xs text-[var(--warn)]"
                }
              >
                {run.result.ok
                  ? run.result.skipped_count > 0
                    ? `ok · ${run.result.skipped_count} skipped`
                    : "ok"
                  : `partial failure · failed steps [${run.result.failed.join(", ")}]`}
              </span>
            ) : null}
          </div>
          {run.error ? (
            <p className="text-sm text-[var(--danger)]">{run.error}</p>
          ) : run.result ? (
            <div className="space-y-3">
              <ul className="divide-y divide-[var(--border)] border-t border-[var(--border)]">
                {(run.result.steps || []).map((step) => {
                  const mark = step.skipped
                    ? "skip"
                    : step.ok
                      ? "ok"
                      : "FAIL";
                  const tone = step.skipped
                    ? "text-[var(--muted)]"
                    : step.ok
                      ? "text-[var(--muted)]"
                      : "text-[var(--danger)]";
                  return (
                    <li key={`${run.sceneId}-${step.index}`} className="py-2 text-sm">
                      <p className={tone}>
                        <span className="font-mono text-xs">[{step.index}]</span>{" "}
                        <span className="font-medium uppercase">{mark}</span>{" "}
                        <span className="font-mono text-xs">{step.entity_id}</span>{" "}
                        {step.action}
                      </p>
                      {step.error ? (
                        <p className="mt-1 text-xs text-[var(--danger)]">{step.error}</p>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
              <details>
                <summary className="cursor-pointer text-xs text-[var(--muted)]">
                  raw JSON
                </summary>
                <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-[var(--fg)]">
                  {JSON.stringify(run.result, null, 2)}
                </pre>
              </details>
            </div>
          ) : null}
        </div>
      )}
    </main>
  );
}
