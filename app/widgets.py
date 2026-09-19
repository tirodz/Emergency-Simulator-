"""Modern visual primitives for the Emergency-Simulator desktop console.

The visual language takes cues from the project's CMF Ringtone Tool reference: deep charcoal surfaces,
soft borders, restrained glass-like layering, generous spacing, Segoe UI typography and one vivid orange
accent. Tkinter cannot reproduce true compositor blur, so the implementation uses layered solid surfaces
and subtle highlights instead. The result remains native, dependency-light and reliable on Windows.
"""

from __future__ import annotations

import tkinter as tk
from typing import Callable, Optional

BG = "#080a0d"
BG_ALT = "#0d1014"
PANEL = "#11151b"
PANEL_ALT = "#171c23"
PANEL_RAISED = "#1e252e"
BORDER = "#252d37"
BORDER_LIGHT = "#303946"

FG = "#f0f2f5"
FG_DIM = "#a0a8b5"
FG_FAINT = "#687281"

ACCENT = "#ff650f"
ACCENT_ACTIVE = "#ff7d2c"
ACCENT_DIM = "#6e2b0c"

OK = "#44cf85"
WARN = "#e0aa4b"
ERR = "#ef6461"
INFO = "#5ea9e6"
NEUTRAL = "#8792a2"

FONT = "Segoe UI Variable"
FONT_FALLBACK = "Segoe UI"
MONO = "Cascadia Mono"
MONO_FALLBACK = "Consolas"


def ui_font(size: int = 9, bold: bool = False) -> tuple:
    return (FONT, size, "bold") if bold else (FONT, size)


def mono(size: int = 9, bold: bool = False) -> tuple:
    return (MONO, size, "bold") if bold else (MONO, size)


def mono_fallback(size: int = 9, bold: bool = False) -> tuple:
    return (MONO_FALLBACK, size, "bold") if bold else (MONO_FALLBACK, size)


GLYPHS = {
    "alert": "▲",
    "cancel": "×",
    "device": "■",
    "ready": "●",
    "unknown": "○",
    "check": "✓",
    "cross": "✕",
    "warn": "!",
    "arrow": "→",
    "bullet": "·",
    "lock": "▣",
    "refresh": "↻",
}


def glyph(name: str) -> str:
    return GLYPHS.get(name, GLYPHS["bullet"])


def state_colour(state: str) -> str:
    return {
        "READY": OK,
        "SUPPORTED": OK,
        "ALERT_DISPLAYED": OK,
        "SUCCESS": OK,
        "BUSY": WARN,
        "UNCERTAIN": WARN,
        "WARN": WARN,
        "OFFLINE": ERR,
        "UNAUTHORIZED": ERR,
        "NO_ROOT": ERR,
        "UNSUPPORTED": ERR,
        "FAILED": ERR,
        "ERROR": ERR,
        "UNKNOWN": FG_DIM,
        "UNTESTED": FG_DIM,
        "ROOT_REQUIRED": ERR,
        "IDLE": FG_DIM,
        "CANCELLED": FG_DIM,
    }.get(state.upper(), FG)


class Card(tk.Frame):
    """Layered panel with a soft border and a small caption."""

    def __init__(self, parent, title: str = "", pad: int = 16, **kw):
        super().__init__(
            parent,
            bg=PANEL,
            highlightbackground=BORDER,
            highlightcolor=BORDER_LIGHT,
            highlightthickness=1,
            bd=0,
            **kw,
        )
        if title:
            header = tk.Frame(self, bg=PANEL)
            header.pack(fill="x", padx=pad, pady=(pad, 0))
            self.caption = tk.Label(
                header,
                text=title.upper(),
                bg=PANEL,
                fg=FG_FAINT,
                font=ui_font(8, True),
            )
            self.caption.pack(side="left")
            tk.Frame(self, bg=ACCENT, height=2, width=28).pack(side="right", pady=2)
        self.body = tk.Frame(self, bg=PANEL)
        self.body.pack(fill="both", expand=True, padx=pad, pady=(10 if title else pad, pad))


