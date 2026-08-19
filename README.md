# LanIoT Agent

Local-network, multi-brand IoT control agent. Talk to a web UI in natural language; the Rust Hub orchestrates auth, scenes, and MCP skills; a Python Agent + LLM decides tool calls; Home Assistant adapts brand protocols and ESP32 MQTT; Companion devices talk to the Hub over HTTP.

**Lan** = LAN-first · **IoT** = brand-agnostic device abstraction · **Agent** = NLU + tool orchestration

## Features

What this stack is built to do (and how it differs from "just wrap Home Assistant"):

| Feature | What you get |
|---------|----------------|
| **Device normalization engine** | Hub exposes a stable northbound API (REST / WS / MCP). Southbound backends implement `DeviceAdapter` and register on `AdapterRouter` — control is routed by `DeviceEntity.source` (`ha`, `faker`, …). Swap or add Matter / zigbee2mqtt later without changing Agent or UI. |
| **Machine-readable capabilities** | Each device carries derived `capabilities` (on/off, brightness, thermostat range, HVAC modes, …) from HA-style attributes — so the Agent can plan actions instead of guessing free-form params. |
| **Agent-native MCP skills** | Hub MCP tools (`devices.list` / `get_state` / `describe` / `control`, `scenes.run`, `companion.command`, …). The Python Agent calls **Hub only** — never HA or brands directly. |
| **`devices.describe`** | Returns capability summary + allowed actions/params for one entity — useful before `devices.control`. |
| **LAN-first + local LLM** | Prefer Ollama on-LAN; LiteLLM can fall back to cloud. Hub/Agent start even if the other side or HA is down. |
| **Offline / demo without HA** | Seeded **faker** brand stubs (Xiaomi / Gree / ESP32-tagged) via `SEED_FAKER_DEVICES` — same control path as live devices (`source=faker`). |
| **Companion out-of-band** | Phone/PC companions are Hub HTTP only (not HA). Demo clients under `companions/windows` and `companions/android`. |
| **Scenes + auth on MongoDB** | Pairing tokens, scenes, companions persist in Mongo when available; in-memory / TOML demos if Mongo is down. |
| **Dual deploy** | **Docker Compose** for daily work; **Singularity/Apptainer** (`deploy/singularity/`) for Linux edge / HPC / NAS — same images, parameterized `--llm` / `--profile`. |

Architecture principle: **HA is one pluggable adapter, not the product identity.** Brand coverage stays with HA (and future adapters); Hub owns the contract Agent and UI depend on.

## Architecture

Five layers (top → bottom):

1. **Client** — Next.js UI (chat / device panel / scenes · PC / mobile / PWA)
2. **Control plane** — Hub (Rust/Axum): unified API · auth · scenes · MCP · **AdapterRouter** (device engine)
3. **Intelligence** — Python Agent (LangGraph/FastAPI) + LLM via LiteLLM (Ollama ⇌ cloud)
4. **Device adaptation** — Home Assistant today (via `HaAdapter`); faker stubs for offline; more adapters later
5. **Devices** — brand IoT + ESP32 via HA; Companion via Hub HTTP

| Path | Notes |
|------|--------|
| Brand IoT | Through Home Assistant (vendor integrations) → Hub `source=ha` |
| ESP32 | **HA MQTT** — not Hub-direct |
| Faker demos | In-memory stubs → Hub `source=faker` (same REST/MCP/WS path) |
| Companion | **Hub HTTP only** — HA does not cover this path |
| Agent | Calls Hub MCP skills only — never HA / devices directly |

> Device paths: brand IoT and ESP32 go through Home Assistant; faker stubs share the same Hub API; Companion is Hub HTTP only; the Agent never talks to HA or brands directly.

## Repository structure

```
lan-iot-agent/
├── apps/
│   ├── hub/             # Rust Axum control plane (AdapterRouter, HA + faker, MCP, registry)
│   ├── agent/           # Python FastAPI intelligence layer
│   └── ui/              # Next.js client
├── companions/          # Windows + Android Companion demos
├── firmware/esp32/      # ESP32 MQTT stubs (via HA, not Hub-direct)
├── deploy/
│   ├── docker/          # docker-compose, Dockerfiles, HA + Mosquitto (primary)
│   └── singularity/     # P7 — .def + deploy.sh (build/start/stop/verify)
├── config/              # hub.toml, agent.toml, llm.toml
├── scripts/             # smoke.ps1 / smoke.sh
└── README.md
```

