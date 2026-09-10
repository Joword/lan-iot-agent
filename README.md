# LanIoT Agent

LAN-first IoT control: Next.js UI → Rust Hub (REST / WS / MCP) → Python Agent + LLM. Home Assistant is one southbound adapter, not the product. Companion PC/phone talks to Hub over HTTP only.

This file is the **only public README**. Nested READMEs and `docs/` are local notes (gitignored). Annotations below are for people hacking on the repo.

## Layout

```
apps/hub          Rust Axum control plane (AdapterRouter: ha + faker)
apps/agent        Python FastAPI / LangGraph (calls Hub MCP only)
apps/ui           Next.js (port 3001 in Compose, 3000 in next dev)
companions/       Windows (9876) + Android (9877) HTTP listeners
firmware/esp32/   MQTT stubs — go through HA, not Hub-direct
deploy/docker     day-to-day Compose
deploy/singularity  Linux .sif (not for Windows)
config/           hub.toml, agent.toml, llm.toml, scenes.toml
scripts/          smoke.ps1 / smoke.sh
```

## Bring-up

Compose (faker stubs on by default via `SEED_FAKER_DEVICES=1`):

```bash
cd deploy/docker
cp .env.example .env   # HA_TOKEN empty is fine for faker-only
docker compose up --build
```

| URL | What |
|-----|------|
| http://localhost:3000/api/v1/health | Hub |
| http://localhost:3000/api/v1/devices | registry (`source`, `capabilities`) |
| http://localhost:8000/health | Agent |
| http://localhost:3001 | UI |
| http://localhost:8123 | HA |
| localhost:1883 | Mosquitto |
| localhost:27017 | Mongo (`lan_iot`) |

```bash
docker compose --profile llm up --build          # + Ollama
docker compose --profile smoke up --abort-on-container-exit   # smoke.sh after healthy
```

Linux Singularity: `cd deploy/singularity && cp env.example .env && ./deploy.sh up --llm=local --profile=full`. SIFs land in `sif/` (gitignored). Builds pull image URIs (`docker://`, `docker-daemon://`), which need no root. Dropping a `<service>.def` next to `deploy.sh` overrides the URI, but def-file builds need root — the script adds `--fakeroot` for you. Only add a def when it carries real `%files` / `%environment` / `%runscript`.

Host processes (no Compose):

```powershell
# Hub — GNU linker on this Windows box
cd apps/hub
$env:HA_URL="http://localhost:8123"
$env:SEED_FAKER_DEVICES="1"
cargo run --target x86_64-pc-windows-gnu

cd apps/agent
pip install -e .
uvicorn lan_iot_agent.main:app --reload --port 8000

cd apps/ui
cp .env.example .env
npm install
npm run dev
```

## Env (the ones that bite)

| Var | Default | Notes |
|-----|---------|--------|
| `SEED_FAKER_DEVICES` | Compose `1`; Hub process: on if HA is unset | In-memory `*.faker_*` stubs. Set `0` when you only want live HA. |
| `AUTH_REQUIRED` | `false` | **Keep false for the demo.** `true` needs Mongo; UI Pair + Companion `--pair` become mandatory or REST is 401. |
| `HA_URL` / `HA_TOKEN` | unset | Live HA. Hub still boots without them. |
| `SCENES_RESEED` | `false` | Re-seed `config/scenes.toml` over an existing Mongo `scenes` collection. |
| `HUB_TOOL_CATALOG_REFRESH` | `true` | Agent pulls the LLM tool list from `GET {HUB_MCP_URL}/tools`. `0` pins the static catalog (the test suite does this). |
| `HUB_TIMEOUT_SECONDS` / `HUB_MAX_RETRIES` | `5.0` / `3` | Agent→Hub. An absent Hub costs roughly timeout × retries per call. |
| `HUB_MCP_URL` | Agent → `http://127.0.0.1:3000/mcp` | Agent never talks to HA. |
| `COMPANION_DRY_RUN` | unset | Skip Windows toast/lock (tests). |

