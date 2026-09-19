"""Tkinter desktop interface for the Emergency Simulator test-alert controller.

The interface owns no Android logic. It collects intent, calls
:class:`~app.controller.EmergencySimulatorController` on a worker thread so the window never blocks,
and renders what comes back. That separation is what keeps the GUI and the CLI doing provably the
same thing: both call the same methods, so the safety rules cannot diverge between them.

The visual language is deliberately a laboratory instrument rather than an emergency-alert screen:
monospace type, a status board, an append-only log, and a caution amber for the one consequential
action. It must be impossible to mistake this window for a real alert, so nothing here borrows the
severity styling of one.
"""

from __future__ import annotations

import queue
import threading
import tkinter as tk
from tkinter import messagebox
from typing import List, Optional

from . import __version__
from .controller import (
    DEFAULT_BODY,
    SERVICE_CATEGORY,
    EmergencySimulatorController,
    SafetyError,
    validate_body,
)
from .models import AlertState, Device, SupportLevel, TransactionState
from .widgets import (
    ACCENT,
    BG,
    BORDER,
    Banner,
    Button,
    Card,
    DeviceRow,
    ERR,
    FG,
    FG_DIM,
    FG_FAINT,
    INFO,
    LogView,
    OK,
    PANEL,
    PANEL_ALT,
    StatusPill,
    WARN,
    glyph,
    mono,
    state_colour,
)

FONT_MONO = mono(9)
FONT_MONO_BOLD = mono(9, bold=True)

SAFETY_STRIP = "TEST ONLY      CONTROLLED DEVICE      NO CELLULAR TRANSMISSION"

#: Extra explanatory line shown under a result whose failure code benefits from it.
_FAILURE_HINTS = {
    "TEST_MODE_DISABLED": (
        "Test alerts are disabled on the device and could not be enabled. "
        "The message would be dropped as 'ignoring alert by user preference'."
    ),
    "CELLBROADCAST_FILTERED": (
        "The message reached the receiver but was filtered. Check the device's test-mode settings."
    ),
    "DUPLICATE_SEND_BLOCKED": (
        "An earlier alert may still be on screen. Android queues alerts and cannot withdraw one, "
        "so a new send would stack a second dialog."
    ),
    "NO_ROOT": (
        "Root is required: the emergency broadcast is a protected broadcast and the framework "
        "rejects a non-root sender."
    ),
    "TIMEOUT": (
        "A timeout is not proof that nothing was delivered. Check the device screen."
    ),
}


