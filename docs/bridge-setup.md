# Connecting your Windows laptop to the analysis environment

## Why a connection is needed at all

The analysis environment (where the agent runs) is a **Linux container**. It has no USB, no access to
your laptop's filesystem, and no route outward to your machine. Every network path is **inbound-only**:
your laptop can reach the container, but the container cannot reach your laptop.

The Galaxy A35 is attached to your Windows laptop. So the container cannot see the phone, cannot run
`adb`, and cannot launch the Windows executable. No tooling on the container side can change this.

That leaves exactly one workable topology, and this directory records it:

```
        YOUR WINDOWS LAPTOP                (you run things here)
                |
        adb --->|---> Samsung Galaxy A35   (stock, read-only)
                |
        HTTPS --|------------------------>  Analysis container (agent)
                |                            https://<HOST>  (per session)
```

The laptop collects raw output and posts it to the container. The container analyses it and hands
back the next batch of commands. This is the whole transport.

---

## Step 1 — Confirm the container is reachable

The bridge is already running. From your laptop:

```powershell
Invoke-RestMethod <HOST>/health
```

`<HOST>` is the URL the agent gives you for the current session. It changes whenever the analysis
environment is restarted, and the old one stops answering, so always take the current one from the
agent rather than reusing a URL from an earlier session or from this document.

Expected:

```json
{ "ok": true, "service": "emergency-simulator-bridge", "note": "..." }
```

If this fails, tell the agent and stop. Nothing else will work.

---

## Step 2 — Set the token in your PowerShell session

Every endpoint except `/health` needs a bearer token. The agent will give you the current token; it is
never committed to the repository.

```powershell
$base  = "<HOST>"
$token = "<TOKEN FROM THE AGENT>"
```

---

## Step 3 — Get the task batch

```powershell
Invoke-RestMethod "$base/task" -Headers @{ Authorization = "Bearer $token" }
```

This returns batch **A35-RO-001**: ten read-only probes. Each task has a `title`, a `reason`, a list
of `commands`, and an `evidence_name` to post the output under.

**All ten are read-only.** None writes a setting, installs anything, clears data, remounts a
partition, or touches a personal file. The batch deliberately excludes `am start`, `am broadcast`,
`settings put`, `pm install` and anything involving root, the bootloader or `/system`.

---

## Step 4 — Check the phone is connected and authorized

```powershell
adb devices -l
```

You want exactly one line, ending in `device`:

```
<serial>   device   product:a35x... model:SM_A356B device:a35x ...
```

If it says `unauthorized`, unlock the phone screen and accept the "Allow USB debugging?" prompt, then
run it again. If it lists nothing, check the USB cable and that USB debugging is on.

If you do not have `adb` on your laptop, the built `Emergency-Simulator.exe` carries its own copy, or
download Platform Tools from Google:
<https://developer.android.com/tools/releases/platform-tools>

---

## Step 5 — Collect and post each task

For each task, run its commands and post the combined raw output under the task's `evidence_name`.
Do not edit, trim or summarise the output. If a command errors, post the error verbatim — the error
is often the answer.

A helper that makes this mechanical:

```powershell
function Send-Evidence {
    param([string]$Name, [string[]]$Commands)
    $sb = [System.Text.StringBuilder]::new()
    foreach ($c in $Commands) {
        [void]$sb.AppendLine("### $c")
        [void]$sb.AppendLine((Invoke-Expression $c 2>&1 | Out-String))
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
    $r = Invoke-RestMethod -Method Post -Uri "$base/evidence" `
            -Headers @{ Authorization = "Bearer $token"; 'X-Evidence-Name' = $Name } `
            -Body $bytes
    Write-Host "posted $($r.stored) ($($r.bytes) bytes)"
}
```

Then, for example:

```powershell
Send-Evidence "a35-01-identity.txt" @(
  "adb devices -l",
  "adb shell getprop ro.product.model",
  "adb shell getprop ro.build.version.release",
  "adb shell getprop ro.build.type"
)
```

Run that for all ten `evidence_name` values in the batch. The names must match exactly:
`a35-01-identity.txt` through `a35-10-adb-capability.txt`.

---

## Step 6 — Tell the agent it is done

Once all ten are posted, say so. The agent will read the evidence, and either:

* author a **second batch** based on what the first revealed, or
* report the legitimate boundary, if no non-privileged path exists.

---

## Rules that will not be relaxed

These apply to every batch, and the agent will not work around them:

* **The A35 stays stock.** No root, no bootloader unlock, no flashing, no custom recovery, no ROM, no
  firmware modification, no partition remount, no privileged APK, no replacing Android or Samsung
  components.
* **Read-only first.** Inspect, observe, document. A command that could change phone state is not run
  without you explicitly approving that specific command.
* **ADB is transport, not privilege escalation.** "The command was accepted" is not "Android
  authorised it". The two are different and the difference is the whole investigation.
* **A blocked test is a success.** "Blocked by Samsung permissions" and "the interface does not exist
  on stock firmware" are valid, useful results. They are not failures to work around.
* **No cellular transmission, at any point, by anyone.** No carrier impersonation, no RF, no operator
  network injection.
* **Delivery is judged from downstream evidence**, never from an exit code. If
  `CellBroadcastReceiver` / `CellBroadcastAlertService` / `CellBroadcastAlertDialog` cannot be seen in
  logcat, the result is reported as `UNVERIFIED` or `UNCERTAIN` — not as success.

---

## If you would rather not expose a port

The bridge is a convenience, not a requirement. The alternative is pure copy-and-paste: the agent
gives you commands, you paste the output back into the chat, and the agent records it. It is slower
and manual, but it needs no listener and no token. Say so if you prefer that.