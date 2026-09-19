"""Tkinter desktop interface for the Emergency Simulator test-alert controller.

The UI owns no Android logic. It collects intent, calls :class:`EmergencySimulatorController` on a
worker thread so the window never blocks, and renders what comes back. That separation is what keeps
the GUI and the CLI doing provably the same thing.

The visual language is deliberately a laboratory tool: monospace, a status board, an append-only log.
It must not resemble a government emergency-alert application.
"""

from __future__ import annotations

import queue
import threading
import tkinter as tk
from tkinter import messagebox, ttk
from typing import List, Optional

from .controller import (
    DEFAULT_BODY,
    SERVICE_CATEGORY,
    EmergencySimulatorController,
    SafetyError,
    validate_body,
)
from .models import AlertState, Device, DeviceState

# -- palette -----------------------------------------------------------------

BG = "#12151a"
PANEL = "#191d24"
PANEL_ALT = "#1f242d"
BORDER = "#2c333f"
FG = "#d7dce4"
FG_DIM = "#8b95a5"
ACCENT = "#e8b23a"      # caution amber, for the test-alert action
OK = "#4caf72"
WARN = "#e0a648"
ERR = "#e05a4c"
INFO = "#5b9bd5"

FONT_MONO = ("Consolas", 9)
FONT_MONO_BOLD = ("Consolas", 9, "bold")

SAFETY_STRIP = "TEST ONLY      CONTROLLED DEVICE      NO CELLULAR TRANSMISSION"


