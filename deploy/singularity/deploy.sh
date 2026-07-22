#!/usr/bin/env bash
# LanIoT Agent - Singularity / Apptainer parameterized deploy (P7)
#
# Docker Compose remains the day-to-day path. This script converts Compose-built
# images to .sif and manages instances for HPC / edge / NAS hosts.
#
# Usage:
#   ./deploy/singularity/deploy.sh up --llm=local --profile=full --ha-token=xxx
#   ./deploy/singularity/deploy.sh build|start|stop|status|verify|help
#
# Without Singularity installed: prints the plan and exits 0 (unless --strict).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
DOCKER_COMPOSE="${REPO_ROOT}/deploy/docker/docker-compose.yml"
SIF_DIR="${SCRIPT_DIR}/sif"
DATA_DIR="${SCRIPT_DIR}/data"
INSTANCE_PREFIX="lan-iot"
HA_CONFIG="${REPO_ROOT}/deploy/docker/homeassistant"
MOSQUITTO_CONF="${REPO_ROOT}/deploy/docker/mosquitto/mosquitto.conf"
HUB_TOML="${REPO_ROOT}/config/hub.toml"
AGENT_TOML="${REPO_ROOT}/config/agent.toml"
LLM_TOML="${REPO_ROOT}/config/llm.toml"

# Prefer `apptainer` when present (modern rename); fall back to `singularity`.
SIF_BIN=""
if command -v apptainer >/dev/null 2>&1; then
  SIF_BIN="apptainer"
elif command -v singularity >/dev/null 2>&1; then
  SIF_BIN="singularity"
fi

CMD="up"
LLM="local"
MODEL="qwen2.5:7b"
PROFILE="full"
HA_TOKEN="${HA_TOKEN:-}"
AUTH_REQUIRED="${AUTH_REQUIRED:-false}"
MONGODB_URI="${MONGODB_URI:-mongodb://127.0.0.1:27017}"
MONGODB_DATABASE="${MONGODB_DATABASE:-lan_iot}"
SEED_FAKER_DEVICES="${SEED_FAKER_DEVICES:-1}"
STRICT=0
SKIP_DOCKER_BUILD=0
DRY_RUN=0
WITH_OLLAMA=0

usage() {
  cat <<'EOF'
LanIoT Singularity / Apptainer deploy (P7)

Usage:
  deploy.sh <command> [options]

Commands:
  up        build SIFs + start instances + verify (default)
  build     convert Docker images -> .sif only
  start     start instances (requires SIFs)
  stop      stop all lan-iot-* instances
  status    list instances + curl Hub/Agent health
  verify    health checks (Hub, Agent, optional HA/Mongo)
  help      show this help

Options:
  --llm=local|cloud|hybrid   LLM mode (default: local)
  --model=NAME                 Model id (default: qwen2.5:7b)
  --profile=minimal|full|dev|prod
                               minimal = hub+agent+ui+mongo
                               full|dev|prod = + ha + mosquitto
  --ha-token=TOKEN             Home Assistant long-lived access token
  --with-ollama                Also build/start Ollama SIF (local/hybrid)
  --skip-docker-build          Assume Docker images already tagged
  --dry-run                    Print planned commands only
  --strict                     Exit 1 if singularity/apptainer or docker missing

Docker -> Singularity (local images):
  singularity build sif/hub.sif   docker-daemon://lan-iot-hub:latest

Remote registry (HA / mongo / mosquitto):
  singularity build sif/ha.sif docker://ghcr.io/home-assistant/home-assistant:stable

Env overrides: HA_TOKEN, AUTH_REQUIRED, MONGODB_URI, MONGODB_DATABASE,
  SEED_FAKER_DEVICES, OLLAMA_URL, LLM_PROVIDER, OPENAI_API_KEY, ...
EOF
}

log()  { printf '[deploy] %s\n' "$*"; }
warn() { printf '[deploy] WARN: %s\n' "$*" >&2; }
die()  { printf '[deploy] ERROR: %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

run() {
  if [[ "${DRY_RUN}" -eq 1 ]]; then
    printf '+ %s\n' "$*"
    return 0
  fi
  "$@"
}

# --- parse args --------------------------------------------------------------
ARGS=()
for arg in "$@"; do
  case "${arg}" in
    up|build|start|stop|status|verify|help) CMD="${arg}" ;;
    --llm=*)              LLM="${arg#*=}" ;;
    --model=*)            MODEL="${arg#*=}" ;;
    --profile=*)          PROFILE="${arg#*=}" ;;
    --ha-token=*)         HA_TOKEN="${arg#*=}" ;;
    --with-ollama)        WITH_OLLAMA=1 ;;
    --skip-docker-build)  SKIP_DOCKER_BUILD=1 ;;
    --dry-run)            DRY_RUN=1 ;;
    --strict)             STRICT=1 ;;
    -h|--help)            usage; exit 0 ;;
    *)                    ARGS+=("${arg}") ;;
  esac