class EmergencySimulatorUI:
    """The main application window."""

    def __init__(self, root: tk.Tk, controller: Optional[EmergencySimulatorController] = None):
        self.root = root
        self.root.title(f"EMERGENCY-SIMULATOR  \u2014  Android Alert Lab  [{__version__}]")
        self.root.configure(bg=BG)
        self.root.geometry("960x820")
        self.root.minsize(820, 660)

        self._events: "queue.Queue[tuple]" = queue.Queue()
        self._cancel = threading.Event()
        self._busy = False
        self._devices: List[Device] = []
        self._selected: Optional[Device] = None
        self._rows: dict = {}
        self._last_result = None

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
        outer.pack(fill="both", expand=True, padx=16, pady=14)

        self._build_header(outer)

        middle = tk.Frame(outer, bg=BG)
        middle.pack(fill="both", expand=True, pady=(12, 0))
        middle.columnconfigure(0, weight=3, uniform="col")
        middle.columnconfigure(1, weight=2, uniform="col")

        self._build_devices(middle)
        self._build_alert(middle)
        self._build_log(outer)

    def _build_header(self, parent: tk.Frame) -> None:
        head = tk.Frame(parent, bg=BG)
        head.pack(fill="x")

        left = tk.Frame(head, bg=BG)
        left.pack(side="left", anchor="w")
        tk.Label(left, text="EMERGENCY-SIMULATOR", bg=BG, fg=FG,
                 font=mono(16, bold=True)).pack(anchor="w")
        tk.Label(left, text="ANDROID ALERT LAB", bg=BG, fg=FG_FAINT,
                 font=mono(8, bold=True)).pack(anchor="w", pady=(1, 0))

        right = tk.Frame(head, bg=BG)
        right.pack(side="right", anchor="e")
        self.adb_pill = StatusPill(right, "ADB")
        self.adb_pill.pack(anchor="e")

        self.safety_banner = Banner(head, SAFETY_STRIP, fg=ACCENT)
        self.safety_banner.pack(fill="x", pady=(11, 0))

        # A separate strip for outcomes. The safety statement above is permanent and must never be
        # replaced by a result, because it is the one line that always has to be true.
        self.outcome_banner = Banner(head, "", fg=FG)
        self._outcome_shown = False

    def _build_devices(self, parent: tk.Frame) -> None:
        card = Card(parent, "Devices")
        card.grid(row=0, column=0, sticky="nsew", padx=(0, 8))

        self.device_list = tk.Frame(card.body, bg=PANEL)
        self.device_list.pack(fill="both", expand=True)
        self._empty_label = tk.Label(
            self.device_list,
            text="Scanning for attached devices\u2026",
            bg=PANEL,
            fg=FG_FAINT,
            font=mono(9),
            pady=18,
        )
        self._empty_label.pack(fill="x")

        actions = tk.Frame(card.body, bg=PANEL)
        actions.pack(fill="x", pady=(9, 0))
        self.btn_refresh = Button(actions, "Refresh", self.on_refresh, icon="bullet")
        self.btn_refresh.pack(side="left")
        self.btn_ack = Button(actions, "Acknowledge", self.on_acknowledge, variant="ghost")
        self.btn_ack.pack(side="left", padx=(7, 0))
        self.btn_ack.set_enabled(False)

    def _build_alert(self, parent: tk.Frame) -> None:
        card = Card(parent, "Test alert")
        card.grid(row=0, column=1, sticky="nsew", padx=(8, 0))
        body = card.body

        target_row = tk.Frame(body, bg=PANEL)
        target_row.pack(fill="x")
        tk.Label(target_row, text="TARGET", bg=PANEL, fg=FG_FAINT,
                 font=mono(8, bold=True)).pack(anchor="w")
        self.target_value = tk.Label(
            target_row, text="none selected", bg=PANEL, fg=FG_DIM, font=mono(10, bold=True)
        )
        self.target_value.pack(anchor="w", pady=(1, 0))
        self.target_verdict = tk.Label(
            target_row, text="", bg=PANEL, fg=FG_FAINT, font=mono(8),
            wraplength=340, justify="left",
        )
        self.target_verdict.pack(anchor="w", pady=(2, 0))

        tk.Frame(body, bg=BORDER, height=1).pack(fill="x", pady=10)

        meta = tk.Frame(body, bg=PANEL)
        meta.pack(fill="x")
        for i, (label, value) in enumerate(
            (("TYPE", "ETWS TEST"), ("CHANNEL", f"{SERVICE_CATEGORY} (0x1103)  LOCKED"))
        ):
            tk.Label(meta, text=label, bg=PANEL, fg=FG_FAINT,
                     font=mono(8, bold=True)).grid(row=0, column=i, sticky="w", padx=(0, 22))
            tk.Label(meta, text=value, bg=PANEL, fg=ACCENT if i == 0 else FG_DIM,
                     font=mono(9, bold=True)).grid(row=1, column=i, sticky="w", padx=(0, 22))

        tk.Label(body, text="MESSAGE", bg=PANEL, fg=FG_FAINT,
                 font=mono(8, bold=True)).pack(anchor="w", pady=(12, 3))
        self.msg_var = tk.StringVar(value=DEFAULT_BODY)
        self.msg_entry = tk.Entry(
            body,
            textvariable=self.msg_var,
            bg=PANEL_ALT,
            fg=FG,
            insertbackground=FG,
            font=mono(10),
            relief="flat",
            highlightbackground=BORDER,
            highlightcolor=ACCENT,
            highlightthickness=1,
        )
        self.msg_entry.pack(fill="x", ipady=6)
        self.msg_var.trace_add("write", lambda *_: self._on_message_changed())

        self.msg_feedback = tk.Label(
            body, text="Must begin with TEST.", bg=PANEL, fg=FG_FAINT,
            font=mono(8), anchor="w", justify="left", wraplength=340,
        )
        self.msg_feedback.pack(fill="x", pady=(4, 0))

        tk.Frame(body, bg=BORDER, height=1).pack(fill="x", pady=12)

        self.btn_send = Button(body, "SEND TEST ALERT", self.on_send,
                               variant="primary", icon="alert")
        self.btn_send.pack(fill="x")
        self.btn_send.set_enabled(False)

        secondary = tk.Frame(body, bg=PANEL)
        secondary.pack(fill="x", pady=(7, 0))
        self.btn_dry = Button(secondary, "Dry Run", self.on_dry_run, variant="ghost")
        self.btn_dry.pack(side="left", fill="x", expand=True)
        self.btn_stop = Button(secondary, "STOP / CANCEL", self.on_stop,
                               variant="danger", icon="cancel")
        self.btn_stop.pack(side="left", fill="x", expand=True, padx=(7, 0))
        self.btn_stop.set_enabled(False)

    def _build_log(self, parent: tk.Frame) -> None:
        wrapper = tk.Frame(parent, bg=BG)
        wrapper.pack(fill="both", expand=True, pady=(12, 0))

        head = tk.Frame(wrapper, bg=BG)
        head.pack(fill="x")
        tk.Label(head, text="SESSION LOG", bg=BG, fg=FG_FAINT,
                 font=mono(8, bold=True)).pack(side="left")
        self.result_pill = StatusPill(head, "LAST RESULT")
        self.result_pill.pack(side="right")

        self.log_view = LogView(wrapper, height=11)
        self.log_view.pack(fill="both", expand=True, pady=(5, 0))
        # Kept under the previous name: the CLI and the tests both refer to it.
        self.log_text = self.log_view.text

    # -- logging -----------------------------------------------------------

    def append_log(self, message: str, tag: str = "") -> None:
        self.log_view.append(message, tag)

    # -- worker plumbing ---------------------------------------------------

    def _pump(self) -> None:
        """Drain worker-thread messages onto the Tk main loop."""
        try:
            while True:
                kind, payload = self._events.get_nowait()
                if kind == "log":
                    self.append_log(str(payload))
                elif kind == "devices":
                    self._render_devices(payload)
                elif kind == "result":
                    self._render_result(payload)
                elif kind == "failure":
                    self._render_failure(payload)
                elif kind == "ackdone":
                    self._after_acknowledge(payload)
                elif kind == "done":
                    self._set_busy(False)
        except queue.Empty:
            pass
        self.root.after(80, self._pump)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        for btn in (self.btn_refresh, self.btn_ack):
            btn.set_enabled(not busy)
        self.btn_stop.set_enabled(busy)
        # SEND and Dry Run depend on the selected device as well as on busy state, so let the target
        # panel decide rather than duplicating the rule here.
        self._update_target_panel()

    def _run_async(self, fn) -> None:
        if self._busy:
            return
        self._cancel.clear()
        self._set_busy(True)
        threading.Thread(target=fn, daemon=True).start()

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

    # -- gate --------------------------------------------------------------

    def _gate_clear(self) -> bool:
        if self._selected is None:
            return False
        return self.controller.transaction_state(self._selected.serial) is TransactionState.READY

    def _refresh_ack_button(self) -> None:
        if self._selected is None or self._busy:
            self.btn_ack.set_enabled(False)
            return
        self.btn_ack.set_enabled(
            self.controller.transaction_state(self._selected.serial) is not TransactionState.READY
        )

    def _show_gate_state(self) -> None:
        if self._selected is None:
            return
        state = self.controller.transaction_state(self._selected.serial)
        if state is TransactionState.READY:
            return
        colour = state_colour(state.value)
        self.target_verdict.configure(
            text=f"{glyph('warn')} {state.value}: dismiss any alert on the device, then "
                 f"Acknowledge before sending again.",
            fg=colour,
        )

    def on_acknowledge(self) -> None:
        dev = self._selected
        if dev is None:
            self.append_log("Select a device first.", "warn")
            return
        state = self.controller.transaction_state(dev.serial)
        if state is TransactionState.READY:
            self.append_log(f"{dev.serial} has no outstanding alert.", "info")
            return
        if not messagebox.askyesno(
            "Acknowledge outstanding alert?",
            f"Only continue if you have dismissed the alert on {dev.serial}.\n\n"
            "Acknowledging clears the block on sending another test alert. Android queues alerts "
            "and cannot withdraw one, so acknowledging too early will stack a second dialog.",
            parent=self.root,
            default="no",
            icon="warning",
        ):
            self.append_log("Acknowledge cancelled.", "info")
            return
        self.controller.acknowledge(dev.serial)
        self._events.put(("ackdone", dev.serial))

    def _after_acknowledge(self, serial: str) -> None:
        self.append_log(f"Acknowledged {serial}: the send gate is clear.", "ok")
        self._set_busy(self._busy)
        self._update_target_panel()

    # -- message validation ------------------------------------------------

    def _on_message_changed(self) -> None:
        raw = self.msg_var.get()
        if not raw.strip():
            self.msg_feedback.configure(text="Must begin with TEST.", fg=FG_FAINT)
            return
        try:
            validate_body(raw)
        except SafetyError as exc:
            self.msg_feedback.configure(text=f"{glyph('cross')} {exc}", fg=ERR)
        else:
            self.msg_feedback.configure(
                text=f"{glyph('check')} accepted \u2014 the alert will be labelled as a test",
                fg=OK,
            )

    def _validated_body(self) -> Optional[str]:
        try:
            return validate_body(self.msg_var.get())
        except SafetyError as exc:
            messagebox.showerror("Message rejected", str(exc), parent=self.root)
            self.append_log(str(exc), "err")
            return None

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

    def _on_device_selected(self, serial: str) -> None:
        for dev in self._devices:
            if dev.serial == serial:
                self._selected = dev
                break
        for key, row in self._rows.items():
            row.set_selected(key == serial)
        self._update_target_panel()

    def _can_send(self) -> bool:
        """SEND requires both a usable device and a clear gate. Either alone is not enough."""
        if self._busy or self._selected is None:
            return False
        return self._selected.is_usable and self._gate_clear()

    def _update_target_panel(self) -> None:
        dev = self._selected
        if dev is None:
            self.target_value.configure(text="none selected", fg=FG_DIM)
            self.target_verdict.configure(text="")
            self.btn_send.set_enabled(False)
            return

        level = dev.support_level
        self.target_value.configure(text=dev.serial, fg=FG)
        # The verdict is always a word first, so it reads without relying on colour.
        self.target_verdict.configure(text=f"{level.value} \u2014 {dev.support_reason}", fg=FG_DIM)
        self._show_gate_state()
        self._refresh_ack_button()
        self.btn_send.set_enabled(self._can_send())
        self.btn_dry.set_enabled(not self._busy and level is not SupportLevel.UNSUPPORTED)

    def on_dry_run(self) -> None:
        dev = self._selected
        if dev is None:
            messagebox.showwarning("No device selected", "Select a device first.", parent=self.root)
            return
        body = self._validated_body()
        if body is None:
            return
        self.append_log(f"Dry run against {dev.serial}: checking everything, sending nothing")

        def work() -> None:
            result = self.controller.send_test_alert(dev, body=body, dry_run=True)
            self._events.put(("result", result))

        self._run_async(lambda: self._worker(work))

    def on_send(self) -> None:
        dev = self._selected
        if dev is None:
            messagebox.showwarning("No device selected", "Select a device first.", parent=self.root)
            return
        if not dev.is_usable:
            messagebox.showerror(
                "Device not ready",
                f"{dev.support_level.value}\n\n{dev.support_reason}\n\n"
                + "\n".join(f"\u00b7 {n}" for n in dev.notes),
                parent=self.root,
            )
            return
        body = self._validated_body()
        if body is None:
            return

        gate = self.controller.transaction_state(dev.serial)
        if gate is not TransactionState.READY:
            messagebox.showwarning(
                "Outstanding alert",
                f"{dev.serial} is {gate.value}.\n\n"
                f"{self.controller.gate_explanation(dev.serial)}",
                parent=self.root,
            )
            return

        if not messagebox.askyesno(
            "Send test alert?",
            "WARNING\n\n"
            "This will trigger a TEST emergency alert on the selected rooted device.\n"
            "The device will produce sound, vibration and a full-screen alert.\n\n"
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
        ):
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
        self.append_log("Dismiss it with the alert's own on-device control.", "warn")

    def on_close(self) -> None:
        self._cancel.set()
        self.root.destroy()

    # -- rendering ---------------------------------------------------------

    def _render_devices(self, devices: List[Device]) -> None:
        self._devices = devices
        for row in self._rows.values():
            row.destroy()
        self._rows = {}
        self._empty_label.pack_forget()

        if not devices:
            self._empty_label.configure(
                text="No devices attached.\nStart an emulator, or connect a device with USB "
                     "debugging enabled."
            )
            self._empty_label.pack(fill="x")
            self.append_log("No devices attached.", "warn")
            return

        for dev in devices:
            level = dev.support_level
            mark = {
                "SUPPORTED": "ready",
                "ROOT_REQUIRED": "warn",
                "UNSUPPORTED": "cross",
            }.get(level.value, "unknown")
            row = DeviceRow(self.device_list, self._on_device_selected)
            row.update_row(
                serial=dev.serial,
                name=dev.model or dev.serial,
                meta=f"Android {dev.release or '?'}  API {dev.sdk or '?'}"
                     + (f"  {dev.build_type}" if dev.build_type else ""),
                verdict=level.value,
                colour=state_colour(level.value),
                mark=mark,
            )
            row.pack(fill="x", pady=(0, 5))
            self._rows[dev.serial] = row

        usable = [d for d in devices if d.is_usable]
        self.append_log(
            f"Found {len(devices)} device(s); {len(usable)} ready",
            "ok" if usable else "warn",
        )

        target = usable[0] if usable else devices[0]
        self._on_device_selected(target.serial)

    def _show_outcome(self, text: str, fg: str, bg: str, mark: str = "warn") -> None:
        """Announce a result on the outcome strip, leaving the safety statement untouched.

        The safety statement above it is permanent: it is the one line that must always be true, so
        no result is ever allowed to overwrite it.
        """
        self.outcome_banner.set(f"{glyph(mark)}  {text}", fg=fg, bg=bg)
        if not self._outcome_shown:
            self.outcome_banner.pack(fill="x", pady=(6, 0))
            self._outcome_shown = True

    def _render_result(self, result) -> None:
        self._last_result = result

        if result.state is AlertState.READY_TO_SEND:
            self.result_pill.set("DRY RUN OK", INFO, "check")
            self._show_outcome(
                "DRY RUN PASSED \u2014 NOTHING WAS SENT", INFO, PANEL_ALT, mark="check"
            )
            self.append_log("Dry run complete. Every check passed; nothing was sent.", "ok")
            self._update_target_panel()
            return

        if result.state is AlertState.ALERT_DISPLAYED:
            self.result_pill.set("ALERT DISPLAYED", OK, "check")
            self._show_outcome(
                f"GENUINE ALERT DISPLAYED ON {result.device_serial} "
                f"\u2014  DISMISS IT ON THE DEVICE",
                fg="#101318",
                bg=OK,
                mark="check",
            )
            self.append_log("SUCCESS: the genuine Android emergency alert was displayed.", "ok")
            for line in result.evidence:
                self.append_log(f"  {line}", "ok")
            self.append_log("Dismiss it with the alert's own on-device control.", "warn")
            self.append_log("Remote dismissal is not supported by Android.", "warn")
            self._update_target_panel()
            return

        if result.state is AlertState.CANCELLED:
            self.result_pill.set("CANCELLED", WARN, "warn")
            self._show_outcome("CANCELLED \u2014 NOTHING WAS DELIVERED", WARN, PANEL_ALT)
            self.append_log("Cancelled. Nothing was delivered.", "warn")
            self._update_target_panel()
            return

        code = result.failure.value if result.failure else "UNKNOWN"
        self.result_pill.set(code, ERR, "cross")
        self._show_outcome(f"FAILED \u2014 {code}", ERR, PANEL_ALT, mark="cross")
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
        hint = _FAILURE_HINTS.get(code)
        if hint:
            self.append_log(f"  {hint}", "warn")
        self._update_target_panel()

    def _render_failure(self, message: str) -> None:
        self.append_log(message, "err")
        self.result_pill.set("ERROR", ERR, "cross")
        self._show_outcome(message.splitlines()[0], ERR, PANEL_ALT)


def main() -> int:
    import sys

    from .logsetup import configure_logging
    from .runtime import adb_setup_hint, locate_adb

    log_file = configure_logging()
    root = tk.Tk()
    root.withdraw()
    try:
        ui = EmergencySimulatorUI(root)
    except SafetyError as exc:
        root.destroy()
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        root.destroy()
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    ui.append_log(f"Session log: {log_file}", "dim")

    candidate = locate_adb()
    if candidate.works:
        how = "BUNDLED" if candidate.mode.value == "BUNDLED" else "EXTERNAL"
        ui.adb_pill.set(how, OK, "ready")
        ui.append_log(f"ADB: {candidate.mode.value} \u2014 {candidate.path}", "info")
    else:
        ui.adb_pill.set("MISSING", ERR, "cross")
        ui.append_log(f"ADB unavailable: {candidate.problem}", "err")
        for line in adb_setup_hint().splitlines():
            ui.append_log(line, "dim")

    if ui.controller.adb_error:
        ui.append_log(ui.controller.adb_error, "err")

    root.deiconify()
    root.protocol("WM_DELETE_WINDOW", ui.on_close)
    root.mainloop()
    return 0


if __name__ == "__main__":
    import sys

    sys.exit(main())