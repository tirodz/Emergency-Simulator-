# One paste into PowerShell

Copy everything in the block below into PowerShell and press Enter. It connects your laptop to the
analysis environment, finds `adb`, confirms the Galaxy A35, pulls the current read-only task batch,
runs it, and posts the raw output back. It prints progress as it goes.

**It changes nothing on the phone.** It runs only the commands in the batch, every batch authored so
far is read-only, and it finishes by telling you how to confirm that.

```powershell
# --- Emergency-Simulator: connect this laptop to the analysis environment ---
# @@BEGIN-CONFIG@@
# This committed file lives in a public repository, so it carries no live host or token: the host
# changes every session and the token is a per-session secret. The real values are rendered from the
# gitignored bridge-token.txt by tools/make-paste.py into docs/connect-one-paste.local.md.
$base  = "<HOST>"
$token = "<TOKEN>"
# @@END-CONFIG@@

# Wrapped so an early stop does not close your PowerShell window.
& {
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

      # $PSScriptRoot is empty when a block is pasted into the console instead of run from a .ps1
      # file, and Join-Path rejects an empty base. Every Join-Path below is also guarded so one bad
      # candidate cannot abort the search.
      $here = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }

      # Beside the built executable, wherever it happens to be.
      $candidates += (Join-Path $here "platform-tools\adb.exe")
      $candidates += (Join-Path $here "dist\platform-tools\adb.exe")
      $candidates += (Join-Path (Join-Path $here "..") "packaging\platform-tools\adb.exe")
      $candidates += (Join-Path (Join-Path $here "..") "dist\platform-tools\adb.exe")

      # An SDK install, if there is one.
      foreach ($root in @($env:LOCALAPPDATA, $env:ANDROID_HOME, $env:ANDROID_SDK_ROOT)) {
          if ($root) {
              try { $candidates += (Join-Path $root "Android\Sdk\platform-tools\adb.exe") } catch { }
              try { $candidates += (Join-Path $root "platform-tools\adb.exe") } catch { }
          }
      }

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
      # Do not abort. Some batches are not about adb at all (the Bluetooth one is not), and the
      # adb-using tasks report their own failures. Aborting here would silently skip them.
      Warn "  adb was not found on PATH."
      Warn "    - If this batch needs adb, point ADB at it below, or install Platform Tools:"
      Warn "      https://developer.android.com/tools/releases/platform-tools"
      Warn "    - Batches that do not use adb are unaffected and will still run."
  } else {
      Good "  adb: $script:AdbPath"
      & $script:AdbPath version | Select-Object -First 1
  }

  # So the task commands can call `adb` unchanged.
  function adb { & $script:AdbPath @args }

  # ---------------------------------------------------------------------------
  # 3. Confirm exactly one authorized device
  # ---------------------------------------------------------------------------

  Say ""
  Say "== 3. Device =="
  $devices = @((& $script:AdbPath devices) -split "`n" | Where-Object { $_ -match "\tdevice$" })
  if (-not $devices) {
      Warn "  No authorized device."
      & $script:AdbPath devices -l
      Warn "  If it says 'unauthorized': unlock the phone and accept the USB debugging prompt."
      Warn "  If nothing is listed: check the cable and that USB debugging is on."
      Warn "  Continuing anyway - tasks that do not use adb still run."
  } elseif ($devices.Count -gt 1) {
      Warn "  More than one device attached. Disconnect the others so results are unambiguous."
      $devices | ForEach-Object { Warn "    $_" }
      Warn "  Continuing anyway - tasks that do not use adb still run."
  } else {
      Good "  device: $($devices[0].Split("`t")[0])"
      # Recorded for step 5. Without this assignment step 5 tested an empty variable and always
      # reported "no phone attached", so the did-anything-change check never actually ran.
      $script:Device = $devices[0].Split("`t")[0]
  }

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
  # 5. Did anything change
  # ---------------------------------------------------------------------------

  Say ""
  Say "== 5. Did anything change? =="
  if ($script:Device) {
      Say "  This batch was authored read-only. Confirm the phone's settings are unchanged:"
      Say "    adb shell settings list global | Select-String -Pattern 'emergency|cellbroadcast'"
      Say "    (compare against the settings task from this same batch)"
  } else {
      Say "  No phone attached, so there is nothing phone-side to compare."
      Say "  This batch was authored read-only: it reads state and writes nothing to any device."
  }
}