done

if [[ ${#ARGS[@]} -gt 0 ]]; then
  die "unknown argument(s): ${ARGS[*]} (try --help)"
fi

case "${CMD}" in
  help) usage; exit 0 ;;
esac

case "${LLM}" in
  local|cloud|hybrid) ;;
  *) die "--llm must be local|cloud|hybrid (got: ${LLM})" ;;
esac

case "${PROFILE}" in
  minimal|full|dev|prod) ;;
  *) die "--profile must be minimal|full|dev|prod (got: ${PROFILE})" ;;
esac
# Aliases: full == prod topology
[[ "${PROFILE}" == "dev" || "${PROFILE}" == "prod" ]] && PROFILE_EFFECTIVE="full" || PROFILE_EFFECTIVE="${PROFILE}"
[[ "${PROFILE}" == "full" ]] && PROFILE_EFFECTIVE="full"

missing=()
[[ -z "${SIF_BIN}" ]] && missing+=("singularity|apptainer")
have docker || missing+=("docker")

if [[ ${#missing[@]} -gt 0 ]]; then
  msg="missing tools: ${missing[*]} - prefer deploy/docker/ for day-to-day"
  if [[ "${STRICT}" -eq 1 ]]; then
    die "${msg}"
  fi
  warn "${msg}"
  warn "Printing planned actions (non-strict). Install Apptainer/Singularity + Docker to execute."
  DRY_RUN=1
fi

# --- LLM env -----------------------------------------------------------------
case "${LLM}" in
  local)
    export LLM_PROVIDER=ollama
    export OLLAMA_URL="${OLLAMA_URL:-http://127.0.0.1:11434}"
    ;;
  cloud)
    export LLM_PROVIDER="${LLM_PROVIDER:-openai}"
    unset OLLAMA_URL 2>/dev/null || true
    ;;
  hybrid)
    export LLM_PROVIDER=ollama
    export OLLAMA_URL="${OLLAMA_URL:-http://127.0.0.1:11434}"
    export LLM_FALLBACK_PROVIDER="${LLM_FALLBACK_PROVIDER:-openai}"
    ;;
esac
export LLM_MODEL="${MODEL}"
export HA_TOKEN
export AUTH_REQUIRED
export MONGODB_URI
export MONGODB_DATABASE
export SEED_FAKER_DEVICES

# --- service sets ------------------------------------------------------------
# Always include mongo for Hub persistence (matches Compose).
CORE_SERVICES=(mongo hub agent ui)
EXTRA_SERVICES=()
case "${PROFILE_EFFECTIVE}" in
  minimal) ;;
  full) EXTRA_SERVICES+=(mosquitto ha) ;;
esac
if [[ "${WITH_OLLAMA}" -eq 1 ]]; then
  EXTRA_SERVICES+=(ollama)
fi

ALL_SERVICES=("${CORE_SERVICES[@]}" "${EXTRA_SERVICES[@]}")

instance_name() { printf '%s-%s' "${INSTANCE_PREFIX}" "$1"; }

ensure_dirs() {
  mkdir -p "${SIF_DIR}" \
    "${DATA_DIR}/mongo" \
    "${DATA_DIR}/mosquitto" \
    "${DATA_DIR}/ollama" \
    "${DATA_DIR}/ha"
  # HA config: reuse Compose tree when present; else empty writable dir
  if [[ ! -d "${HA_CONFIG}" ]]; then
    warn "HA config missing at ${HA_CONFIG} - using ${DATA_DIR}/ha"
  fi
}

# --- Docker image prepare ----------------------------------------------------
ensure_docker_images() {
  if [[ "${SKIP_DOCKER_BUILD}" -eq 1 ]]; then
    log "skipping docker compose build (--skip-docker-build)"
    return 0
  fi
  if ! have docker; then
    warn "docker not available; cannot build images"
    return 0
  fi
  log "building Docker images via compose (hub agent ui)"
  run docker compose -f "${DOCKER_COMPOSE}" build hub agent ui

  local project
  project="$(docker compose -f "${DOCKER_COMPOSE}" config --format json 2>/dev/null \
    | sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1 || true)"
  if [[ -z "${project}" ]]; then
    project="docker"
  fi
  for svc in hub agent ui; do
    local target="lan-iot-${svc}:latest"
    local candidates=(
      "${project}-${svc}:latest"
      "deploy-docker-${svc}:latest"
      "docker-${svc}:latest"
      "lan-iot-${svc}:latest"
    )
    local found=""
    for c in "${candidates[@]}"; do
      if docker image inspect "${c}" >/dev/null 2>&1; then
        found="${c}"
        break
      fi
    done
    if [[ -n "${found}" ]]; then
      if [[ "${found}" != "${target}" ]]; then
        log "tag ${found} -> ${target}"
        run docker tag "${found}" "${target}"
      else
        log "image ready: ${target}"
      fi
    else
      warn "could not find built image for ${svc}; tag manually to ${target}"
    fi
  done
}

