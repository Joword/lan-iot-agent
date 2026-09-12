# LanIoT

<p><img src="https://img.shields.io/badge/Next.js-15.1-000000?style=flat&logo=nextdotjs&logoColor=white" alt="Next.js 15.1" /> <img src="https://img.shields.io/badge/Python-3.11%2B-3776AB?style=flat&logo=python&logoColor=white" alt="Python 3.11+" /> <img src="https://img.shields.io/badge/Rust-stable-DEA584?style=flat&logo=rust&logoColor=000" alt="Rust stable" /> <img src="https://img.shields.io/badge/Kotlin-2.0-7F52FF?style=flat&logo=kotlin&logoColor=white" alt="Kotlin 2.0" /> <img src="https://img.shields.io/badge/Docker-Compose-2496ED?style=flat&logo=docker&logoColor=white" alt="Docker Compose" /> <img src="https://img.shields.io/badge/MQTT-Mosquitto%202-660066?style=flat" alt="MQTT Mosquitto 2" /></p>

**A LAN-first console for the home: one sentence or one tap to switch lights, set the AC, run a scene, notify a PC, lock it, or send a robot already on the LAN home.**

Home Assistant, MQTT, and brand clouds are southbound adapters. They are not the product. LanIoT is the layer that sits above them: **one device surface, optional natural language, and traffic that stays on the LAN by default.**

<p align="center">
  <img src="assets/laniot-capabilities.png" alt="LanIoT capabilities: unified devices, chat, scenes, Companion ping/notify/lock, LAN robot stop/dock/start, and Chip Research&Develop as an R&D hook" width="900" />
</p>

## The problem we solve

Household control is usually split across three stacks that do not share a brain:

- Lights and climate live in a **vendor app** or in **Home Assistant**
- The household **PC, phone, or robot already on Wi-Fi** is not a device the hub can command
- A language model, if you add one, often calls HA or a cloud API **directly**, so permissions and failures leak outside your LAN

Swap a brand, and the logic layer changes. If Home Assistant is down, the demo dies. Locking a Windows session still means walking over to the machine. A robot on the same Wi-Fi still lives in its own app.

LanIoT’s job is narrower and more useful: **a Hub on the LAN owns devices, scenes, companions, and robots already on the network, and exposes a stable API. The Agent talks only to that Hub — never to Home Assistant, never to a bulb protocol.**

<p align="center">
  <img src="assets/laniot-problem.png" alt="Before: vendor apps, Home Assistant, cloud voice, and PC/phone/robot are disconnected. After: one LanIoT Hub on the LAN also commands a robot already on Wi-Fi" width="900" />
</p>

## What this project does

| You want | LanIoT does |
|----------|-------------|
| One list of stuff that can be controlled | Hub registry: lights, climate, plugs, sensors — with source and allowed actions, not a page per brand |
| “Set the AC to 26°C” / “away mode” | Agent turns that into Hub tools. No model installed? Keyword shortcuts still run |
| Bedtime / leaving home | Scenes turn lights and plugs off and set climate. Missing devices are **skipped**, not a hard fail |
| The PC as part of the home | Windows or Android Companion: ping, notify, lock. Lock is a real OS lock; this product cannot unlock |
| A robot already on the LAN | Same Hub HTTP as Companion (`kind=robot`, port **9879**). Scan → Adopt → Ping / Stop / Dock / Start. The stub is the contract; the chassis (or a thin bridge) replaces `companions/robot/server.py`. Not a vendor SDK |
| A demo that survives missing HA | In-memory stubs run the full path. Point HA at real Xiaomi / Gree / ESP32 later — Hub code does not fork per brand |
| A place to hang later chip / firmware R&D | Same Hub HTTP as Companion (`kind=chip`, port **9878**). Not a household device class — an entry so future silicon can plug in without a new northbound API |

**Degradation is intentional:** no HA → stubs; no LLM → keywords; Companion or robot offline → a visible `ok: false`, not a fake success.

## Architecture

Not a protocol maze: you, a console, a Hub, then home gear, a PC, **or** a robot already on the LAN. Intelligence is a client of the Hub.

<p align="center">
  <img src="assets/laniot-architecture.png" alt="You and the console talk to the Hub; the Agent uses MCP/REST to the Hub only; Hub fans out to Home Assistant or stubs, Companion PC/phone, and a LAN robot" width="900" />
</p>

- **Console** — devices, scenes, chat, Companions (Scan LAN) in the browser
- **Hub** — device book, scene engine, Companion HTTP, LAN scan/adopt, MCP tool catalog for the Agent
- **Agent** — optional LLM plus keywords; **MCP/REST to the Hub only**
- **Southbound** — Home Assistant when you have hardware; faker stubs when you do not
- **Companion** — Hub HTTP only. Never HA. **PC/phone are the product path** (`:9876` / `:9877`)
- **Robot** — same HTTP as Companion (`kind=robot`, **9879**) for a chassis already on the LAN. Scan / adopt / Stop / Dock / Start. Not a brand SDK
- **Chip hook** — optional `kind=chip` on **9878**, same HTTP contract, so firmware R&D can attach later without changing the console or Agent. Not a home device type

MongoDB is used when present (scenes, companions). If it is down, the Hub still boots from config and memory.

## Who it is for