## Quick start

### Docker Compose (recommended)

```bash
cd deploy/docker
# Optional: export HA_TOKEN=<long-lived token from HA profile → Security>
docker compose up --build
```

| Service | URL |
|---------|-----|
| Hub health | http://localhost:3000/api/v1/health |
| Hub devices | http://localhost:3000/api/v1/devices |
| Agent health | http://localhost:8000/health |
| UI | http://localhost:3001 |
| Home Assistant | http://localhost:8123 |
| Mosquitto | localhost:1883 |
| MongoDB | localhost:27017 (Hub DB `lan_iot`) |

Optional local LLM:

```bash
docker compose --profile llm up --build
```

Hub and Agent intentionally do **not** hard-depend on each other. If HA is down or `HA_TOKEN` is unset, Hub still serves health and can use **faker** brand devices when seeded (`SEED_FAKER_DEVICES`); otherwise the device list may be empty with a warning (no crash).

`GET /api/v1/health` reports registered southbound adapters (e.g. `["faker","ha"]`). Device payloads include `source`, `capabilities`, and optional `brand` / `is_faker` metadata.

### Singularity / Apptainer (P7, Linux)

Same stack as Compose, packaged as `.sif` for hosts that prefer Apptainer. **Not for Windows** — use Compose there.

```bash
cd deploy/singularity
cp env.example .env    # set HA_TOKEN if you have HA
./deploy.sh up --llm=local --profile=full --ha-token="${HA_TOKEN}"
# Subcommands: build | start | stop | status | verify
```

| Profile | Services |
|---------|----------|
| `minimal` | mongo + hub + agent + ui |
| `full` / `dev` / `prod` | + mosquitto + Home Assistant |
| `--with-ollama` | also start an Ollama SIF |

Details: [deploy/singularity/README.md](deploy/singularity/README.md).

### Smoke checks

After Hub (and optionally Agent) are up:

```powershell
.\scripts\smoke.ps1
```

```bash
./scripts/smoke.sh
```

Prints PASS/FAIL/SKIP per check (Hub health, devices, scenes, companions, MCP tools; Agent health + chat).

### Device API + MCP (curl)

```bash
curl -s http://localhost:3000/api/v1/health
curl -s http://localhost:3000/api/v1/devices

curl -s -X POST http://localhost:3000/api/v1/devices/light.demo_esp32_light/actions \
  -H "Content-Type: application/json" \
  -d "{\"action\":\"turn_on\"}"

curl -s http://localhost:3000/mcp/tools
curl -s -X POST http://localhost:3000/mcp/call \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"devices.describe\",\"arguments\":{\"entity_id\":\"climate.demo_gree_ac\"}}"
```

Supported actions (routed by adapter): `turn_on`, `turn_off`, `toggle`, `set_temperature`, `set_brightness`, `set_hvac_mode`, …

### Demo ESP32 light (no hardware)

1. In HA UI: add **MQTT** integration → broker host `mosquitto`, port `1883`  
   (details: [deploy/docker/homeassistant/README.md](deploy/docker/homeassistant/README.md))
2. Publish a fake state:

```bash
cd deploy/docker
docker compose exec mosquitto mosquitto_pub -h localhost \
  -t home/demo/esp32_light/availability -m online
docker compose exec mosquitto mosquitto_pub -h localhost \
  -t home/demo/esp32_light/state -m "{\"state\":\"ON\",\"brightness\":180}"
```

3. `GET /api/v1/devices` should include `light.demo_esp32_light`.

Real Xiaomi gear: install **xiaomi_miot** via HACS (or core **xiaomi_miio**) inside HA — Hub code does not change per brand.

### Local development (outline)

```bash
# Hub (needs HA_URL / HA_TOKEN for live devices)
cd apps/hub
set HA_URL=http://localhost:8123
set HA_TOKEN=your_token
cargo run

# Agent
cd apps/agent && pip install -e . && uvicorn lan_iot_agent.main:app --reload --port 8000

# UI
cd apps/ui && cp .env.example .env && npm install && npm run dev
# HUB_URL=http://localhost:3000
```

**Windows note:** Hub build needs a working Rust linker. Prefer the Hub Docker image if local `cargo` linking fails. Singularity is Linux-only.