build_one_sif() {
  local name="$1"
  local source="$2"
  local out="${SIF_DIR}/${name}.sif"
  local def="${SCRIPT_DIR}/${name}.def"

  if [[ -n "${SIF_BIN}" ]]; then
    if [[ -f "${def}" ]]; then
      log "${SIF_BIN} build ${out} <- ${def}"
      run "${SIF_BIN}" build --force "${out}" "${def}"
    else
      log "${SIF_BIN} build ${out} <- ${source}"
      run "${SIF_BIN}" build --force "${out}" "${source}"
    fi
  else
    log "PLAN: singularity build ${out} ${source}"
  fi
}

cmd_build() {
  ensure_dirs
  ensure_docker_images
  log "converting images -> SIFs under ${SIF_DIR}"
  build_one_sif mongo     "docker://mongo:7"
  build_one_sif hub       "docker-daemon://lan-iot-hub:latest"
  build_one_sif agent     "docker-daemon://lan-iot-agent:latest"
  build_one_sif ui        "docker-daemon://lan-iot-ui:latest"
  if [[ "${PROFILE_EFFECTIVE}" == "full" ]]; then
    build_one_sif mosquitto "docker://eclipse-mosquitto:2"
    build_one_sif ha        "docker://ghcr.io/home-assistant/home-assistant:stable"
  fi
  if [[ "${WITH_OLLAMA}" -eq 1 ]]; then
    build_one_sif ollama "docker://ollama/ollama:latest"
  fi
  log "build complete"
}

stop_one() {
  local name="$1"
  local inst
  inst="$(instance_name "${name}")"
  if [[ -z "${SIF_BIN}" ]]; then
    log "PLAN: ${SIF_BIN:-singularity} instance stop ${inst}"
    return 0
  fi
  if "${SIF_BIN}" instance list 2>/dev/null | grep -qE "(^|[[:space:]])${inst}([[:space:]]|$)"; then
    log "stopping ${inst}"
    run "${SIF_BIN}" instance stop "${inst}" || true
  else
    log "instance ${inst} not running"
  fi
}

cmd_stop() {
  # Stop dependents first, then data plane
  local order=(ui agent hub ha mosquitto ollama mongo)
  for s in "${order[@]}"; do
    stop_one "${s}"
  done
  log "stop complete"
}

start_one() {
  local name="$1"
  shift
  local sif="${SIF_DIR}/${name}.sif"
  local inst
  inst="$(instance_name "${name}")"

  if [[ ! -f "${sif}" && "${DRY_RUN}" -eq 0 && -n "${SIF_BIN}" ]]; then
    die "missing ${sif} - run: $0 build"
  fi

  if [[ -n "${SIF_BIN}" ]]; then
    if "${SIF_BIN}" instance list 2>/dev/null | grep -qE "(^|[[:space:]])${inst}([[:space:]]|$)"; then
      log "restarting ${inst}"
      run "${SIF_BIN}" instance stop "${inst}" || true
    fi
    log "starting ${inst}"
    run "${SIF_BIN}" instance start "$@" "${sif}" "${inst}"
  else
    log "PLAN: singularity instance start $* ${sif} ${inst}"
  fi
}

