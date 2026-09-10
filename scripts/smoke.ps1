# LanIoT Agent — stack smoke checks (Hub + Agent).
# Does not require Docker. Exit non-zero only when Hub is reachable but a Hub check fails.
#
# MongoDB is optional for smoke: we only curl Hub health / devices / scenes / … .
# Hub still boots without Mongo when AUTH_REQUIRED=false (default).
# Hub needs MongoDB when AUTH_REQUIRED=true (pairing tokens + protected routes).
#
# Usage (from repo root or anywhere):
#   .\scripts\smoke.ps1
#   .\scripts\smoke.ps1 -HubUrl http://127.0.0.1:3000 -AgentUrl http://127.0.0.1:8000

[CmdletBinding()]
param(
    [string]$HubUrl = $(if ($env:HUB_URL) { $env:HUB_URL.TrimEnd('/') } else { "http://127.0.0.1:3000" }),
    [string]$AgentUrl = $(if ($env:AGENT_URL) { $env:AGENT_URL.TrimEnd('/') } else { "http://127.0.0.1:8000" }),
    [int]$TimeoutSec = 8
)

$ErrorActionPreference = "Continue"
$HubUrl = $HubUrl.TrimEnd('/')
$AgentUrl = $AgentUrl.TrimEnd('/')

$script:HubReachable = $false
$script:HubFailed = $false
$script:Pass = 0
$script:Fail = 0
$script:Skip = 0

function Write-Result {
    param(
        [ValidateSet("PASS", "FAIL", "SKIP")]
        [string]$Status,
        [string]$Name,
        [string]$Detail = ""
    )
    switch ($Status) {
        "PASS" { $script:Pass++; Write-Host ("[PASS] {0}" -f $Name) -ForegroundColor Green }
        "FAIL" { $script:Fail++; Write-Host ("[FAIL] {0}" -f $Name) -ForegroundColor Red }
        "SKIP" { $script:Skip++; Write-Host ("[SKIP] {0}" -f $Name) -ForegroundColor Yellow }
    }
    if ($Detail) {
        Write-Host ("       {0}" -f $Detail)
    }
}

function Invoke-Http {
    param(
        [string]$Method = "GET",
        [string]$Uri,
        [string]$Body = $null,
        [string]$ContentType = "application/json"
    )
    try {
        $params = @{
            Method          = $Method
            Uri             = $Uri
            TimeoutSec      = $TimeoutSec
            UseBasicParsing = $true
        }
        if ($null -ne $Body) {
            $params.Body = $Body
            $params.ContentType = $ContentType
        }
        $resp = Invoke-WebRequest @params
        $text = $resp.Content
        $json = $null
        try { $json = $text | ConvertFrom-Json } catch { }
        return @{
            Ok         = $true
            StatusCode = [int]$resp.StatusCode
            Text       = $text
            Json       = $json
            Error      = $null
        }
    }
    catch {
        $status = $null
        $text = $null
        if ($_.Exception.Response) {
            try {
                $status = [int]$_.Exception.Response.StatusCode
                $stream = $_.Exception.Response.GetResponseStream()
                if ($stream) {
                    $reader = New-Object System.IO.StreamReader($stream)
                    $text = $reader.ReadToEnd()
                    $reader.Close()
                }
            }
            catch { }
        }
        return @{
            Ok         = $false
            StatusCode = $status
            Text       = $text
            Json       = $null
            Error      = $_.Exception.Message
        }
    }
}

function Test-HubGet {
    param(
        [string]$Name,
        [string]$Path,
        [scriptblock]$Validate
    )
    if (-not $script:HubReachable) {
        Write-Result SKIP $Name "Hub unreachable"
        return
    }
    $r = Invoke-Http -Uri ($HubUrl + $Path)
    if (-not $r.Ok -or $r.StatusCode -lt 200 -or $r.StatusCode -ge 300) {
        $script:HubFailed = $true
        $detail = if ($r.Error) { $r.Error } else { "HTTP $($r.StatusCode)" }
        Write-Result FAIL $Name $detail
        return
    }
    if (-not $r.Json) {
        $script:HubFailed = $true
        Write-Result FAIL $Name "non-JSON body"
        return
    }
    $msg = & $Validate $r
    if ($msg -is [string] -and $msg.StartsWith("FAIL:")) {
        $script:HubFailed = $true
        Write-Result FAIL $Name $msg.Substring(5).Trim()
    }
    else {
        Write-Result PASS $Name $(if ($msg) { $msg } else { "HTTP $($r.StatusCode)" })
    }
}

Write-Host "LanIoT smoke"
Write-Host "  Hub:   $HubUrl"
Write-Host "  Agent: $AgentUrl"
Write-Host ""

