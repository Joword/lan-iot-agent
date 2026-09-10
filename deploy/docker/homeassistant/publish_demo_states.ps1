# Publish faker MQTT states for ESP32 + Gree + Xiaomi stubs.

# Run from deploy/docker (Compose project root for this stack).

#

# Prerequisites:

#   docker compose up -d mosquitto homeassistant

#   HA MQTT integration → broker host mosquitto:1883

#   Hub running with HA_URL + HA_TOKEN

#

# After publish, Hub should list (all is_faker=true):

#   light.demo_esp32_light

#   climate.demo_gree_ac

#   light.demo_xiaomi_bulb

#   switch.demo_xiaomi_plug



$ErrorActionPreference = "Stop"



function Pub([string]$Topic, [string]$Message) {

    Write-Host "→ $Topic = $Message"

    docker compose exec mosquitto mosquitto_pub -h localhost -t $Topic -m $Message

}



Write-Host "=== ESP32 Light [faker] ==="

Pub "home/demo/esp32_light/availability" "online"

Pub "home/demo/esp32_light/state" '{"state":"ON","brightness":180}'



Write-Host "=== Gree AC [faker] ==="

Pub "home/demo/gree_ac/availability" "online"

Pub "home/demo/gree_ac/mode/state" "cool"

Pub "home/demo/gree_ac/temp/state" "26"

Pub "home/demo/gree_ac/current_temp" "28.5"



Write-Host "=== Xiaomi Bulb [faker] ==="

Pub "home/demo/xiaomi_bulb/availability" "online"

Pub "home/demo/xiaomi_bulb/state" '{"state":"OFF","brightness":128}'



Write-Host "=== Xiaomi Plug [faker] ==="

Pub "home/demo/xiaomi_plug/availability" "online"

Pub "home/demo/xiaomi_plug/state" "ON"



Write-Host "=== ESP32 Temperature [faker] ==="

Pub "home/demo/esp32_sensor/availability" "online"

Pub "home/demo/esp32_sensor/state" '{"temperature":26.5}'



Write-Host ""

Write-Host "Done. Verify:"

Write-Host "  curl -s http://127.0.0.1:3000/api/v1/devices"

Write-Host "Expect HA MQTT entity_ids: demo_esp32_*, demo_gree_ac, demo_xiaomi_* (faker_* is Hub-internal)"


