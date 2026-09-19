# Emergency-Simulator — Windows laptop bootstrap
#
# Paste this whole file into PowerShell. It connects your laptop to the analysis environment,
# finds adb, confirms the phone, pulls the current task batch, runs it, and posts the raw output.
#
# It is INSPECTION ONLY. It runs whatever commands the task batch lists, and every batch authored so
# far is read-only. Nothing here uses `su`, modifies the device, or bypasses a permission.
#
# Usage:
#   $base  = "https://<your-work-host>.prod-runtime.all-hands.dev"
#   $token = "<token from the agent>"
#   .\connect.ps1                      # or paste the body
#
# Or paste the one-liner block from docs/connect-one-paste.md, which sets $base and $token first.

param(
    [string]$Base  = $base,
    [string]$Token = $token
)

$ErrorActionPreference = "Stop"

if (-not $Base)  { throw "Set `$base to the analysis host URL first." }
if (-not $Token) { throw "Set `$token to the bearer token from the agent first." }

$Base = $Base.TrimEnd('/')
$headers = @{ Authorization = "Bearer $Token" }

function Say  { param($m) Write-Host $m }
function Warn { param($m) Write-Host $m -ForegroundColor Yellow }
function Good { param($m) Write-Host $m -ForegroundColor Green }

# ---------------------------------------------------------------------------
# 1. Reachability
# ---------------------------------------------------------------------------

Say ""
Say "== 1. Reaching the analysis environment =="
try {
    $health = Invoke-RestMethod "$Base/health" -TimeoutSec 25
    Good "  reachable: $($health.service)"
} catch {
    Warn "  FAILED to reach $Base"
    Warn "  $($_.Exception.Message)"
    Warn "  Stop here and report this to the agent. Nothing else will work."
    return
}

# ---------------------------------------------------------------------------
# 2. Locate adb
# ---------------------------------------------------------------------------
# The built Emergency-Simulator.exe carries its own Platform Tools, so check the usual places
# before giving up. This does not install anything.

Say ""
Say "== 2. Locating adb =="

function Find-Adb {
    $candidates = @()
    $c = Get-Command adb -ErrorAction SilentlyContinue
    if ($c) { $candidates += $c.Source }

    # Beside the built executable, wherever it happens to be.
    # $PSScriptRoot is empty when this block is pasted into the console rather than run from a .ps1
    # file, and Join-Path rejects an empty base. Guard every Join-Path so one bad candidate cannot
    # abort the search.
    $here = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }

    $candidates += (Join-Path $here "platform-tools\adb.exe")
    $candidates += (Join-Path $here "dist\platform-tools\adb.exe")
    $candidates += (Join-Path (Join-Path $here "..") "packaging\platform-tools\adb.exe")
    $candidates += (Join-Path (Join-Path $here "..") "dist\platform-tools\adb.exe")

    # An SDK install, if there is one.
    if ($env:LOCALAPPDATA) {
        $candidates += (Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools\adb.exe")
    }
    if ($env:ANDROID_HOME)    { $candidates += (Join-Path $env:ANDROID_HOME    "platform-tools\adb.exe") }
    if ($env:ANDROID_SDK_ROOT) { $candidates += (Join-Path $env:ANDROID_SDK_ROOT "platform-tools\adb.exe") }

    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) {
            # Present is not enough: adb.exe fails at load time without its companion DLLs, so run it.
            try {
                $null = & $p version 2>&1
                if ($LASTEXITCODE -eq 0) { return (Resolve-Path $p).Path }
            } catch { }
        }
    }
    return $null
}

$script:AdbPath = Find-Adb
if (-not $script:AdbPath) {
    Warn "  adb was not found."
    Warn "  Options:"
    Warn "    - Use the built Emergency-Simulator.exe, which bundles it, and run this from its folder."
    Warn "    - Or install Platform Tools: https://developer.android.com/tools/releases/platform-tools"
    return
}
Good "  adb: $script:AdbPath"
& $script:AdbPath version | Select-Object -First 1

# So the task commands can call `adb` unchanged.
function adb { & $script:AdbPath @args }

# ---------------------------------------------------------------------------
# 3. Confirm exactly one authorized device
# ---------------------------------------------------------------------------

Say ""
Say "== 3. Device =="
# @() so a single device stays an array: Where-Object returns a scalar for one match, and
# $devices[0] would then index a character instead of the element.
$devices = @((& $script:AdbPath devices) -split "`n" | Where-Object { $_ -match "\tdevice$" })
if (-not $devices) {
    Warn "  No authorized device."
    & $script:AdbPath devices -l
    Warn "  If it says 'unauthorized': unlock the phone and accept the USB debugging prompt."
    Warn "  If nothing is listed: check the cable and that USB debugging is on."
    return
}
if ($devices.Count -gt 1) {
    Warn "  More than one device attached. Disconnect the others so results are unambiguous."
    $devices | ForEach-Object { Warn "    $_" }
    return
}
Good "  device: $($devices[0].Split("`t")[0])"

# ---------------------------------------------------------------------------
# 4. Pull the batch and run it
# ---------------------------------------------------------------------------

Say ""
Say "== 4. Task batch =="
$batch = Invoke-RestMethod "$Base/task" -Headers $headers -TimeoutSec 25
Say "  batch : $($batch.batch)"
Say "  tasks : $($batch.tasks.Count)"
if ($batch.purpose) { Say "  why   : $($batch.purpose)" }

function Send-Evidence {
    param([string]$Name, [string[]]$Commands)
    $sb = [System.Text.StringBuilder]::new()
    foreach ($c in $Commands) {
        [void]$sb.AppendLine("### $c")
        try {
            $out = Invoke-Expression $c 2>&1 | Out-String
        } catch {
            $out = "EXCEPTION: $($_.Exception.Message)"
        }
        [void]$sb.AppendLine($out.TrimEnd())
        [void]$sb.AppendLine("")
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
    $r = Invoke-RestMethod -Method Post -Uri "$Base/evidence" -TimeoutSec 60 `
            -Headers @{ Authorization = "Bearer $Token"; 'X-Evidence-Name' = $Name } `
            -Body $bytes -ContentType 'application/octet-stream'
    return $r
}

$posted = 0
foreach ($t in $batch.tasks) {
    $name = $t.evidence_name
    Say ""
    Say ("  [{0}] {1}" -f $t.id, $t.title)
    if ($t.read_only) { Say "        read-only" }
    try {
        $r = Send-Evidence -Name $name -Commands $t.commands
        $posted++
        Good ("        posted {0} ({1} bytes)" -f $r.stored, $r.bytes)
    } catch {
        Warn ("        FAILED to post {0}: {1}" -f $name, $_.Exception.Message)
    }
}

Say ""
Good "== done: posted $posted of $($batch.tasks.Count) =="
Say "Tell the agent the batch is posted. Do not summarise the output; it was sent raw."

# ---------------------------------------------------------------------------
# 5. Confirm the phone was left untouched
# ---------------------------------------------------------------------------

Say ""
Say "== 5. Confirm nothing changed on the phone =="
Say "  adb shell settings list global | Select-String -Pattern 'emergency|cellbroadcast'"
Say "  (compare against task 8 from this same batch: the values should be identical)"