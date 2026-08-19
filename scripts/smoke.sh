#!/usr/bin/env bash
# LanIoT Agent — stack smoke checks (Hub + Agent).
# Does not require Docker. Exit non-zero only when Hub is reachable but a Hub check fails.
#
# MongoDB is optional for smoke: we only curl Hub health / devices / scenes / … .
# Hub still boots without Mongo when AUTH_REQUIRED=false (default).
# Hub needs MongoDB when AUTH_REQUIRED=true (pairing tokens + protected routes).
#
# Usage:
#   ./scripts/smoke.sh
#   HUB_URL=http://127.0.0.1:3000 AGENT_URL=http://127.0.0.1:8000 ./scripts/smoke.sh

set -u

HUB_URL="${HUB_URL:-http://127.0.0.1:3000}"
AGENT_URL="${AGENT_URL:-http://127.0.0.1:8000}"
HUB_URL="${HUB_URL%/}"
AGENT_URL="${AGENT_URL%/}"
TIMEOUT="${SMOKE_TIMEOUT:-8}"

PASS=0
FAIL=0
SKIP=0
HUB_REACHABLE=0
HUB_FAILED=0
CURL_ERR=""
HTTP_CODE=0

have_jq=0
command -v jq >/dev/null 2>&1 && have_jq=1

tmpdir="${TMPDIR:-/tmp}"
errfile="${tmpdir}/lan_iot_smoke_curl_err.$$"

cleanup() { rm -f "$errfile"; }
trap cleanup EXIT

result() {
  local status="$1" name="$2" detail="${3:-}"
  case "$status" in
    PASS) PASS=$((PASS + 1)); printf '[PASS] %s\n' "$name" ;;
    FAIL) FAIL=$((FAIL + 1)); printf '[FAIL] %s\n' "$name" ;;
    SKIP) SKIP=$((SKIP + 1)); printf '[SKIP] %s\n' "$name" ;;
  esac
  if [[ -n "$detail" ]]; then
    printf '       %s\n' "$detail"
  fi
}

# Sets HTTP_CODE, CURL_ERR; prints body on stdout.
http_get() {
  local url="$1"
  HTTP_CODE=0
  CURL_ERR=""
  local out
  out="$(curl -sS -m "$TIMEOUT" -w '\n%{http_code}' "$url" 2>"$errfile" || true)"
  if [[ -z "$out" ]]; then
    CURL_ERR="$(cat "$errfile" 2>/dev/null || true)"
    HTTP_CODE=0
    return 1
  fi
  HTTP_CODE="${out##*$'\n'}"
  printf '%s' "${out%$'\n'*}"
}

http_post_json() {
  local url="$1" json="$2"
  HTTP_CODE=0
  CURL_ERR=""
  local out
  out="$(curl -sS -m "$TIMEOUT" -w '\n%{http_code}' \
    -H 'Content-Type: application/json' \
    -d "$json" "$url" 2>"$errfile" || true)"
  if [[ -z "$out" ]]; then
    CURL_ERR="$(cat "$errfile" 2>/dev/null || true)"
    HTTP_CODE=0
    return 1
  fi
  HTTP_CODE="${out##*$'\n'}"
  printf '%s' "${out%$'\n'*}"
}