class Button(tk.Frame):
    """A consistent mouse/keyboard-friendly flat control."""

    def __init__(
        self,
        parent,
        text: str,
        command: Optional[Callable[[], None]] = None,
        variant: str = "default",
        icon: str = "",
        padx: int = 14,
        pady: int = 8,
    ):
        styles = {
            "primary": (ACCENT, "#ffffff", ACCENT_ACTIVE),
            "danger": (PANEL_RAISED, ERR, "#28313b"),
            "ghost": (PANEL, FG_DIM, PANEL_ALT),
            "default": (PANEL_RAISED, FG, "#28313b"),
        }
        bg, fg, active = styles.get(variant, styles["default"])
        self._bg, self._fg, self._active = bg, fg, active
        self._enabled = True
        self._command = command
        super().__init__(parent, bg=bg, bd=0, highlightthickness=0)
        self._label = tk.Label(
            self,
            text=((glyph(icon) + "  ") if icon else "") + text,
            bg=bg,
            fg=fg,
            font=ui_font(9, True),
            padx=padx,
            pady=pady,
            cursor="hand2",
        )
        self._label.pack(fill="both", expand=True)
        for widget in (self, self._label):
            widget.bind("<Button-1>", self._on_click)
            widget.bind("<Enter>", self._on_enter)
            widget.bind("<Leave>", self._on_leave)

    def _paint(self, bg: str, fg: Optional[str] = None) -> None:
        self.configure(bg=bg)
        self._label.configure(bg=bg, fg=(fg if fg is not None else self._fg))

    def _on_click(self, _event) -> None:
        if self._enabled and self._command:
            self._command()

    def _on_enter(self, _event) -> None:
        if self._enabled:
            self._paint(self._active)

    def _on_leave(self, _event) -> None:
        if self._enabled:
            self._paint(self._bg)

    def set_enabled(self, enabled: bool) -> None:
        self._enabled = enabled
        self._paint(self._bg if enabled else BG_ALT, self._fg if enabled else FG_FAINT)

    def configure_text(self, text: str, icon: str = "") -> None:
        self._label.configure(text=((glyph(icon) + "  ") if icon else "") + text)


class StatusPill(tk.Frame):
    """Status word with a dot; readable without relying on colour."""

    def __init__(self, parent, label: str = "", value: str = "---"):
        super().__init__(parent, bg=BG_ALT)
        if label:
            self._label = tk.Label(
                self,
                text=label,
                bg=BG_ALT,
                fg=FG_FAINT,
                font=ui_font(8, True),
            )
            self._label.pack(side="left", padx=(9, 4))
        else:
            self._label = tk.Label(self, text="", bg=BG_ALT)
        self._dot = tk.Label(self, text=glyph("unknown"), bg=BG_ALT, fg=FG_DIM, font=ui_font(9))
        self._dot.pack(side="left")
        self._value = tk.Label(
            self, text=value, bg=BG_ALT, fg=FG, font=ui_font(8, True)
        )
        self._value.pack(side="left", padx=(4, 9))

    def set(self, value: str, colour: Optional[str] = None, mark: str = "ready") -> None:
        self._value.configure(text=value)
        colour = colour or state_colour(value)
        self._value.configure(fg=colour)
        self._dot.configure(text=glyph(mark), fg=colour)


class Banner(tk.Frame):
    """Full-width announcement strip."""

    def __init__(self, parent, text: str, fg: str = ACCENT, bg: str = PANEL_ALT):
        super().__init__(
            parent,
            bg=bg,
            highlightbackground=BORDER,
            highlightthickness=1,
            bd=0,
        )
        self._label = tk.Label(
            self, text=text, bg=bg, fg=fg, font=ui_font(8, True), pady=9, padx=13
        )
        self._label.pack(fill="x")

    def set(self, text: str, fg: str = ACCENT, bg: str = PANEL_ALT) -> None:
        self.configure(bg=bg, highlightbackground=BORDER)
        self._label.configure(text=text, fg=fg, bg=bg)


