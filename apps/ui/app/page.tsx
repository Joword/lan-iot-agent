import Link from "next/link";
import { MongoHealth } from "../components/MongoHealth";
import { HUB_URL, hubWsUrl } from "../lib/config";

export default function HomePage() {
  const ws = hubWsUrl();

  return (
    <main className="mx-auto flex max-w-2xl flex-col px-6 py-16">
      <p className="mb-3 text-sm uppercase tracking-[0.2em] text-[var(--muted)]">
        Local · Multi-brand · Agent
      </p>
      <h1 className="mb-4 text-4xl font-semibold tracking-tight sm:text-5xl">
        LanIoT Agent
      </h1>
      <p className="mb-10 max-w-xl text-lg leading-relaxed text-[var(--muted)]">
        Natural-language control for IoT devices on your LAN. Talk to the Agent;
        Hub orchestrates devices via Home Assistant.
      </p>

      <div className="mb-12 flex flex-wrap gap-3">
        <Link
          href="/devices"
          className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-2 text-sm font-medium hover:border-[var(--accent)]"
        >
          View devices
        </Link>
        <Link
          href="/scenes"
          className="rounded border border-[var(--border)] bg-[var(--surface)] px-4 py-2 text-sm font-medium hover:border-[var(--accent)]"
        >
          Scenes
        </Link>
        <Link
          href="/chat"
          className="rounded border border-[var(--accent)] bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white"
        >
          Open chat
        </Link>
      </div>

      <dl className="space-y-3 border-t border-[var(--border)] pt-6 text-sm">
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">Hub</dt>
          <dd className="font-mono text-xs sm:text-sm">
            <a
              className="text-[var(--accent)] underline-offset-4 hover:underline"
              href={`${HUB_URL}/api/v1/health`}
              target="_blank"
              rel="noreferrer"
            >
              {HUB_URL}/api/v1/health
            </a>
          </dd>
        </div>
        <MongoHealth />
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">WebSocket</dt>
          <dd className="font-mono text-xs sm:text-sm text-[var(--muted)]">
            {ws}
          </dd>
        </div>
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">Devices API</dt>
          <dd className="font-mono text-xs sm:text-sm">
            <a
              className="text-[var(--accent)] underline-offset-4 hover:underline"
              href={`${HUB_URL}/api/v1/devices`}
              target="_blank"
              rel="noreferrer"
            >
              {HUB_URL}/api/v1/devices
            </a>
          </dd>
        </div>
        <div className="flex gap-3">
          <dt className="w-28 shrink-0 text-[var(--muted)]">Scenes API</dt>
          <dd className="font-mono text-xs sm:text-sm">
            <a
              className="text-[var(--accent)] underline-offset-4 hover:underline"
              href={`${HUB_URL}/api/v1/scenes`}
              target="_blank"
              rel="noreferrer"
            >
              {HUB_URL}/api/v1/scenes
            </a>
          </dd>
        </div>
      </dl>
    </main>
  );
}