hub_check() {
  local name="$1" path="$2"
  if [[ "$HUB_REACHABLE" -ne 1 ]]; then
    result SKIP "$name" "Hub unreachable"
    return
  fi
  local body
  body="$(http_get "${HUB_URL}${path}")" || true
  if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    HUB_FAILED=1
    result FAIL "$name" "HTTP ${HTTP_CODE}${CURL_ERR:+ — $CURL_ERR}"
    return
  fi

  if [[ "$have_jq" -ne 1 ]]; then
    result PASS "$name" "HTTP ${HTTP_CODE} (install jq for deeper checks)"
    return
  fi

  case "$path" in
    /api/v1/health)
      local st svc
      st="$(printf '%s' "$body" | jq -r '.status // empty')"
      svc="$(printf '%s' "$body" | jq -r '.service // empty')"
      if [[ "$st" != "ok" || "$svc" != "hub" ]]; then
        HUB_FAILED=1
        result FAIL "$name" "expected status=ok service=hub; got status=$st service=$svc"
      else
        result PASS "$name" "status=ok ha.configured=$(printf '%s' "$body" | jq -r '.ha.configured') ha.connection=$(printf '%s' "$body" | jq -r '.ha.connection') devices_cached=$(printf '%s' "$body" | jq -r '.devices_cached')"
      fi
      ;;
    /api/v1/devices)
      if ! printf '%s' "$body" | jq -e 'has("devices")' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing devices[]"
      else
        result PASS "$name" "count=$(printf '%s' "$body" | jq '.devices|length') ha_available=$(printf '%s' "$body" | jq -r '.ha_available')"
      fi
      ;;
    /api/v1/scenes)
      if ! printf '%s' "$body" | jq -e 'has("scenes")' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing scenes[]"
      else
        result PASS "$name" "count=$(printf '%s' "$body" | jq -r '.count') ids=$(printf '%s' "$body" | jq -r '[.scenes[].id]|join(",")')"
      fi
      ;;
    /api/v1/companions)
      if ! printf '%s' "$body" | jq -e 'has("companions")' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing companions[]"
      else
        result PASS "$name" "count=$(printf '%s' "$body" | jq -r '.count') ids=$(printf '%s' "$body" | jq -r '[.companions[].id]|join(",")')"
      fi
      ;;
    /mcp/tools)
      if ! printf '%s' "$body" | jq -e 'has("tools")' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing tools[]"
      else
        local names missing=""
        names="$(printf '%s' "$body" | jq -r '[.tools[].name]|join(",")')"
        for need in devices.list devices.describe scenes.run companion.command; do
          if [[ ",$names," != *",$need,"* ]]; then
            missing="${missing}${need},"
          fi
        done
        if [[ -n "$missing" ]]; then
          HUB_FAILED=1
          result FAIL "$name" "missing ${missing%,}; have=$names"
        else
          result PASS "$name" "tools=$(printf '%s' "$body" | jq '.tools|length') ($names)"
        fi
      fi
      ;;
    *)
      result PASS "$name" "HTTP ${HTTP_CODE}"
      ;;
  esac
}

hub_post_check() {
  local name="$1" path="$2" json="$3" kind="$4"
  if [[ "$HUB_REACHABLE" -ne 1 ]]; then
    result SKIP "$name" "Hub unreachable"
    return
  fi
  local body
  body="$(http_post_json "${HUB_URL}${path}" "$json")" || true
  if [[ "$HTTP_CODE" -eq 400 || "$HTTP_CODE" -eq 404 ]]; then
    result SKIP "$name" "HTTP ${HTTP_CODE} (entity/scene not seeded)"
    return
  fi
  if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    HUB_FAILED=1
    result FAIL "$name" "HTTP ${HTTP_CODE}${CURL_ERR:+ — $CURL_ERR}"
    return
  fi
  if [[ "$have_jq" -ne 1 ]]; then
    result PASS "$name" "HTTP ${HTTP_CODE} (install jq for deeper checks)"
    return
  fi
  case "$kind" in
    describe)
      if ! printf '%s' "$body" | jq -e '.data.capabilities // .capabilities' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing capabilities"
      else
        result PASS "$name" "entity=$(printf '%s' "$body" | jq -r '.data.entity_id // .entity_id') caps=$(printf '%s' "$body" | jq '.data.capabilities // .capabilities | length')"
      fi
      ;;
    action)
      local ok
      ok="$(printf '%s' "$body" | jq -r '.ok // empty')"
      if [[ "$ok" != "true" ]]; then
        HUB_FAILED=1
        result FAIL "$name" "expected ok=true"
      else
        result PASS "$name" "ok entity=$(printf '%s' "$body" | jq -r '.entity_id') action=$(printf '%s' "$body" | jq -r '.action')"
      fi
      ;;
    scene)
      if ! printf '%s' "$body" | jq -e 'has("steps")' >/dev/null 2>&1; then
        HUB_FAILED=1
        result FAIL "$name" "missing steps[]"
      else
        result PASS "$name" "ok=$(printf '%s' "$body" | jq -r '.ok') steps=$(printf '%s' "$body" | jq '.steps|length') failed=$(printf '%s' "$body" | jq '.failed|length') skipped=$(printf '%s' "$body" | jq -r '.skipped_count')"
      fi
      ;;
    *)
      result PASS "$name" "HTTP ${HTTP_CODE}"
      ;;
  esac
}

printf 'LanIoT smoke\n'
printf '  Hub:   %s\n' "$HUB_URL"
printf '  Agent: %s\n' "$AGENT_URL"
printf '\n'