class DeviceRow(tk.Frame):
    """Selectable device card row."""

    def __init__(self, parent, on_select: Callable[[str], None]):
        super().__init__(
            parent,
            bg=PANEL_ALT,
            highlightthickness=1,
            highlightbackground=BORDER,
            bd=0,
        )
        self._serial = ""
        self._on_select = on_select
        self.selected = False

        inner = tk.Frame(self, bg=PANEL_ALT)
        inner.pack(fill="x", padx=12, pady=10)

        self._dot = tk.Label(
            inner, text=glyph("unknown"), bg=PANEL_ALT, fg=FG_DIM, font=ui_font(10, True)
        )
        self._dot.grid(row=0, column=0, rowspan=2, sticky="w", padx=(0, 10))

        self._name = tk.Label(
            inner, text="", bg=PANEL_ALT, fg=FG, font=ui_font(9, True), anchor="w"
        )
        self._name.grid(row=0, column=1, sticky="w")

        self._meta = tk.Label(
            inner, text="", bg=PANEL_ALT, fg=FG_DIM, font=ui_font(8), anchor="w"
        )
        self._meta.grid(row=1, column=1, sticky="w", pady=(2, 0))

        self._verdict = tk.Label(
            inner, text="", bg=PANEL_ALT, fg=FG_DIM, font=ui_font(8, True)
        )
        self._verdict.grid(row=0, column=2, rowspan=2, sticky="e", padx=(12, 0))
        inner.columnconfigure(1, weight=1)

        for widget in (self, inner, self._dot, self._name, self._meta, self._verdict):
            widget.bind("<Button-1>", self._click)
            widget.configure(cursor="hand2")

    def _click(self, _event) -> None:
        if self._serial:
            self._on_select(self._serial)

    def update_row(
        self,
        serial: str,
        name: str,
        meta: str,
        verdict: str,
        colour: str,
        mark: str,
    ) -> None:
        self._serial = serial
        self._name.configure(text=name or serial)
        self._meta.configure(text=(serial + "   " + meta).strip())
        self._verdict.configure(text=verdict, fg=colour)
        self._dot.configure(text=glyph(mark), fg=colour)

    def set_selected(self, selected: bool) -> None:
        self.selected = selected
        bg = PANEL_RAISED if selected else PANEL_ALT
        border = ACCENT if selected else BORDER
        self.configure(bg=bg, highlightbackground=border)
        for widget in self.winfo_children():
            widget.configure(bg=bg)
            for sub in widget.winfo_children():
                sub.configure(bg=bg)


class LogView(tk.Frame):
    """Append-only activity log."""

    def __init__(self, parent, height: int = 11):
        super().__init__(parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1, bd=0)
        self.text = tk.Text(
            self,
            bg=BG_ALT,
            fg=FG,
            font=ui_font(9),
            relief="flat",
            highlightthickness=0,
            borderwidth=0,
            height=height,
            wrap="word",
            insertbackground=FG,
            selectbackground=ACCENT_DIM,
            state="disabled",
            padx=10,
            pady=9,
        )
        self.text.pack(fill="both", expand=True)
        self.text.tag_configure("ok", foreground=OK)
        self.text.tag_configure("err", foreground=ERR)
        self.text.tag_configure("warn", foreground=WARN)
        self.text.tag_configure("info", foreground=INFO)
        self.text.tag_configure("dim", foreground=FG_DIM)
        self.text.tag_configure("head", foreground=FG, font=ui_font(9, True))

    def append(self, message: str, tag: str = "") -> None:
        prefix = {"ok": "  ✓ ", "err": "  × ", "warn": "  ! "}.get(tag, "  · ")
        self.text.configure(state="normal")
        self.text.insert("end", prefix + message + "\n", tag)
        self.text.see("end")
        self.text.configure(state="disabled")

    def contents(self) -> str:
        return self.text.get("1.0", "end")

    def clear(self) -> None:
        self.text.configure(state="normal")
        self.text.delete("1.0", "end")
        self.text.configure(state="disabled")
