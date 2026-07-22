#!/usr/bin/env bash

# Publish faker MQTT states for ESP32 + Gree + Xiaomi stubs.

# Run from deploy/docker (Compose project root for this stack).

#

# After publish, Hub should list (all is_faker=true):

#   light.demo_esp32_light

#   climate.demo_gree_ac

#   light.faker_xiaomi_bulb

#   switch.faker_xiaomi_plug



set -euo pipefail



pub() {

  local topic="$1"

  local message="$2"

  echo "→ ${topic} = ${message}"

  docker compose exec mosquitto mosquitto_pub -h localhost -t "$topic" -m "$message"

}



echo "=== ESP32 Light [faker] ==="

pub "home/demo/esp32_light/availability" "online"

pub "home/demo/esp32_light/state" '{"state":"ON","brightness":180}'



echo "=== Gree AC [faker] ==="

pub "home/demo/gree_ac/availability" "online"

pub "home/demo/gree_ac/mode/state" "cool"

pub "home/demo/gree_ac/temp/state" "26"

pub "home/demo/gree_ac/current_temp" "28.5"



echo "=== Xiaomi Bulb [faker] ==="

pub "home/faker/xiaomi_bulb/availability" "online"

pub "home/faker/xiaomi_bulb/state" '{"state":"OFF","brightness":128}'



echo "=== Xiaomi Plug [faker] ==="

pub "home/faker/xiaomi_plug/availability" "online"

pub "home/faker/xiaomi_plug/state" "ON"



echo "=== ESP32 Temperature [faker] ==="

pub "home/demo/esp32_sensor/availability" "online"

pub "home/demo/esp32_sensor/state" '{"temperature":26.5}'



echo

echo "Done. Verify:"

echo "  curl -s http://127.0.0.1:3000/api/v1/devices"

echo "Expect faker entity_ids: demo_esp32_*, demo_gree_ac, faker_xiaomi_*"