## Faker ids vs HA MQTT

In-memory faker (scenes, smoke, Agent defaults):

- `light.faker_esp32_light`
- `climate.faker_gree_ac`
- `light.faker_xiaomi_bulb`
- `switch.faker_xiaomi_plug`
- `sensor.faker_esp32_temperature`

HA MQTT fixtures in `deploy/docker/homeassistant/mqtt.yaml` are all `*.demo_*`. Keep that split when adding fixtures — a shared `entity_id` means one silently replaces the other in the registry. On a collision **live HA wins**: the faker seed is skipped if HA already owns the id, and an HA sync overwrites a faker stub (logged as a warning).

`config/scenes.toml` and the built-in demo scenes list both families, so a scene works whether you booted faker stubs or HA MQTT; absent entities come back as skipped steps. Scenes are persisted to Mongo on first boot, so editing the TOML afterwards does nothing — set `SCENES_RESEED=1` for one boot to overwrite the seeded ids (scenes you authored under other ids survive).

Real brand gear: enable the HA integration (xiaomi_miot / gree / MQTT ESP32). Hub code does not change per brand.

## Companion

Windows (binds `0.0.0.0:9876`, registers on Hub):

```powershell
python companions\windows\server.py --hub http://localhost:3000
# Hub in Docker:
python companions\windows\server.py --hub http://localhost:3000 --base-url http://host.docker.internal:9876
# Inbound blocked:
python companions\windows\server.py --hub http://localhost:3000 --open-firewall   # Administrator
```

There is no tray app — leave the console running. `Lock` really locks the session. **No Unlock** from UI or OS without credentials.

Android: `cd companions/android && ./gradlew assembleDebug` (JDK 17). Pair advertises a real WLAN IPv4. Emulator registers `http://127.0.0.1:9877` — run `adb forward tcp:9877 tcp:9877`. Hub URL from the emulator is still `http://10.0.2.2:3000`.

## Agent / Hub contract

Agent LLM tools prefer live `GET {HUB_MCP_URL}/tools`. If Hub is down, Agent falls back to the static catalog in `apps/agent/.../schemas.py`. Keyword stubs still work without an LLM (`pip install -e ".[llm]"` for LiteLLM).

Northbound device I/O goes through `AdapterRouter` (`ha` | `faker`). Do not add new `state.ha` special cases.

Every Agent→Hub `httpx.AsyncClient` passes `trust_env=False`. Hub is on the LAN or loopback, and httpx's default `trust_env=True` picks up the **Windows registry** proxy via `urllib.getproxies()` — not just `HTTP_PROXY` — so a machine with a system proxy configured gets a `502` on every Hub call, three times over with retries. Keep the flag on any new client.

## Tests / CI

```powershell
cd apps\agent
.\.venv\Scripts\python.exe -m pytest -q

cd apps\hub
cargo test --target x86_64-pc-windows-gnu

.\scripts\smoke.ps1
```

GitHub Actions (`.github/workflows/ci.yml`): Agent pytest + Hub `cargo test` on Ubuntu. The Hub job sets `RUSTUP_TOOLCHAIN` / `CARGO_BUILD_TARGET` because `apps/hub/rust-toolchain.toml` and `.cargo/config.toml` pin windows-gnu for local dev — env wins over both, so don't drop it. Compose smoke is not in CI (image build is too heavy). After `docker compose up -d`, run `.\scripts\smoke.ps1` or `make smoke`.

## curl scratchpad

```bash
curl -s http://localhost:3000/api/v1/health
curl -s http://localhost:3000/api/v1/devices
curl -s -X POST http://localhost:3000/api/v1/devices/light.faker_esp32_light/actions \
  -H "Content-Type: application/json" -d "{\"action\":\"turn_on\"}"
curl -s http://localhost:3000/mcp/tools
```

MQTT demo light (HA path, entity `light.demo_esp32_light`): in HA add MQTT broker `mosquitto:1883`, then `deploy/docker/homeassistant/publish_demo_states.ps1`.