- A **Windows / lab / no-public-internet** demo that puts multi-brand IoT, a PC, and a robot already on Wi-Fi on the same console
- Teams that want “natural language → real action” **without** binding the LLM to Home Assistant
- Anyone who will later swap stubs for real hardware — **changing Hub code per brand is not the goal**. Chip HTTP is the firmware R&D door, not a shipped device class

This is a **demo prototype**, not a consumer cloud app and not a Home Assistant replacement. Forced login stays **off** by default.

## Privacy and safety

- Default path is **LAN / loopback**. Nothing requires a vendor cloud.
- The Agent **never** calls Home Assistant. Only the Hub’s HA adapter does, when configured.
- Completions leave the LAN only if **you** point the LLM at a cloud provider. Local Ollama stays on-site.
- **Companion Lock really locks Windows.** There is no Unlock here — use the OS password. Dry runs: `COMPANION_DRY_RUN=1`. Do not click Lock on a shared demo machine unless that is the point.
- **Robot Stop / Dock / Start** go to whatever you adopted on **9879**. On the stub that is local state; on a real chassis it is the robot. Do not adopt a machine you do not own.

## How to start with Docker

An empty `HA_TOKEN` is fine — you get in-memory demo devices.

```bash
cd deploy/docker
cp .env.example .env
docker compose up --build
```

| Open | What |
|------|------|
| http://localhost:3001 | Console |
| http://localhost:3001/companions | Companions — Scan LAN / Adopt |
| http://localhost:3000/api/v1/devices | What the Hub sees |
| http://localhost:8000/health | Agent |

Optional local LLM: `docker compose --profile llm up --build`.

Listeners already on the LAN (Scan finds `GET /health` with `lan_iot: true`):

| Listener | Port | `kind` | Commands |
|----------|------|--------|----------|
| Windows Companion | **9876** | `pc` | ping, notify, lock |
| Android Companion | **9877** | `phone` | ping, notify, lock |
| Robot (already on the LAN) | **9879** | `robot` | ping, stop, dock, start |
| Chip (R&D hook, not a home device) | **9878** | `chip` | ping, on, off |

Windows Companion (Hub on port 3000):

```powershell
python companions\windows\server.py --hub http://localhost:3000
```

Then open Companions → **Scan LAN** → Adopt → Ping / Notify. Skip Lock unless you mean it.

If Hub is in Docker, set `LAN_SCAN_HOSTS=host.docker.internal` and advertise Companion with `--base-url http://host.docker.internal:9876`. Extra ports: `LAN_SCAN_PORTS`.

Robot already on the LAN (optional):

```powershell
python companions\robot\server.py
```

Companions → **Scan LAN** → Adopt → Ping / Stop / Dock. Replace that process with the robot’s own HTTP later; Hub scan/adopt does not change.

Chip / firmware R&D (optional, not part of the home demo):

```powershell
python companions\chip\server.py
```

Same Scan → Adopt path. On/Off only. Swap that process for real silicon later; do not treat it as a shipped home device.

## In the demo vs not

**Ships today:** multi-brand stubs, scenes, optional language, PC/phone companions (scan + adopt), robot HTTP on the same contract (`kind=robot`), Compose bring-up. Chip HTTP is an R&D hook, not a consumer device type.

**Not this product:** public-account auth as the default, unlock-from-app, replacing Home Assistant, a Windows tray, a vendor robot SDK.

**Next on a real home:** enable HA integrations for Xiaomi / Gree / ESP32; walk the UI, Companion, and a robot already on Wi-Fi on a clean machine.

## FAQ

**How do I control a PC already on the LAN?**  
Run `companions/windows/server.py` (`:9876`). Companions → **Scan LAN** → Adopt. Extra hosts: `LAN_SCAN_HOSTS`.

**How do I control a robot already on the LAN?**  
Same Hub HTTP as Companion, `kind=robot`, port **9879**. Run `companions/robot/server.py` (or point Hub at the robot’s own `GET /health` + `POST /command`). Scan → Adopt → Ping / Stop / Dock / Start. Not a vendor SDK — the chassis must speak this contract, or sit behind a thin bridge.

**Where does chip / firmware R&D plug in?**  
Same Hub HTTP as Companion, `kind=chip`, port **9878**. `companions/chip/server.py` is the stand-in contract (`lan_iot` health + on/off). Future firmware replaces that process; do not treat it as a shipped home device.

**Do I need Home Assistant?** No. Stubs are enough to walk the UI, Agent, and scenes. Real bulbs can still come in through HA later.

**Do I need a GPU?** No. A local LLM is optional; keywords work without one.

**I locked Windows from the UI.** Use the Windows password or PIN. LanIoT cannot unlock.

**Real Xiaomi bulb or Gree AC?** Through HA (`xiaomi_miot`, `gree`, MQTT ESP32). Set `SEED_FAKER_DEVICES=0` if you only want live HA.

**Does the Agent talk to HA?** No. Hub MCP/REST only.

**Where is what in the repo?**

| Path | Role |
|------|------|
| `apps/ui` | Console |
| `apps/hub` | Hub |
| `apps/agent` | Natural language |
| `companions/windows` | PC listener `:9876` |
| `companions/android` | Phone listener `:9877` |
| `companions/robot` | Robot HTTP contract `:9879` |
| `companions/chip` | Firmware R&D HTTP hook `:9878` |
| `deploy/docker` | Everyday compose |
| `config/` | Scenes and service config |