```

---

## What you should see

```
== 1. Reaching the analysis environment ==
  reachable: emergency-simulator-bridge
== 2. Locating adb ==
  adb: C:\...\adb.exe
Android Debug Bridge version 1.0.41
== 3. Device ==
  device: RXXXXXXXXXX
== 4. Task batch ==
  batch : A35-RO-001
  tasks : 10
  why   : Establish the stock Galaxy A35's Cell Broadcast surface using read-only inspection only.

  [1] Device identity and build
        read-only
        posted a35-01-identity.txt (1234 bytes)
  ...
== done: posted 10 of 10 ==
```

Then tell the agent the batch is posted.

---

## If something fails

The script stops early and says why, rather than continuing with bad inputs:

* **`reachable: ...` never prints** — the port is down or the URL is wrong. Tell the agent.
* **`adb was not found`** — run the paste from the folder containing `Emergency-Simulator.exe` (it
  bundles Platform Tools), or install
  [Platform Tools](https://developer.android.com/tools/releases/platform-tools).
* **`No authorized device`** — unlock the phone and accept the "Allow USB debugging?" prompt, then
  run `adb devices -l` and check it shows one line ending in `device`.
* **`More than one device attached`** — unplug the others, so results cannot be attributed to the
  wrong phone.

Per-task post failures do not stop the run; the summary reports how many of the ten were posted.

---

## What it does under the hood

Ten read-only probes. None of them writes a setting, installs anything, clears data, remounts a
partition, or touches a personal file. Specifically **absent** on purpose: `am start`, `am broadcast`,
`settings put`, `pm install`, `su`, and anything touching `/system`, `/vendor`, `/product` or `/data`.

| # | Establishes |
| --- | --- |
| 1 | Identity and build type |
| 2 | Security state (verified boot, flash-locked, carrier/CSC) |
| 3 | Which packages own alerts on this phone |
| 4 | Exported components, protecting permissions, signature |
| 5 | Whether the module is an APEX |
| 6 | Emergency permission protection levels |
| 7 | Live service and carrier config state |
| 8 | Current per-category user settings |
| 9 | Whether any test/diagnostic component exists |
| 10 | What the adb shell is actually allowed to do |

Task 10 ends with `adb shell su -c id`, which is **expected to fail** on a stock phone. Its failure is
the finding: it confirms no root shell is available. Do not try to obtain root.

---

## Verified vs not verified

**Verified from the analysis environment:**

* The bridge answers on the public URL, confirmed by an independent external fetch (not a self-check).
* A missing or wrong token is rejected with 401.
* Posted evidence round-trips byte-for-byte, and an evidence name attempting traversal is refused.
* The task batch parses, and its filters are PowerShell-native (`Select-String`, not `findstr`).

**Not verified — and this matters:**

* **The PowerShell script in this document has never been executed.** The analysis environment is
  Linux and has no PowerShell, so the paste is written and hand-checked (guards, brace and paren
  balance, quoting) but not run. If it errors on its first line, that is a defect in the script, not
  in your laptop — send the error text and it will be fixed.
* The device-discovery, adb-location and evidence-posting paths have only been exercised against the
  bridge's own API from Linux, never from a real Windows shell with a real phone attached.

---

## The rules this project holds to

* The A35 stays stock: no root, no bootloader unlock, no flashing, no custom recovery or ROM, no
  privileged APK, no replaced Android or Samsung components.
* Read-only first. Anything that could change phone state needs your explicit approval for that
  specific command.
* ADB is transport, not privilege escalation. "The command was accepted" is not "Android authorised
  it" — and that difference is the investigation.
* A blocked test is a result, not a failure to work around.
* No cellular transmission, ever: no carrier impersonation, no RF, no operator network injection.
* Delivery is judged from downstream evidence
  (`CellBroadcastReceiver` / `CellBroadcastAlertService` / `CellBroadcastAlertDialog`), never from an
  exit code.
