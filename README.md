# LanIoT

A LAN-first home-control console: one sentence or one tap to switch lights, set the AC, run a scene, notify a PC, or lock it.

Home Assistant, MQTT, and brand clouds are **southbound adapters**, not the product. The product is a **single device surface**, **natural-language control**, and **traffic that stays on the LAN by default**.

## The problem

Home devices usually live in three stacks that do not talk to each other:

- Lights and climate sit in Home Assistant or a vendor app
- The household PC or phone is not a first-class device the hub can command
- Natural-language control often calls HA or a cloud API directly, so permissions and failures are hard to bound

Swap a brand, and the logic layer changes. If HA is down, the demo dies. Locking a Windows session still means walking over to the machine.

LanIoT collapses that into one job: **a hub on the LAN that owns devices, scenes, and companions, and exposes a stable API. The intelligence layer (Agent) talks only to the Hub — never to Home Assistant.**

## Who it is for

- People who need a **Windows / lab / no-public-internet** demo that puts multi-brand IoT and a PC on the same console
- Teams that want “natural language → real action” without binding the LLM to Home Assistant
- Developers who will later swap faker stubs for real Xiaomi / Gree / ESP32 gear (changing Hub code per brand is not the goal)

The default is **demo mode** (no forced login). This is not a consumer cloud app and not a replacement for Home Assistant.

## What you can do

| Capability | What you actually see |
|------------|------------------------|
| Unified devices | Lights, climate, plugs, sensors — with source and allowed actions, not a page per brand |
| Natural language | “Set the AC to 26°C”, “away mode” → Agent calls Hub tools. No model? Keyword shortcuts still run |
| Scenes | Sleep / away: lights off, plug off, AC set. Missing devices are skipped; the scene does not abort |
| Companion | A Windows PC or Android phone registers with the Hub: ping, notify, lock session (lock is real; this UI cannot unlock) |
| Pluggable southbound | No HA? In-memory stubs still run the full path. With HA, the same API applies; brands enter through HA integrations |

The Agent **only calls Hub MCP/REST**. A new light brand means enabling it in HA, not rewriting the Agent.

## How this differs

| | Vendor app | Home Assistant alone | Cloud voice assistant | **LanIoT** |
|--|------------|----------------------|------------------------|------------|
| Device source | One brand | HA integrations | Vendor + cloud account | Hub registry (HA **or** demo stubs) |
| LLM talks to | — | Often HA / REST directly | Cloud | **Hub only** |
| PC / phone as a device | No | Not first-class | Rarely | Companion over Hub HTTP |
| Works without cloud | Depends | Yes | No | **Yes** (optional local or cloud LLM) |
| Works without HA | No | — | Yes | **Yes** (faker stubs) |

## Degradation is a feature

- **No Home Assistant** — demo stubs still list, control, and run scenes
- **No LLM** — keyword routing still drives Hub tools (`pip install -e ".[llm]"` only when you want LiteLLM)
- **Companion offline** — the Hub returns a visible failure (`ok: false`), not a fake success
- **HA comes back later** — live HA entities win if an id collides with a stub

## Data and privacy

- Default path is **LAN / loopback**. Hub, UI, and Agent do not require a vendor cloud.
- The Agent **never calls Home Assistant**. Only the Hub’s HA adapter does, when configured.
- Demo auth is **off** (`AUTH_REQUIRED=false`). Do not expose the ports on a hostile network.
- Packets leave the LAN only if **you** point the LLM at a cloud provider. Local Ollama keeps completions on-site.

## Safety

**Companion Lock really locks the Windows session.** There is no Unlock in this product; you need the OS password. Do not click Lock on a shared demo machine unless that is the point. Tests and dry runs should set `COMPANION_DRY_RUN=1`.

## What is in scope vs not

**In the demo today:** multi-brand stubs, scenes, optional natural language, PC/phone as companions, Docker one-command bring-up.

**Intentionally not:** public-account auth as the default, unlock-from-app, replacing Home Assistant, a Windows tray app.

**Next (product, not a public roadmap):** point HA at real Xiaomi / Gree / ESP32 devices; walk the UI and Companion on a clean machine.

## Three beats (about two minutes)

1. **Bedtime** — open the console → Sleep Mode → lights/plug off, AC off (or skipped if that device is absent).
2. **Leaving** — say or type away mode → cool/off setpoints run on whatever climate exists.
3. **At the desk** — Windows Companion stays registered → Ping or Notify from the console. Skip Lock unless you mean it.

## Five-minute bring-up

Docker is enough. An empty `HA_TOKEN` is fine (in-memory demo devices).

```bash
cd deploy/docker
cp .env.example .env
docker compose up --build
```

| Open | What |
|------|------|
| http://localhost:3001 | Console (devices, scenes, chat, companions) |
| http://localhost:3000/api/v1/devices | What the Hub currently sees |
| http://localhost:8000/health | Agent liveness |

Optional local LLM: `docker compose --profile llm up --build`.

Windows Companion (Hub already on port 3000):

```powershell
python companions\windows\server.py --hub http://localhost:3000
```

If Hub runs in Docker, advertise a URL the container can dial:

```powershell
python companions\windows\server.py --hub http://localhost:3000 --base-url http://host.docker.internal:9876
```

## Where it runs

| Setting | What you need |
|---------|----------------|
| Everyday demo | One PC with Docker; no GPU |
| Natural language | Optional: Ollama on the machine, or a cloud LLM key |
| Lab / HPC | Linux Singularity/Apptainer (`deploy/singularity`); not for Windows |
| Without Compose | Run Hub, Agent, and UI as host processes |

Stack, one line: **Next.js console → Rust Hub → Python Agent**. Companions are small HTTP listeners on Windows and Android.

## How the pieces split

```
You (browser / a sentence)
        ↓
Console UI
        ↓
Hub  — device book, scenes, companions, tool catalog for the Agent
   ↓                         ↓
Home adapters              PC / phone Companion
(HA or demo stubs)         (Hub HTTP only)
```

The Agent (optional LLM) only asks the Hub “which tools exist, run this.” It does not speak bulb protocols or OS APIs.

## Repository map

| Path | Role |
|------|------|
| `apps/ui` | Console |
| `apps/hub` | Hub: devices, scenes, companions, MCP |
| `apps/agent` | Natural language and tool orchestration |
| `companions/` | Windows / Android agents |
| `deploy/docker` | Day-to-day demo compose |
| `config/` | Scenes and service config |

## FAQ

**Do I need Home Assistant?**  
No. Stubs are enough to walk the UI, Agent, and scenes. Enable HA when you have real hardware.

**Do I need a GPU?**  
No. The console and Hub do not. A local LLM is optional; keywords work without one.

**I locked Windows from the UI. How do I get back?**  
Use the Windows password (or PIN). LanIoT cannot unlock the session.

**Can this drive a real Xiaomi bulb or Gree AC?**  
Yes, through Home Assistant integrations (`xiaomi_miot`, `gree`, MQTT ESP32). Hub code does not fork per brand. Set `SEED_FAKER_DEVICES=0` if you only want live HA.

**Does the Agent talk to HA?**  
No. `HUB_MCP_URL` / Hub REST only.

## Status

Open-source **demo prototype**. Forced auth stays off by default. Review what you expose before putting this on a real home LAN.