# --- Probe Hub ---
$probe = Invoke-Http -Uri ($HubUrl + "/api/v1/health")
if ($probe.Ok -and $probe.StatusCode -ge 200 -and $probe.StatusCode -lt 300) {
    $script:HubReachable = $true
}
else {
    Write-Host "Hub not reachable - Hub checks SKIP; Agent checks still run if Agent is up."
    Write-Host "  ($($probe.Error))"
    Write-Host ""
}

# --- Hub checks (count toward exit code when Hub reachable) ---
Test-HubGet "Hub health" "/api/v1/health" {
    param($r)
    if ($r.Json.status -eq "ok" -and $r.Json.service -eq "hub") {
        $mongo = $r.Json.mongodb
        $mongoPart = if ($null -eq $mongo) {
            "mongodb=(absent)"
        }
        elseif ($null -ne $mongo.database -and "$($mongo.database)".Length -gt 0) {
            "mongodb.ok=$($mongo.ok) mongodb.database=$($mongo.database)"
        }
        else {
            "mongodb.ok=$($mongo.ok)"
        }
        "status=ok ha.configured=$($r.Json.ha.configured) ha.connection=$($r.Json.ha.connection) $mongoPart devices_cached=$($r.Json.devices_cached)"
    }
    else {
        "FAIL: expected status=ok service=hub"
    }
}

Test-HubGet "Hub devices" "/api/v1/devices" {
    param($r)
    if ($null -eq $r.Json.devices) {
        "FAIL: missing devices[]"
    }
    else {
        $n = @($r.Json.devices).Count
        $warn = if ($r.Json.warning) { " warning=$($r.Json.warning)" } else { "" }
        "count=$n ha_available=$($r.Json.ha_available)$warn"
    }
}

Test-HubGet "Hub scenes" "/api/v1/scenes" {
    param($r)
    if ($null -eq $r.Json.scenes) {
        "FAIL: missing scenes[]"
    }
    else {
        $ids = @($r.Json.scenes | ForEach-Object { $_.id })
        "count=$($r.Json.count) ids=$($ids -join ',')"
    }
}

Test-HubGet "Hub companions" "/api/v1/companions" {
    param($r)
    if ($null -eq $r.Json.companions) {
        "FAIL: missing companions[]"
    }
    else {
        $ids = @($r.Json.companions | ForEach-Object { $_.id })
        "count=$($r.Json.count) ids=$($ids -join ',')"
    }
}

Test-HubGet "Hub MCP tools" "/mcp/tools" {
    param($r)
    if ($null -eq $r.Json.tools) {
        "FAIL: missing tools[]"
    }
    else {
        $names = @($r.Json.tools | ForEach-Object { $_.name })
        $need = @("devices.list", "devices.describe", "scenes.run", "companion.command")
        $missing = @($need | Where-Object { $_ -notin $names })
        if ($missing.Count -gt 0) {
            "FAIL: missing $($missing -join ','); have=$($names -join ',')"
        }
        else {
            "tools=$($names.Count) ($($names -join ', '))"
        }
    }
}

function Test-HubPost {
    param(
        [string]$Name,
        [string]$Path,
        [string]$Body,
        [scriptblock]$Validate,
        [switch]$SkipMissing
    )
    if (-not $script:HubReachable) {
        Write-Result SKIP $Name "Hub unreachable"
        return
    }
    $r = Invoke-Http -Method POST -Uri ($HubUrl + $Path) -Body $Body
    if ($SkipMissing -and $r.StatusCode -in 400, 404) {
        Write-Result SKIP $Name "HTTP $($r.StatusCode) (entity/scene not seeded)"
        return
    }
    if (-not $r.Ok -or $r.StatusCode -lt 200 -or $r.StatusCode -ge 300) {
        $script:HubFailed = $true
        $detail = if ($r.Error) { $r.Error } else { "HTTP $($r.StatusCode)" }
        Write-Result FAIL $Name $detail
        return
    }
    if (-not $r.Json) {
        $script:HubFailed = $true
        Write-Result FAIL $Name "non-JSON body"
        return
    }
    $msg = & $Validate $r
    if ($msg -is [string] -and $msg.StartsWith("FAIL:")) {
        $script:HubFailed = $true
        Write-Result FAIL $Name $msg.Substring(5).Trim()
    }
    else {
        Write-Result PASS $Name $(if ($msg) { $msg } else { "HTTP $($r.StatusCode)" })
    }
}

Test-HubPost "Hub describe faker climate" "/mcp/call" '{"name":"devices.describe","arguments":{"entity_id":"climate.faker_gree_ac"}}' {
    param($r)
    $doc = $r.Json.data
    if ($null -eq $doc) { $doc = $r.Json }
    $caps = $doc.capabilities
    if ($null -eq $caps) {
        "FAIL: missing capabilities"
    }
    else {
        "entity=$($doc.entity_id) caps=$(@($caps).Count)"
    }
} -SkipMissing