cmd_start() {
  ensure_dirs
  log "llm=${LLM} model=${MODEL} profile=${PROFILE} services=${ALL_SERVICES[*]}"

  # mongo first
  start_one mongo \
    --bind "${DATA_DIR}/mongo:/data/db"

  if [[ "${PROFILE_EFFECTIVE}" == "full" ]]; then
    start_one mosquitto \
      --bind "${MOSQUITTO_CONF}:/mosquitto/config/mosquitto.conf:ro" \
      --bind "${DATA_DIR}/mosquitto:/mosquitto/data"

    local ha_bind="${HA_CONFIG}"
    [[ -d "${ha_bind}" ]] || ha_bind="${DATA_DIR}/ha"
    start_one ha \
      --bind "${ha_bind}:/config" \
      --env "TZ=${TZ:-UTC}"
  fi

  if [[ "${WITH_OLLAMA}" -eq 1 ]]; then
    start_one ollama \
      --bind "${DATA_DIR}/ollama:/root/.ollama"
  fi

  start_one hub \
    --env "HA_URL=http://127.0.0.1:8123" \
    --env "HA_TOKEN=${HA_TOKEN}" \
    --env "AGENT_URL=http://127.0.0.1:8000" \
    --env "AUTH_REQUIRED=${AUTH_REQUIRED}" \
    --env "MONGODB_URI=${MONGODB_URI}" \
    --env "MONGODB_DATABASE=${MONGODB_DATABASE}" \
    --env "SEED_FAKER_DEVICES=${SEED_FAKER_DEVICES}" \
    --bind "${HUB_TOML}:/etc/lan-iot/hub.toml:ro"

  start_one agent \
    --env "LLM_PROVIDER=${LLM_PROVIDER}" \
    --env "LLM_MODEL=${LLM_MODEL}" \
    --env "OLLAMA_URL=${OLLAMA_URL:-}" \
    --env "LLM_FALLBACK_PROVIDER=${LLM_FALLBACK_PROVIDER:-}" \
    --env "HUB_URL=http://127.0.0.1:3000" \
    --env "HUB_MCP_URL=http://127.0.0.1:3000/mcp" \
    --bind "${AGENT_TOML}:/etc/lan-iot/agent.toml:ro" \
    --bind "${LLM_TOML}:/etc/lan-iot/llm.toml:ro"

  start_one ui \
    --env "HUB_URL=http://127.0.0.1:3000" \
    --env "AGENT_URL=http://127.0.0.1:8000" \
    --env "NEXT_PUBLIC_HUB_URL=http://127.0.0.1:3000" \
    --env "NEXT_PUBLIC_AGENT_URL=http://127.0.0.1:8000" \
    --env "PORT=3001" \
    --env "HOSTNAME=0.0.0.0"

  if [[ "${LLM}" == "local" || "${LLM}" == "hybrid" ]]; then
    if [[ "${WITH_OLLAMA}" -eq 0 ]]; then
      warn "Ollama SIF not started - use host Ollama or re-run with --with-ollama"
    fi
  fi

  cat <<EOF

[deploy] instances started (shared host network - apps bind 0.0.0.0).

  Hub:         http://127.0.0.1:3000/api/v1/health
  Agent:       http://127.0.0.1:8000/health
  UI:          http://127.0.0.1:3001
  Mongo:       mongodb://127.0.0.1:27017  (db ${MONGODB_DATABASE})
  HA:          http://127.0.0.1:8123  (full profile)
  Mosquitto:   mqtt://127.0.0.1:1883 (full profile)
  Ollama:      http://127.0.0.1:11434 (--with-ollama)
EOF
}

curl_ok() {
  local url="$1"
  if have curl; then
    curl -fsS --max-time 5 "${url}" >/dev/null 2>&1
  else
    return 1
  fi
}

cmd_verify() {
  local fail=0
  log "verifying endpoints..."
  if curl_ok "http://127.0.0.1:3000/api/v1/health"; then
    log "PASS Hub health"
  else
    warn "FAIL Hub health (http://127.0.0.1:3000/api/v1/health)"
    fail=1
  fi
  if curl_ok "http://127.0.0.1:8000/health"; then
    log "PASS Agent health"
  else
    warn "FAIL Agent health"
    fail=1
  fi
  if [[ "${PROFILE_EFFECTIVE}" == "full" ]]; then
    if curl_ok "http://127.0.0.1:8123/"; then
      log "PASS HA HTTP"
    else
      warn "SKIP/FAIL HA (may still be booting)"
    fi
  fi
  if [[ "${fail}" -eq 0 ]]; then
    log "verify ok - optional: ../../scripts/smoke.sh"
    return 0
  fi
  if [[ "${STRICT}" -eq 1 ]]; then
    die "verify failed"
  fi
  warn "verify had failures (non-strict)"
  return 0
}

cmd_status() {
  if [[ -n "${SIF_BIN}" ]]; then
    log "instances (${SIF_BIN}):"
    "${SIF_BIN}" instance list || true
  else
    log "singularity/apptainer not installed - no instances"
  fi
  cmd_verify || true
}

cmd_up() {
  cmd_build
  cmd_start
  # Give services a moment before verify
  if [[ "${DRY_RUN}" -eq 0 ]]; then
    sleep 3
  fi
  cmd_verify
}

log "repo=${REPO_ROOT} sif_bin=${SIF_BIN:-none} cmd=${CMD}"
log "llm=${LLM} model=${MODEL} profile=${PROFILE}"

case "${CMD}" in
  build)  cmd_build ;;
  start)  cmd_start ;;
  stop)   cmd_stop ;;
  status) cmd_status ;;
  verify) cmd_verify ;;
  up)     cmd_up ;;
  *)      die "unknown command: ${CMD}" ;;
esac
