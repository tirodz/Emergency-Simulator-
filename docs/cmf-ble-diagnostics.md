# CMF Watch Pro 2 — Windows host / Bluetooth diagnostic paste

Read this alongside `docs/connect-one-paste.md`. This one is for the **CMF watch** project, which is a
different transport from the phone work: Windows → Bluetooth/BLE → CMF Watch Pro 2, not Windows → adb
→ Android.

The rendered block (with this session's host and token) is produced by:

```bash
python3 tools/make-paste.py --host https://<HOST> --task-file bridge-tasks-cmf-ble-001.json
```

Everything in batch `CMF-BLE-001` is read-only. No connection to the watch, no pairing change, no
driver change, no Bluetooth reset, no write of any kind.

## What each probe establishes

| # | Establishes | Layer |
| --- | --- | --- |
| 1 | Which machine this actually is | A |
| 2 | Whether Windows has a Bluetooth adapter, and whether it is enabled | D |
| 3 | Whether the Bluetooth support service is running | C |
| 4 | What Windows has paired and enumerated | F |
| 5 | Whether Python and `bleak` are present on the laptop | G/H |
| 6 | A passive BLE discovery scan | D/E/G |
| 7 | Whether the CMF Watch Pro 2 specifically appears | E |
| 8 | Whether the previous working tooling is still on disk | — |
| 9 | What the session can reach, and what long-running processes exist | B/I |
| 10 | Where `adb` is (separate project, kept separate on purpose) | — |

## Why this exists

A previous session on this watch project could reach the operator's Windows machine and its
Bluetooth adapter. The same capability is **not** available in the current analysis environment:
it is a Linux container with no Bluetooth hardware, no PowerShell, no Windows filesystem, and no
outbound route to the laptop.

This batch is how the boundary is established with evidence rather than assumption. It separates:

* **A** — the container cannot reach the real Windows machine
* **B** — no PowerShell bridge/session to that machine
* **C/D** — Windows Bluetooth stack or adapter unavailable
* **E** — the watch is not advertising
* **F** — pairing state problem
* **G/H** — scanner tooling missing or permission-blocked
* **I** — something else

The failure is reported at the layer that actually fails, with the raw output as proof.