Test-HubPost "Hub faker light on" "/api/v1/devices/light.faker_esp32_light/actions" '{"action":"turn_on"}' {
    param($r)
    if ($r.Json.ok -eq $true) {
        "ok entity=$($r.Json.entity_id) action=$($r.Json.action)"
    }
    else {
        "FAIL: expected ok=true"
    }
} -SkipMissing

Test-HubPost "Hub scene sleep_mode" "/api/v1/scenes/sleep_mode/run" '{}' {
    param($r)
    if ($null -eq $r.Json.steps) {
        "FAIL: missing steps[]"
    }
    else {
        $n = @($r.Json.steps).Count
        $failed = @($r.Json.failed).Count
        "ok=$($r.Json.ok) steps=$n failed=$failed skipped=$($r.Json.skipped_count)"
    }
} -SkipMissing

# --- Agent checks (report always; do not gate exit on Agent alone) ---
Write-Host ""
$agentProbe = Invoke-Http -Uri ($AgentUrl + "/health")
$agentUp = $agentProbe.Ok -and $agentProbe.StatusCode -ge 200 -and $agentProbe.StatusCode -lt 300

if (-not $agentUp) {
    Write-Result SKIP "Agent health" $(if ($agentProbe.Error) { $agentProbe.Error } else { "HTTP $($agentProbe.StatusCode)" })
    Write-Result SKIP "Agent chat list devices" "Agent unreachable"
    Write-Result SKIP "Agent chat sleep mode" "Agent unreachable"
}
else {
    if ($agentProbe.Json -and $agentProbe.Json.status -eq "ok" -and $agentProbe.Json.service -eq "agent") {
        Write-Result PASS "Agent health" "status=ok service=agent"
    }
    elseif ($agentProbe.Json -and $agentProbe.Json.status -eq "ok") {
        Write-Result PASS "Agent health" "status=ok"
    }
    else {
        Write-Result PASS "Agent health" "HTTP $($agentProbe.StatusCode)"
    }

    $listBody = '{"message":"list devices","context":{"devices":[],"scenes":[]}}'
    $list = Invoke-Http -Method POST -Uri ($AgentUrl + "/v1/chat") -Body $listBody
    if (-not $list.Ok -or $list.StatusCode -lt 200 -or $list.StatusCode -ge 300) {
        Write-Result FAIL "Agent chat list devices" $(if ($list.Error) { $list.Error } else { "HTTP $($list.StatusCode)" })
    }
    elseif (-not $list.Json -or [string]::IsNullOrEmpty([string]$list.Json.reply)) {
        Write-Result FAIL "Agent chat list devices" "missing reply field"
    }
    else {
        $snip = [string]$list.Json.reply
        if ($snip.Length -gt 120) { $snip = $snip.Substring(0, 117) + "..." }
        Write-Result PASS "Agent chat list devices" "status=$($list.Json.status) reply=$snip"
    }

    # Sleep mode may fail Hub MCP tools when Hub is down — still PASS if Agent returns a reply.
    $sleepBody = '{"message":"sleep mode","context":{"devices":[],"scenes":[]}}'
    $sleep = Invoke-Http -Method POST -Uri ($AgentUrl + "/v1/chat") -Body $sleepBody
    if (-not $sleep.Ok -or $sleep.StatusCode -lt 200 -or $sleep.StatusCode -ge 300) {
        Write-Result FAIL "Agent chat sleep mode" $(if ($sleep.Error) { $sleep.Error } else { "HTTP $($sleep.StatusCode)" })
    }
    elseif (-not $sleep.Json -or [string]::IsNullOrEmpty([string]$sleep.Json.reply)) {
        Write-Result FAIL "Agent chat sleep mode" "missing reply field"
    }
    else {
        $snip = [string]$sleep.Json.reply
        if ($snip.Length -gt 120) { $snip = $snip.Substring(0, 117) + "..." }
        $note = "status=$($sleep.Json.status) used_tools=$($sleep.Json.used_tools) reply=$snip"
        if (-not $script:HubReachable) {
            $note += " (Hub down - tool failure expected)"
        }
        Write-Result PASS "Agent chat sleep mode" $note
    }
}

Write-Host ""
Write-Host ("Summary: PASS={0} FAIL={1} SKIP={2}" -f $script:Pass, $script:Fail, $script:Skip)

if (-not $script:HubReachable) {
    Write-Host "Exit 0 - Hub down (Docker/services not required)."
    exit 0
}

if ($script:HubFailed) {
    Write-Host "Exit 1 - Hub reachable but one or more Hub checks failed."
    exit 1
}

Write-Host "Exit 0 - Hub checks OK."
exit 0
