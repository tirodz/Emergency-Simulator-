"""Polished Windows desktop console for Emergency-Simulator.

The layout intentionally follows the visual ideas established in the CMF Ringtone Tool reference:
a dark graphite workspace, orange accent, layered panels, compact navigation, generous spacing and
status-first information design. It is implemented natively with Tkinter so the application remains
self-contained and reliable in the Windows executable.

All device work still goes through EmergencySimulatorController. The UI is presentation and operator
intent only; it cannot bypass the controller's safety gates.
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
from .models import AlertState, Device, DeviceState, FailureCode, SupportLevel, TransactionState
from .widgets import (
    ACCENT,
    ACCENT_DIM,
    BG,
    BG_ALT,
    BORDER,
    ERR,
    FG,
    FG_DIM,
    FG_FAINT,
    INFO,
    OK,
    PANEL,
    PANEL_ALT,
    PANEL_RAISED,
    WARN,
    Banner,
    Button,
    Card,
    DeviceRow,
    LogView,
    StatusPill,
    glyph,
    mono,
    ui_font,
    state_colour,
)

SAFETY_STRIP = "TEST ONLY      CONTROLLED DEVICE      NO CELLULAR TRANSMISSION"

_FAILURE_HINTS = {
    "TEST_MODE_DISABLED": (
        "Test alerts are disabled on the device and could not be enabled. "
        "The message would be dropped by the receiver."
    ),
    "CELLBROADCAST_FILTERED": (
        "The message reached the receiver but was filtered. Check the device's test-mode settings."
    ),
    "DUPLICATE_SEND_BLOCKED": (
        "An earlier alert may still be outstanding. Android queues alerts; dismiss it on the device "
        "and acknowledge it here before sending again."
    ),
    "NO_ROOT": (
        "This controlled test path requires root/system-level authority. The framework rejects the "
        "protected emergency-broadcast action from an ordinary shell."
    ),
    "TIMEOUT": (
        "A timeout is not proof that nothing was delivered. Check the device screen before retrying."
    ),
}


class EmergencySimulatorUI:
    """Main operator console."""

    def __init__(self, root: tk.Tk, controller: Optional[EmergencySimulatorController] = None):
        self.root = root
        self.root.title("EMERGENCY-SIMULATOR  —  Android Alert Lab  [" + __version__ + "]")
        self.root.configure(bg=BG)
        self.root.geometry("1180x820")
        self.root.minsize(980, 700)

        self._events: "queue.Queue[tuple]" = queue.Queue()
        self._cancel = threading.Event()
        self._busy = False
        self._devices: List[Device] = []
        self._selected: Optional[Device] = None
        self._rows: dict = {}
        self._last_result = None
        self._pulse_on = False

        self.controller = controller or EmergencySimulatorController(
            cancel_check=self._cancel.is_set,
            on_log=lambda m: self._events.put(("log", m)),
        )

        self._build()
        self.root.after(80, self._pump)
        self.root.after(220, self.on_refresh)
        self.root.after(700, self._pulse_status)

    # -- construction ------------------------------------------------------

    def _build(self) -> None:
        shell = tk.Frame(self.root, bg=BG)
        shell.pack(fill="both", expand=True)

        self._build_sidebar(shell)
        main = tk.Frame(shell, bg=BG)
        main.pack(side="left", fill="both", expand=True)

        self._build_topbar(main)

        content = tk.Frame(main, bg=BG)
        content.pack(fill="both", expand=True, padx=22, pady=(0, 18))

        self._build_header(content)
        self._build_metrics(content)

        workspace = tk.Frame(content, bg=BG)
        workspace.pack(fill="both", expand=True, pady=(12, 0))
        workspace.columnconfigure(0, weight=5)
        workspace.columnconfigure(1, weight=6)
        workspace.rowconfigure(0, weight=1)

        self._build_devices(workspace)
        self._build_alert(workspace)

        self._build_log(content)

    def _build_sidebar(self, parent: tk.Frame) -> None:
        side = tk.Frame(parent, bg=BG_ALT, width=208)
        side.pack(side="left", fill="y")
        side.pack_propagate(False)

        brand = tk.Frame(side, bg=BG_ALT)
        brand.pack(fill="x", padx=18, pady=(22, 26))
        mark = tk.Canvas(brand, width=42, height=42, bg=BG_ALT, highlightthickness=0)
        mark.pack(side="left")
        mark.create_oval(4, 4, 38, 38, fill=ACCENT_DIM, outline=ACCENT)
        mark.create_text(21, 21, text="E", fill=FG, font=ui_font(17, True))
        text_box = tk.Frame(brand, bg=BG_ALT)
        text_box.pack(side="left", padx=(10, 0))
        tk.Label(
            text_box, text="EMERGENCY", bg=BG_ALT, fg=FG, font=ui_font(10, True)
        ).pack(anchor="w")
        tk.Label(
            text_box, text="SIMULATOR", bg=BG_ALT, fg=FG_FAINT, font=ui_font(8, True)
        ).pack(anchor="w", pady=(1, 0))

        nav_title = tk.Label(
            side, text="CONTROL CENTER", bg=BG_ALT, fg=FG_FAINT, font=ui_font(8, True)
        )
        nav_title.pack(anchor="w", padx=18, pady=(0, 8))

        self._nav_rows = []
        for label, icon, active in (
            ("Overview", "•", True),
            ("Devices", "■", False),
            ("Activity", "≡", False),
        ):
            row = tk.Frame(side, bg=PANEL_ALT if active else BG_ALT, height=38)
            row.pack(fill="x", padx=12, pady=2)
            tk.Label(row, text=icon, bg=row.cget("bg"), fg=ACCENT if active else FG_FAINT,
                     font=ui_font(10, True), width=2).pack(side="left", padx=(8, 0))
            tk.Label(row, text=label, bg=row.cget("bg"), fg=FG if active else FG_DIM,
                     font=ui_font(9, True if active else False)).pack(side="left")
            if active:
                tk.Frame(row, bg=ACCENT, width=3).pack(side="right", fill="y")
            self._nav_rows.append(row)

        info = tk.Frame(side, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        info.pack(side="bottom", fill="x", padx=12, pady=14)
        tk.Label(info, text="SAFE TEST PROFILE", bg=PANEL, fg=ACCENT, font=ui_font(8, True)).pack(
            anchor="w", padx=11, pady=(10, 2)
        )
        tk.Label(
            info,
            text="ETWS TEST  ·  4355\nNo cellular transmission",
            bg=PANEL,
            fg=FG_DIM,
            justify="left",
            font=ui_font(8),
        ).pack(anchor="w", padx=11, pady=(0, 10))

        tk.Label(
            side,
            text="v" + __version__ + "  ·  Controlled lab build",
            bg=BG_ALT,
            fg=FG_FAINT,
            font=ui_font(7),
        ).pack(side="bottom", anchor="w", padx=18, pady=(0, 16))

    def _build_topbar(self, parent: tk.Frame) -> None:
        bar = tk.Frame(parent, bg=BG, height=58)
        bar.pack(fill="x", padx=22)
        bar.pack_propagate(False)

        left = tk.Frame(bar, bg=BG)
        left.pack(side="left", fill="y")
        tk.Label(left, text="Android Alert Console", bg=BG, fg=FG, font=ui_font(9, True)).pack(
            side="left", pady=18
        )
        tk.Label(
            left, text="  /  01", bg=BG, fg=FG_FAINT, font=mono(8)
        ).pack(side="left", pady=18)

        right = tk.Frame(bar, bg=BG)
        right.pack(side="right", fill="y")
        self.mode_pill = StatusPill(right, "MODE", "CONTROLLED")
        self.mode_pill.pack(side="left", pady=12)
        tk.Frame(right, bg=BG, width=10).pack(side="left")
        self.adb_pill = StatusPill(right, "ADB")
        self.adb_pill.pack(side="left", pady=12)

    def _build_header(self, parent: tk.Frame) -> None:
        head = tk.Frame(parent, bg=BG)
        head.pack(fill="x", pady=(2, 0))

        copy = tk.Frame(head, bg=BG)
        copy.pack(side="left")
        tk.Label(copy, text="Test Alert Console", bg=BG, fg=FG, font=ui_font(22, True)).pack(
            anchor="w"
        )
        sub = tk.Frame(copy, bg=BG)
        sub.pack(anchor="w", pady=(4, 0))
        self.status_dot = tk.Label(sub, text=glyph("ready"), bg=BG, fg=OK, font=ui_font(9, True))
        self.status_dot.pack(side="left")
        self.status_text = tk.Label(
            sub,
            text="Waiting for a controlled device",
            bg=BG,
            fg=FG_DIM,
            font=ui_font(9),
        )
        self.status_text.pack(side="left", padx=(7, 0))

        self.safety_banner = Banner(head, SAFETY_STRIP, fg=ACCENT, bg=PANEL_ALT)
        self.safety_banner.pack(side="right", padx=(18, 0), pady=(6, 0))

        self.outcome_banner = Banner(head, "", fg=FG, bg=PANEL_ALT)
        self.outcome_banner.pack(fill="x", pady=(12, 0))
        self.outcome_banner.pack_forget()

    def _build_metrics(self, parent: tk.Frame) -> None:
        metrics = tk.Frame(parent, bg=BG)
        metrics.pack(fill="x", pady=(12, 0))
        for i in range(3):
            metrics.columnconfigure(i, weight=1)

        self.metric_connected = self._metric(metrics, "CONNECTED DEVICES", "0", "ADB")
        self.metric_ready = self._metric(metrics, "READY TARGETS", "0", "root + CB")
        self.metric_channel = self._metric(metrics, "LOCKED CHANNEL", "4355", "ETWS TEST")
        self.metric_connected.grid(row=0, column=0, sticky="ew", padx=(0, 5))
        self.metric_ready.grid(row=0, column=1, sticky="ew", padx=5)
        self.metric_channel.grid(row=0, column=2, sticky="ew", padx=(5, 0))

    def _metric(self, parent: tk.Frame, title: str, value: str, meta: str) -> tk.Frame:
        card = tk.Frame(parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        body = tk.Frame(card, bg=PANEL)
        body.pack(fill="both", expand=True, padx=13, pady=10)
        tk.Label(body, text=title, bg=PANEL, fg=FG_FAINT, font=ui_font(7, True)).pack(anchor="w")
        bottom = tk.Frame(body, bg=PANEL)
        bottom.pack(fill="x", pady=(4, 0))
        label = tk.Label(bottom, text=value, bg=PANEL, fg=FG, font=ui_font(16, True))
        label.pack(side="left")
        tk.Label(
            bottom, text=meta, bg=PANEL, fg=FG_FAINT, font=ui_font(7, True)
        ).pack(side="right", pady=(5, 0))
        card._value_label = label
        return card

    def _build_devices(self, parent: tk.Frame) -> None:
        card = Card(parent, "Devices")
        card.grid(row=0, column=0, sticky="nsew", padx=(0, 7))

        head = tk.Frame(card.body, bg=PANEL)
        head.pack(fill="x")
        self.device_hint = tk.Label(
            head,
            text="ADB targets are assessed for actual readiness, not just enumeration.",
            bg=PANEL,
            fg=FG_DIM,
            font=ui_font(8),
        )
        self.device_hint.pack(side="left")

        self.btn_refresh = Button(head, "Refresh", self.on_refresh, variant="ghost", icon="refresh")
        self.btn_refresh.pack(side="right")

        self.device_list = tk.Frame(card.body, bg=PANEL)
        self.device_list.pack(fill="both", expand=True, pady=(11, 0))

        self._empty_label = tk.Label(
            self.device_list,
            text="Scanning for attached devices…",
            bg=PANEL,
            fg=FG_FAINT,
            font=ui_font(9),
            pady=30,
        )
        self._empty_label.pack(fill="x")

        foot = tk.Frame(card.body, bg=PANEL)
        foot.pack(fill="x", pady=(10, 0))
        tk.Label(
            foot,
            text="Controlled development mode  ·  Stock devices are not claimed supported.",
            bg=PANEL,
            fg=FG_FAINT,
            font=ui_font(7),
        ).pack(side="left")
        self.btn_ack = Button(foot, "Acknowledge", self.on_acknowledge, variant="ghost")
        self.btn_ack.pack(side="right")
        self.btn_ack.set_enabled(False)

    def _build_alert(self, parent: tk.Frame) -> None:
        card = Card(parent, "Test alert")
        card.grid(row=0, column=1, sticky="nsew", padx=(7, 0))
        body = card.body

        target = tk.Frame(body, bg=PANEL)
        target.pack(fill="x")
        left = tk.Frame(target, bg=PANEL)
        left.pack(side="left", fill="x", expand=True)
        tk.Label(left, text="TARGET", bg=PANEL, fg=FG_FAINT, font=ui_font(7, True)).pack(anchor="w")
        self.target_value = tk.Label(
            left, text="none selected", bg=PANEL, fg=FG, font=ui_font(14, True)
        )
        self.target_value.pack(anchor="w", pady=(2, 0))
        self.target_verdict = tk.Label(
            left,
            text="Connect a controlled device to begin.",
            bg=PANEL,
            fg=FG_DIM,
            font=ui_font(8),
            wraplength=420,
            justify="left",
        )
        self.target_verdict.pack(anchor="w", pady=(4, 0))

        self.target_status = StatusPill(target, "STATE", "IDLE")
        self.target_status.pack(side="right", anchor="n")

        tk.Frame(body, bg=BORDER, height=1).pack(fill="x", pady=13)

        meta = tk.Frame(body, bg=PANEL)
        meta.pack(fill="x")
        self._meta_box(meta, "TYPE", "ETWS TEST")
        self._meta_box(meta, "CHANNEL", str(SERVICE_CATEGORY) + "  /  0x1103")
        self._meta_box(meta, "MODE", "LOCKED")

        tk.Label(body, text="MESSAGE", bg=PANEL, fg=FG_FAINT, font=ui_font(7, True)).pack(
            anchor="w", pady=(13, 5)
        )

        self.msg_var = tk.StringVar(value=DEFAULT_BODY)
        self.msg_entry = tk.Entry(
            body,
            textvariable=self.msg_var,
            bg=PANEL_ALT,
            fg=FG,
            insertbackground=FG,
            selectbackground=ACCENT_DIM,
            selectforeground=FG,
            font=ui_font(10),
            relief="flat",
            bd=0,
            highlightbackground=BORDER,
            highlightcolor=ACCENT,
            highlightthickness=1,
        )
        self.msg_entry.pack(fill="x", ipady=10)
        self.msg_var.trace_add("write", lambda *_: self._on_message_changed())

        self.msg_feedback = tk.Label(
            body,
            text="Must begin with TEST.",
            bg=PANEL,
            fg=FG_FAINT,
            font=ui_font(8),
            anchor="w",
            justify="left",
            wraplength=500,
        )
        self.msg_feedback.pack(fill="x", pady=(5, 0))

        action = tk.Frame(body, bg=PANEL)
        action.pack(fill="x", pady=(14, 0))
        self.btn_send = Button(action, "SEND TEST ALERT", self.on_send, variant="primary", icon="alert")
        self.btn_send.pack(side="left", fill="x", expand=True)
        self.btn_send.set_enabled(False)

        secondary = tk.Frame(body, bg=PANEL)
        secondary.pack(fill="x", pady=(8, 0))
        self.btn_dry = Button(secondary, "Dry Run", self.on_dry_run, variant="ghost")
        self.btn_dry.pack(side="left", fill="x", expand=True)
        self.btn_stop = Button(secondary, "STOP / CANCEL", self.on_stop, variant="danger", icon="cancel")
        self.btn_stop.pack(side="left", fill="x", expand=True, padx=(8, 0))
        self.btn_stop.set_enabled(False)

        note = tk.Frame(body, bg=PANEL_ALT, highlightbackground=BORDER, highlightthickness=1)
        note.pack(fill="x", pady=(12, 0))
        tk.Label(
            note,
            text="ⓘ  What happens",
            bg=PANEL_ALT,
            fg=INFO,
            font=ui_font(8, True),
        ).pack(anchor="w", padx=11, pady=(9, 2))
        tk.Label(
            note,
            text="The controller uses Android's own protected test-alert pipeline. "
                 "No RF, modem or cellular transmission is performed.",
            bg=PANEL_ALT,
            fg=FG_DIM,
            font=ui_font(8),
            wraplength=520,
            justify="left",
        ).pack(anchor="w", padx=11, pady=(0, 9))

    def _meta_box(self, parent: tk.Frame, label: str, value: str) -> None:
        box = tk.Frame(parent, bg=PANEL)
        box.pack(side="left", fill="x", expand=True)
        tk.Label(box, text=label, bg=PANEL, fg=FG_FAINT, font=ui_font(7, True)).pack(anchor="w")
        tk.Label(box, text=value, bg=PANEL, fg=FG_DIM, font=ui_font(8, True)).pack(
            anchor="w", pady=(3, 0)
        )

    def _build_log(self, parent: tk.Frame) -> None:
        outer = tk.Frame(parent, bg=BG)
        outer.pack(fill="both", expand=False, pady=(12, 0))
        head = tk.Frame(outer, bg=BG)
        head.pack(fill="x")
        tk.Label(head, text="ACTIVITY", bg=BG, fg=FG_FAINT, font=ui_font(7, True)).pack(side="left")
        self.result_pill = StatusPill(head, "LAST RESULT")
        self.result_pill.pack(side="right")

        self.log_view = LogView(outer, height=8)
        self.log_view.pack(fill="both", expand=False, pady=(6, 0))
        self.log_text = self.log_view.text

    # -- animation ---------------------------------------------------------

    def _pulse_status(self) -> None:
        if not self.root.winfo_exists():
            return
        self._pulse_on = not self._pulse_on
        if self._busy:
            self.status_dot.configure(fg=WARN if self._pulse_on else ACCENT, text="●")
        else:
            self.status_dot.configure(fg=OK, text="●")
        self.root.after(700, self._pulse_status)

    # -- worker plumbing ----------------------------------------------------

    def _pump(self) -> None:
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
        self.btn_refresh.set_enabled(not busy)
        self.btn_ack.set_enabled(
            not busy
            and self._selected is not None
            and self.controller.transaction_state(self._selected.serial) is not TransactionState.READY
        )
        self.btn_stop.set_enabled(busy)
        self.status_text.configure(text="Working…" if busy else self._status_text())
        self._update_target_panel()

    def _run_async(self, fn) -> None:
        if self._busy:
            return
        self._cancel.clear()
        self._set_busy(True)
        threading.Thread(target=fn, daemon=True).start()

    def _worker(self, fn) -> None:
        try:
            fn()
        except SafetyError as exc:
            self._events.put(("failure", str(exc)))
        except Exception as exc:
            self._events.put(("failure", type(exc).__name__ + ": " + str(exc)))
        finally:
            self._events.put(("done", None))

    def _status_text(self) -> str:
        if not self._selected:
            return "Waiting for a controlled device"
        if self._selected.is_usable:
            return self._selected.model or self._selected.serial
        return self._selected.support_reason

    # -- gate ---------------------------------------------------------------

    def _gate_clear(self) -> bool:
        if self._selected is None:
            return False
        return self.controller.transaction_state(self._selected.serial) is TransactionState.READY

    def _show_gate_state(self) -> None:
        if self._selected is None:
            return
        state = self.controller.transaction_state(self._selected.serial)
        if state is TransactionState.READY:
            return
        colour = state_colour(state.value)
        self.target_verdict.configure(
            text=glyph("warn") + " " + state.value
            + ": dismiss any alert on the device, then Acknowledge before sending again.",
            fg=colour,
        )

    def _refresh_ack_button(self) -> None:
        if self._selected is None or self._busy:
            self.btn_ack.set_enabled(False)
            return
        self.btn_ack.set_enabled(
            self.controller.transaction_state(self._selected.serial) is not TransactionState.READY
        )

    def on_acknowledge(self) -> None:
        dev = self._selected
        if dev is None:
            self.append_log("Select a device first.", "warn")
            return
        state = self.controller.transaction_state(dev.serial)
        if state is TransactionState.READY:
            self.append_log(dev.serial + " has no outstanding alert.", "info")
            return
        if not messagebox.askyesno(
            "Acknowledge outstanding alert?",
            "Only continue after you have dismissed the alert on " + dev.serial + ".\n\n"
            "Acknowledging clears the local send block. Android queues alerts and cannot withdraw one.",
            parent=self.root,
            default="no",
            icon="warning",
        ):
            self.append_log("Acknowledge cancelled.", "info")
            return
        self.controller.acknowledge(dev.serial)
        self._events.put(("ackdone", dev.serial))

    def _after_acknowledge(self, serial: str) -> None:
        self.append_log("Acknowledged " + serial + ": send gate is clear.", "ok")
        self._update_target_panel()

    # -- validation ---------------------------------------------------------

    def _on_message_changed(self) -> None:
        raw = self.msg_var.get()
        if not raw.strip():
            self.msg_feedback.configure(text="Must begin with TEST.", fg=FG_FAINT)
            return
        try:
            validate_body(raw)
        except SafetyError as exc:
            self.msg_feedback.configure(text=glyph("cross") + " " + str(exc), fg=ERR)
        else:
            self.msg_feedback.configure(
                text=glyph("check") + " accepted — the alert will be labelled as a test",
                fg=OK,
            )

    def _validated_body(self) -> Optional[str]:
        try:
            return validate_body(self.msg_var.get())
        except SafetyError as exc:
            messagebox.showerror("Message rejected", str(exc), parent=self.root)
            self.append_log(str(exc), "err")
            return None

    # -- device rendering ---------------------------------------------------

    def _render_devices(self, devices: List[Device]) -> None:
        self._devices = devices
        for row in self._rows.values():
            row.destroy()
        self._rows.clear()
        if not devices:
            self._empty_label.configure(text="No Android devices detected.\nEnable USB debugging and connect a controlled target.")
            if not self._empty_label.winfo_ismapped():
                self._empty_label.pack(fill="x")
            self._selected = None
        else:
            self._empty_label.pack_forget()
            for dev in devices:
                row = DeviceRow(self.device_list, self._on_device_selected)
                row.pack(fill="x", pady=4)
                meta = "Android " + (dev.release or "?") + "  ·  API " + (dev.sdk or "?")
                row.update_row(
                    dev.serial,
                    dev.model or dev.product or dev.serial,
                    meta,
                    dev.support_level.value,
                    state_colour(dev.support_level.value),
                    "ready" if dev.is_usable else ("warn" if dev.state is DeviceState.NO_ROOT else "unknown"),
                )
                self._rows[dev.serial] = row

            current = self._selected.serial if self._selected else ""
            preferred = next((d for d in devices if d.serial == current), None)
            if preferred is None:
                preferred = next((d for d in devices if d.is_usable), devices[0])
            self._selected = preferred

        ready = sum(1 for d in devices if d.is_usable)
        self.metric_connected._value_label.configure(text=str(len(devices)))
        self.metric_ready._value_label.configure(text=str(ready))

        if self._selected:
            self._on_device_selected(self._selected.serial)
        else:
            self._update_target_panel()

    def _on_device_selected(self, serial: str) -> None:
        for dev in self._devices:
            if dev.serial == serial:
                self._selected = dev
                break
        for key, row in self._rows.items():
            row.set_selected(key == serial)
        self._update_target_panel()

    def _can_send(self) -> bool:
        return bool(
            not self._busy
            and self._selected is not None
            and self._selected.is_usable
            and self._gate_clear()
        )

    def _update_target_panel(self) -> None:
        dev = self._selected
        if dev is None:
            self.target_value.configure(text="none selected", fg=FG_DIM)
            self.target_verdict.configure(text="Connect a controlled device to begin.", fg=FG_DIM)
            self.target_status.set("IDLE", FG_DIM, "unknown")
            self.btn_send.set_enabled(False)
            self._refresh_ack_button()
            self.status_text.configure(text="Waiting for a controlled device")
            return

        level = dev.support_level
        self.target_value.configure(text=dev.serial, fg=FG)
        self.target_verdict.configure(text=level.value + " — " + dev.support_reason, fg=FG_DIM)
        self.target_status.set(dev.state.value, state_colour(dev.state.value))
        self._show_gate_state()
        self._refresh_ack_button()

        self.btn_send.set_enabled(self._can_send())
        self.btn_dry.set_enabled(not self._busy and level is not SupportLevel.UNSUPPORTED)
        self.status_text.configure(text=self._status_text())

    # -- actions ------------------------------------------------------------

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

    def on_dry_run(self) -> None:
        dev = self._selected
        if dev is None:
            messagebox.showwarning("No device selected", "Select a device first.", parent=self.root)
            return
        body = self._validated_body()
        if body is None:
            return
        self.append_log("Dry run against " + dev.serial + ": checking everything, sending nothing.")

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
                dev.support_level.value + "\n\n" + dev.support_reason
                + "\n\n" + "\n".join("· " + n for n in dev.notes),
                parent=self.root,
            )
            return
        body = self._validated_body()
        if body is None:
            return
        state = self.controller.transaction_state(dev.serial)
        if state is not TransactionState.READY:
            messagebox.showwarning(
                "Outstanding alert",
                dev.serial + " is " + state.value + ".\n\n"
                + self.controller.gate_explanation(dev.serial),
                parent=self.root,
            )
            return

        confirm = messagebox.askyesno(
            "Send test alert?",
            "This will trigger a TEST emergency alert through Android's genuine protected "
            "Cell Broadcast test path.\n\n"
            "Audio and the system alert dialog are proven on the development target. "
            "Vibration and lock-screen behavior remain device-dependent and unverified here.\n\n"
            "Target:  " + dev.serial + "\n"
            "Model:   " + (dev.model or "unknown") + "\n"
            "Android: " + (dev.release or "?") + " / API " + (dev.sdk or "?") + "\n"
            "Type:    ETWS TEST\n"
            "Channel: " + str(SERVICE_CATEGORY) + " (locked)\n"
            "Message: " + body + "\n\n"
            "No cellular transmission occurs.",
            parent=self.root,
            default="no",
            icon="warning",
        )
        if not confirm:
            self.append_log("Cancelled at confirmation. Nothing was sent.", "warn")
            return

        self.append_log("Sending ETWS TEST alert to " + dev.serial)

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
            "warn",
        )
        self._cancel.clear()

    # -- results ------------------------------------------------------------

    def _render_result(self, result) -> None:
        self._last_result = result
        value = result.state.value
        colour = state_colour(value)
        self.result_pill.set(value.replace("_", " "), colour, "ready" if result.ok else "warn")
        self.outcome_banner.pack(fill="x", pady=(12, 0), after=self.safety_banner)
        if result.ok:
            self.outcome_banner.set(
                "✓  GENUINE ALERT DISPLAYED   ·   " + result.device_serial
                + "   ·   DISMISS IT ON THE DEVICE",
                fg=OK,
                bg="#102018",
            )
            self.append_log(result.message, "ok")
            for evidence in result.evidence:
                self.append_log(evidence, "ok")
        else:
            failure = result.failure.value if result.failure else value
            self.outcome_banner.set("×  " + failure + "   ·   " + result.device_serial, fg=ERR, bg="#211417")
            self.append_log(result.message or "Operation failed.", "err")
            hint = _FAILURE_HINTS.get(failure)
            if hint:
                self.append_log(hint, "dim")

        self.target_status.set(value, colour, "ready" if result.ok else "warn")
        self._update_target_panel()

    def _render_failure(self, message: str) -> None:
        self.outcome_banner.pack(fill="x", pady=(12, 0), after=self.safety_banner)
        self.outcome_banner.set("×  OPERATION FAILED", fg=ERR, bg="#211417")
        self.result_pill.set("ERROR", ERR, "cross")
        self.append_log(message, "err")

    # -- lifecycle ----------------------------------------------------------

    def on_close(self) -> None:
        self._cancel.set()
        self.root.destroy()

    # -- compatibility helpers used by older tests and scripts ------------

    def append_log(self, message: str, tag: str = "") -> None:
        self.log_view.append(message, tag)

    def configure_message(self, body: str) -> None:
        self.msg_var.set(body)


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
        print("ERROR: " + str(exc), file=sys.stderr)
        return 1
    except Exception as exc:
        root.destroy()
        print("ERROR: " + str(exc), file=sys.stderr)
        return 1

    ui.append_log("Session log: " + str(log_file), "dim")
    candidate = locate_adb()
    if candidate.works:
        ui.adb_pill.set("BUNDLED" if candidate.mode.value == "BUNDLED" else "EXTERNAL", OK, "ready")
        ui.append_log("ADB: " + candidate.mode.value + " — " + candidate.path, "info")
    else:
        ui.adb_pill.set("MISSING", ERR, "cross")
        ui.append_log("ADB unavailable: " + (candidate.problem or "unknown"), "err")
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