class EmergencySimulatorUI:
    """The main application window."""

    def __init__(self, root: tk.Tk, controller: Optional[EmergencySimulatorController] = None):
        self.root = root
        self.root.title("EMERGENCY-SIMULATOR  -  Android Alert Lab")
        self.root.configure(bg=BG)
        self.root.geometry("880x760")
        self.root.minsize(760, 620)

        self._events: "queue.Queue[tuple]" = queue.Queue()
        self._cancel = threading.Event()
        self._busy = False
        self._devices: List[Device] = []
        self._selected: Optional[Device] = None

        self.controller = controller or EmergencySimulatorController(
            cancel_check=self._cancel.is_set,
            on_log=lambda m: self._events.put(("log", m)),
        )

        self._build()
        self.root.after(80, self._pump)
        self.root.after(200, self.on_refresh)

    # -- construction ------------------------------------------------------

    def _build(self) -> None:
        outer = tk.Frame(self.root, bg=BG)
        outer.pack(fill="both", expand=True, padx=14, pady=12)

        self._build_header(outer)
        self._build_devices(outer)
        self._build_alert_panel(outer)
        self._build_status(outer)
        self._build_log(outer)

    def _build_header(self, parent: tk.Frame) -> None:
        head = tk.Frame(parent, bg=BG)
        head.pack(fill="x")

        tk.Label(head, text="EMERGENCY-SIMULATOR", bg=BG, fg=FG,
                 font=("Consolas", 17, "bold")).pack(anchor="w")
        tk.Label(head, text="ANDROID ALERT LAB", bg=BG, fg=FG_DIM,
                 font=("Consolas", 9)).pack(anchor="w")

        strip = tk.Frame(head, bg=PANEL_ALT, highlightbackground=BORDER, highlightthickness=1)
        strip.pack(fill="x", pady=(8, 0))
        tk.Label(strip, text=SAFETY_STRIP, bg=PANEL_ALT, fg=ACCENT,
                 font=("Consolas", 9, "bold")).pack(pady=5)

    def _panel(self, parent: tk.Frame, title: str) -> tk.Frame:
        frame = tk.Frame(parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        frame.pack(fill="x", pady=(10, 0))
        tk.Label(frame, text=title, bg=PANEL, fg=FG_DIM,
                 font=("Consolas", 8, "bold")).pack(anchor="w", padx=10, pady=(7, 2))
        body = tk.Frame(frame, bg=PANEL)
        body.pack(fill="x", padx=10, pady=(0, 9))
        return body

    def _build_devices(self, parent: tk.Frame) -> None:
        body = self._panel(parent, "CONNECTED DEVICES")

        self.device_list = tk.Listbox(
            body, height=4, bg=PANEL_ALT, fg=FG, font=FONT_MONO,
            selectbackground="#2f4360", selectforeground=FG,
            highlightthickness=0, borderwidth=0, activestyle="none",
        )
        self.device_list.pack(fill="x")
        self.device_list.bind("<<ListboxSelect>>", self._on_device_selected)

        row = tk.Frame(body, bg=PANEL)
        row.pack(fill="x", pady=(7, 0))
        self.btn_refresh = self._button(row, "Refresh", self.on_refresh)
        self.btn_refresh.pack(side="left")
        self.btn_inspect = self._button(row, "Select Device", self.on_select_device)
        self.btn_inspect.pack(side="left", padx=(7, 0))

    def _build_alert_panel(self, parent: tk.Frame) -> None:
        body = self._panel(parent, "TEST ALERT")

        grid = tk.Frame(body, bg=PANEL)
        grid.pack(fill="x")

        def labelled(row: int, label: str, value: str, fg: str = FG) -> None:
            tk.Label(grid, text=label, bg=PANEL, fg=FG_DIM, font=FONT_MONO,
                     width=10, anchor="w").grid(row=row, column=0, sticky="w", pady=1)
            tk.Label(grid, text=value, bg=PANEL, fg=fg, font=FONT_MONO,
                     anchor="w").grid(row=row, column=1, sticky="w", pady=1)

        labelled(0, "Type:", "ETWS TEST", ACCENT)
        labelled(1, "Channel:", f"{SERVICE_CATEGORY} (0x1103)  [ LOCKED ]", FG_DIM)

        tk.Label(grid, text="Message:", bg=PANEL, fg=FG_DIM, font=FONT_MONO,
                 width=10, anchor="w").grid(row=2, column=0, sticky="nw", pady=(6, 1))
        self.msg_var = tk.StringVar(value=DEFAULT_BODY)
        entry = tk.Entry(grid, textvariable=self.msg_var, bg=PANEL_ALT, fg=FG,
                         insertbackground=FG, font=FONT_MONO, relief="flat",
                         highlightbackground=BORDER, highlightcolor=ACCENT, highlightthickness=1)
        entry.grid(row=2, column=1, sticky="ew", pady=(6, 1), ipady=4)
        grid.columnconfigure(1, weight=1)

        tk.Label(body, text="The message must begin with TEST. The channel cannot be changed.",
                 bg=PANEL, fg=FG_DIM, font=("Consolas", 8)).pack(anchor="w", pady=(6, 0))

        actions = tk.Frame(body, bg=PANEL)
        actions.pack(fill="x", pady=(9, 0))
        self.btn_dry = self._button(actions, "Dry Run", self.on_dry_run)
        self.btn_dry.pack(side="left")
        self.btn_send = self._button(actions, "SEND TEST ALERT", self.on_send, primary=True)
        self.btn_send.pack(side="left", padx=(7, 0))
        self.btn_stop = self._button(actions, "STOP / CANCEL", self.on_stop, danger=True)
        self.btn_stop.pack(side="right")

    def _build_status(self, parent: tk.Frame) -> None:
        body = self._panel(parent, "STATUS")
        grid = tk.Frame(body, bg=PANEL)
        grid.pack(fill="x")

        self.status_labels = {}
        rows = [
            ("root", "Root:"),
            ("test_mode", "Test mode:"),
            ("injector", "Injector:"),
            ("last", "Last result:"),
        ]
        for i, (key, label) in enumerate(rows):
            tk.Label(grid, text=label, bg=PANEL, fg=FG_DIM, font=FONT_MONO,
                     width=14, anchor="w").grid(row=i, column=0, sticky="w")
            value = tk.Label(grid, text="---", bg=PANEL, fg=FG, font=FONT_MONO, anchor="w")
            value.grid(row=i, column=1, sticky="w")
            self.status_labels[key] = value

    def _build_log(self, parent: tk.Frame) -> None:
        frame = tk.Frame(parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        frame.pack(fill="both", expand=True, pady=(10, 0))
        tk.Label(frame, text="LOG", bg=PANEL, fg=FG_DIM,
                 font=("Consolas", 8, "bold")).pack(anchor="w", padx=10, pady=(7, 2))

        self.log_text = tk.Text(
            frame, bg=PANEL_ALT, fg=FG, font=FONT_MONO, relief="flat",
            highlightthickness=0, borderwidth=0, height=12, wrap="word",
            insertbackground=FG, state="disabled",
        )
        self.log_text.pack(fill="both", expand=True, padx=10, pady=(0, 9))
        self.log_text.tag_configure("ok", foreground=OK)
        self.log_text.tag_configure("err", foreground=ERR)
        self.log_text.tag_configure("warn", foreground=WARN)
        self.log_text.tag_configure("dim", foreground=FG_DIM)

    def _button(self, parent, text: str, command, primary: bool = False,
                danger: bool = False) -> tk.Button:
        if primary:
            bg, fg, active = ACCENT, "#12151a", "#f0c65a"
        elif danger:
            bg, fg, active = PANEL_ALT, ERR, "#2a3038"
        else:
            bg, fg, active = PANEL_ALT, FG, "#2a3038"
        btn = tk.Button(parent, text=text, command=command, bg=bg, fg=fg,
                        activebackground=active, activeforeground=fg, font=FONT_MONO_BOLD,
                        relief="flat", borderwidth=0, highlightthickness=0, padx=14, pady=6,
                        cursor="hand2")
        return btn

    # -- logging -----------------------------------------------------------

    def append_log(self, message: str, tag: str = "") -> None:
        self.log_text.configure(state="normal")
        prefix = {
            "ok": "> ", "err": "x ", "warn": "! ",
        }.get(tag, "> ")
        self.log_text.insert("end", f"{prefix}{message}\n", tag or "")
        self.log_text.see("end")
        self.log_text.configure(state="disabled")

    # -- worker plumbing ---------------------------------------------------

    def _pump(self) -> None:
        """Drain worker-thread messages onto the Tk main loop."""
        try:
            while True:
                kind, payload = self._events.get_nowait()
                if kind == "log":
                    self.append_log(str(payload))
                elif kind == "log_tagged":
                    text, tag = payload
                    self.append_log(text, tag)
                elif kind == "devices":
                    self._render_devices(payload)
                elif kind == "result":
                    self._render_result(payload)
                elif kind == "failure":
                    self._render_failure(payload)
                elif kind == "done":
                    self._set_busy(False)
        except queue.Empty:
            pass
        self.root.after(80, self._pump)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        state = "disabled" if busy else "normal"
        for btn in (self.btn_refresh, self.btn_inspect, self.btn_send, self.btn_dry):
            btn.configure(state=state)
        self.btn_stop.configure(state="normal" if busy else "disabled")

    def _run_async(self, fn) -> None:
        if self._busy:
            return
        self._cancel.clear()
        self._set_busy(True)
        thread = threading.Thread(target=fn, daemon=True)
        thread.start()

    def _worker(self, fn) -> None:
        """Run `fn` off the UI thread; always re-enable the buttons afterwards."""
        try:
            fn()
        except SafetyError as exc:
            self._events.put(("failure", str(exc)))
        except Exception as exc:  # never let a worker kill the app silently
            self._events.put(("failure", f"{type(exc).__name__}: {exc}"))
        finally:
            self._events.put(("done", None))

    # -- actions -----------------------------------------------------------

    def on_refresh(self) -> None:
        def work() -> None:
            self._events.put(("log", "Scanning for attached devices"))
            try:
                devices = self.controller.discover()
            except Exception as exc:
                self._events.put(("failure", str(exc)))
                return
            self._events.put(("devices", devices))
        self._run_async(lambda: self._worker(work))

    def _on_device_selected(self, _event=None) -> None:
        sel = self.device_list.curselection()
        if sel:
            self._selected = self._devices[sel[0]]

    def on_select_device(self) -> None:
        if self._selected is None:
            self.append_log("Select a device from the list first.", "warn")
            return
        dev = self._selected
        self.append_log(f"Selected {dev.serial} ({dev.model or 'unknown model'})")
        self.status_labels["root"].configure(
            text="YES" if dev.is_root else "NO", fg=OK if dev.is_root else ERR)
        if dev.cellbroadcast_package is None:
            self.status_labels["injector"].configure(text="NO CELLBROADCAST", fg=ERR)
        if not dev.is_root:
            self.append_log(
                "Root is required: the emergency broadcast is a protected broadcast and the "
                "framework rejects a non-root sender.", "err")

    def _require_usable_device(self) -> Optional[Device]:
        dev = self._selected
        if dev is None:
            messagebox.showwarning(
                "No device selected",
                "Select a device from the list, then choose 'Select Device'.",
                parent=self.root,
            )
            return None
        if not dev.is_usable:
            reason = {
                DeviceState.NO_ROOT: "The device is not rooted.",
                DeviceState.UNSUPPORTED: "No CellBroadcast receiver package was found.",
                DeviceState.OFFLINE: "The device is offline.",
                DeviceState.UNAUTHORIZED: "The device is unauthorized; accept the USB prompt.",
                DeviceState.BUSY: "The device is busy (still booting?).",
            }.get(dev.state, "The device is not ready.")
            messagebox.showerror("Device not ready", reason, parent=self.root)
            return None
        return dev

    def _validated_body(self) -> Optional[str]:
        try:
            return validate_body(self.msg_var.get())
        except SafetyError as exc:
            messagebox.showerror("Message rejected", str(exc), parent=self.root)
            self.append_log(str(exc), "err")
            return None

    def on_dry_run(self) -> None:
        dev = self._require_usable_device()
        if dev is None:
            return
        body = self._validated_body()
        if body is None:
            return
        self.append_log("Dry run: checking everything, sending nothing")

        def work() -> None:
            result = self.controller.send_test_alert(dev, body=body, dry_run=True)
            self._events.put(("result", result))
        self._run_async(lambda: self._worker(work))

    def on_send(self) -> None:
        dev = self._require_usable_device()
        if dev is None:
            return
        body = self._validated_body()
        if body is None:
            return

        confirmed = messagebox.askyesno(
            "Send test alert?",
            "WARNING\n\n"
            "This will trigger a TEST emergency alert on the selected rooted device.\n\n"
            f"Target:   {dev.serial}\n"
            f"Model:    {dev.model or 'unknown'}\n"
            f"Android:  {dev.release or '?'} / API {dev.sdk or '?'}\n"
            f"Type:     ETWS TEST\n"
            f"Channel:  {SERVICE_CATEGORY} (locked)\n"
            f"Message:  {body}\n\n"
            "No cellular transmission occurs.",
            parent=self.root,
            default="no",
            icon="warning",
        )
        if not confirmed:
            self.append_log("Cancelled at confirmation. Nothing was sent.", "warn")
            return

        self.append_log(f"Sending ETWS TEST alert to {dev.serial}")

        def work() -> None:
            result = self.controller.send_test_alert(dev, body=body, dry_run=False)
            self._events.put(("result", result))
        self._run_async(lambda: self._worker(work))

    def on_stop(self) -> None:
        if not self._busy:
            self.append_log("Nothing is in progress.")
            return
        self._cancel.set()
        self.append_log("STOP requested: cancelling the pending operation.", "warn")
        self.append_log(
            "If an alert has already been delivered, Android does not permit remote dismissal.",
            "warn")
        self.append_log(
            "Dismiss it with the alert's own on-device control.", "warn")

    def on_close(self) -> None:
        self._cancel.set()
        self.root.destroy()

    # -- rendering ---------------------------------------------------------

    def _render_devices(self, devices: List[Device]) -> None:
        self._devices = devices
        self.device_list.delete(0, "end")
        for dev in devices:
            mark = "*" if dev.is_root else "."
            android = f"Android {dev.release}" if dev.release else "unknown"
            state = dev.state.value
            self.device_list.insert(
                "end", f" {mark} {dev.serial:<18} {android:<14} {state}")
        if not devices:
            self.append_log("No devices attached. Start an emulator or connect a device.", "warn")
            self.status_labels["root"].configure(text="---", fg=FG_DIM)
            return

        usable = [d for d in devices if d.is_usable]
        self.append_log(f"Found {len(devices)} device(s); {len(usable)} ready", "ok" if usable else "warn")
        if usable:
            self.device_list.selection_clear(0, "end")
            self.device_list.selection_set(0)
            self._selected = usable[0]
            self.on_select_device()

    def _render_result(self, result) -> None:
        if result.state is AlertState.READY_TO_SEND:
            self.status_labels["last"].configure(text="DRY RUN OK", fg=INFO)
            self.status_labels["test_mode"].configure(text="READY", fg=OK)
            self.append_log("Dry run complete. Every check passed; nothing was sent.", "ok")
            return

        if result.state is AlertState.ALERT_DISPLAYED:
            self.status_labels["last"].configure(text="ALERT DISPLAYED", fg=OK)
            self.append_log("SUCCESS: the genuine Android emergency alert was displayed.", "ok")
            for line in result.evidence:
                self.append_log(f"  {line}", "ok")
            self.append_log("Dismiss it with the alert's own on-device control.", "warn")
            self.append_log("Remote dismissal is not supported by Android.", "warn")
            return

        if result.state is AlertState.CANCELLED:
            self.status_labels["last"].configure(text="CANCELLED", fg=WARN)
            self.append_log("Cancelled. Nothing was delivered.", "warn")
            return

        code = result.failure.value if result.failure else "UNKNOWN"
        self.status_labels["last"].configure(text=code, fg=ERR)
        self.append_log(f"FAILED: {code}", "err")
        if result.message:
            self.append_log(f"  {result.message}", "err")
        for line in result.evidence:
            self.append_log(f"  {line}", "dim")
        if result.injector_exit_code is not None:
            self.append_log(f"  injector exit code: {result.injector_exit_code}", "dim")
        if result.injector_stderr.strip():
            for line in result.injector_stderr.strip().splitlines()[-4:]:
                self.append_log(f"  injector: {line}", "dim")
        if result.failure and result.failure.value in ("CELLBROADCAST_FILTERED", "TEST_MODE_DISABLED"):
            self.append_log(
                "  The message reached the receiver but was filtered. Test mode may be disabled "
                "on the device.", "warn")

    def _render_failure(self, message: str) -> None:
        self.append_log(message, "err")
        self.status_labels["last"].configure(text="ERROR", fg=ERR)


def main() -> int:
    from .logsetup import configure_logging

    log_file = configure_logging()
    root = tk.Tk()
    try:
        ui = EmergencySimulatorUI(root)
    except SafetyError as exc:
        root.destroy()
        import sys
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        root.destroy()
        import sys
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    ui.append_log(f"Session log: {log_file}", "dim")
    if ui.controller.adb_error:
        ui.append_log(ui.controller.adb_error, "err")
    root.protocol("WM_DELETE_WINDOW", ui.on_close)
    root.mainloop()
    return 0


if __name__ == "__main__":
    import sys

    sys.exit(main())