_body="$(http_get "${HUB_URL}/api/v1/health")" || true
if [[ "$HTTP_CODE" -ge 200 && "$HTTP_CODE" -lt 300 ]]; then
  HUB_REACHABLE=1
else
  printf 'Hub not reachable — Hub checks SKIP; Agent checks still run if Agent is up.\n'
  printf '  (%s)\n\n' "${CURL_ERR:-HTTP $HTTP_CODE}"
fi

hub_check "Hub health" "/api/v1/health"
hub_check "Hub devices" "/api/v1/devices"
hub_check "Hub scenes" "/api/v1/scenes"
hub_check "Hub companions" "/api/v1/companions"
hub_check "Hub MCP tools" "/mcp/tools"
hub_post_check "Hub describe faker climate" "/mcp/call" '{"name":"devices.describe","arguments":{"entity_id":"climate.demo_gree_ac"}}' describe
hub_post_check "Hub faker light on" "/api/v1/devices/light.demo_esp32_light/actions" '{"action":"turn_on"}' action
hub_post_check "Hub scene sleep_mode" "/api/v1/scenes/sleep_mode/run" '{}' scene

printf '\n'
agent_body="$(http_get "${AGENT_URL}/health")" || true
if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
  result SKIP "Agent health" "${CURL_ERR:-HTTP $HTTP_CODE}"
  result SKIP "Agent chat list devices" "Agent unreachable"
  result SKIP "Agent chat sleep mode" "Agent unreachable"
else
  if [[ "$have_jq" -eq 1 ]]; then
    result PASS "Agent health" "status=$(printf '%s' "$agent_body" | jq -r '.status // empty') service=$(printf '%s' "$agent_body" | jq -r '.service // empty')"
  else
    result PASS "Agent health" "HTTP ${HTTP_CODE}"
  fi

  list_body="$(http_post_json "${AGENT_URL}/v1/chat" '{"message":"list devices","context":{"devices":[],"scenes":[]}}')" || true
  if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    result FAIL "Agent chat list devices" "${CURL_ERR:-HTTP $HTTP_CODE}"
  elif [[ "$have_jq" -eq 1 ]]; then
    reply="$(printf '%s' "$list_body" | jq -r '.reply // empty')"
    if [[ -z "$reply" ]]; then
      result FAIL "Agent chat list devices" "missing reply"
    else
      snip="$(printf '%s' "$reply" | tr '\n' ' ' | cut -c1-120)"
      result PASS "Agent chat list devices" "status=$(printf '%s' "$list_body" | jq -r '.status // empty') reply=$snip"
    fi
  else
    result PASS "Agent chat list devices" "HTTP ${HTTP_CODE}"
  fi

  sleep_body="$(http_post_json "${AGENT_URL}/v1/chat" '{"message":"sleep mode","context":{"devices":[],"scenes":[]}}')" || true
  if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    result FAIL "Agent chat sleep mode" "${CURL_ERR:-HTTP $HTTP_CODE}"
  elif [[ "$have_jq" -eq 1 ]]; then
    reply="$(printf '%s' "$sleep_body" | jq -r '.reply // empty')"
    if [[ -z "$reply" ]]; then
      result FAIL "Agent chat sleep mode" "missing reply"
    else
      snip="$(printf '%s' "$reply" | tr '\n' ' ' | cut -c1-120)"
      note="status=$(printf '%s' "$sleep_body" | jq -r '.status // empty') used_tools=$(printf '%s' "$sleep_body" | jq -r '.used_tools // empty') reply=$snip"
      [[ "$HUB_REACHABLE" -ne 1 ]] && note="$note (Hub down — tool failure expected)"
      result PASS "Agent chat sleep mode" "$note"
    fi
  else
    note="HTTP ${HTTP_CODE}"
    [[ "$HUB_REACHABLE" -ne 1 ]] && note="$note (Hub down — tool failure expected)"
    result PASS "Agent chat sleep mode" "$note"
  fi
fi

printf '\nSummary: PASS=%s FAIL=%s SKIP=%s\n' "$PASS" "$FAIL" "$SKIP"

if [[ "$HUB_REACHABLE" -ne 1 ]]; then
  printf 'Exit 0 — Hub down (Docker/services not required).\n'
  exit 0
fi

if [[ "$HUB_FAILED" -eq 1 ]]; then
  printf 'Exit 1 — Hub reachable but one or more Hub checks failed.\n'
  exit 1
fi

printf 'Exit 0 — Hub checks OK.\n'
exit 0
