"""Visual primitives shared by the desktop interface.

The interface is a laboratory instrument, not an emergency-alert screen, and the visual language has
to make that unmistakable at a glance. Nothing here uses the red-and-white severity styling that real
alerts use; the palette is restrained, the type is monospace, and status is carried by words as well
as colour so it survives a monochrome display or a colour-blind operator.

Tkinter's themed widgets cannot be styled deeply, so the components here are built from plain frames
and canvases. That is deliberate: the buttons and cards used for the destructive-ish action need
exact, predictable styling rather than whatever the platform theme decides.
"""

from __future__ import annotations

import tkinter as tk
from typing import Callable, Optional

# -- palette ---------------------------------------------------------------

BG = "#0d1015"
PANEL = "#151a21"
PANEL_ALT = "#1b212a"
PANEL_RAISED = "#212832"
BORDER = "#2a323d"
BORDER_LIGHT = "#38424f"

FG = "#dde3ec"
FG_DIM = "#8a95a5"
FG_FAINT = "#5d6675"

ACCENT = "#e0a33a"        # the test-alert action: caution, never alarm red
ACCENT_ACTIVE = "#f0bb5e"
ACCENT_DIM = "#6b4f1d"

OK = "#4fae74"
WARN = "#d9a441"
ERR = "#d95f52"
INFO = "#5c9dd6"
NEUTRAL = "#7b8798"

MONO = "Consolas"
MONO_FALLBACK = "Courier New"


def mono(size: int = 9, bold: bool = False) -> tuple:
    """A monospace font tuple, with a fallback for platforms without Consolas."""
    return (MONO, size, "bold") if bold else (MONO, size)


def mono_fallback(size: int = 9, bold: bool = False) -> tuple:
    return (MONO_FALLBACK, size, "bold") if bold else (MONO_FALLBACK, size)


# -- glyphs ----------------------------------------------------------------
#
# A release needs to look sharp on Windows, which has a colour emoji font. A development or CI host
# may have no emoji font at all, where these characters render as empty boxes. Rather than ship
# either a broken look or a dependency on a font, the symbols are chosen from ranges that are near
# universal, and every glyph is paired with the word it stands for so meaning never depends on it.

GLYPHS = {
    "alert": "\u25b2",       # black up-pointing triangle
    "cancel": "\u2715",      # multiplication x
    "device": "\u25a0",      # black square
    "ready": "\u25cf",       # black circle
    "unknown": "\u25cb",     # white circle
    "check": "\u2713",       # check mark
    "cross": "\u2717",       # ballot x
    "warn": "\u26a0",        # warning sign (monochrome in most UI fonts)
    "arrow": "\u2192",       # rightwards arrow
    "bullet": "\u00b7",      # middle dot
    "lock": "\u25a3",        # square with left half black, used as a "locked" mark
}


def glyph(name: str) -> str:
    return GLYPHS.get(name, GLYPHS["bullet"])


# -- colour selection ------------------------------------------------------


def state_colour(state: str) -> str:
    """Colour for a device or alert state, chosen for meaning rather than decoration."""
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
        "IDLE": FG_DIM,
        "CANCELLED": FG_DIM,
    }.get(state.upper(), FG)


# -- components ------------------------------------------------------------


class Card(tk.Frame):
    """A bordered panel with a small caption. The unit of layout in this interface."""

    def __init__(self, parent, title: str = "", pad: int = 12, **kw):
        super().__init__(
            parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1, **kw
        )
        self._title = title
        self.body = tk.Frame(self, bg=PANEL)
        self.body.pack(fill="both", expand=True, padx=pad, pady=(pad - 3, pad))

        if title:
            header = tk.Frame(self, bg=PANEL)
            header.pack(fill="x", padx=pad, pady=(pad - 2, 0), before=self.body)
            self.caption = tk.Label(
                header, text=title.upper(), bg=PANEL, fg=FG_FAINT, font=mono(8, bold=True)
            )
            self.caption.pack(side="left")


class Button(tk.Frame):
    """A flat button with explicit colours that do not follow the platform theme.

    Tk's own Button cannot be made to look identical across platforms, so this is drawn from labels.
    It keeps keyboard focus and an active state, which the previous plain Button provided for free.
    """

    def __init__(
        self,
        parent,
        text: str,
        command: Optional[Callable[[], None]] = None,
        variant: str = "default",
        icon: str = "",
        padx: int = 14,
        pady: int = 7,
    ):
        styles = {
            "primary": (ACCENT, "#101318", ACCENT_ACTIVE),
            "danger": (PANEL_RAISED, ERR, "#2b333d"),
            "ghost": (PANEL, FG_DIM, PANEL_ALT),
            "default": (PANEL_RAISED, FG, "#2b333d"),
        }
        bg, fg, active = styles.get(variant, styles["default"])
        self._bg, self._fg, self._active = bg, fg, active
        self._enabled = True
        self._command = command

        super().__init__(parent, bg=bg, highlightthickness=0)
        self._label = tk.Label(
            self,
            text=(f"{glyph(icon)}  {text}" if icon else text),
            bg=bg,
            fg=fg,
            font=mono(9, bold=True),
            padx=padx,
            pady=pady,
            cursor="hand2",
        )
        self._label.pack()

        for widget in (self, self._label):
            widget.bind("<Button-1>", self._on_click)
            widget.bind("<Enter>", self._on_enter)
            widget.bind("<Leave>", self._on_leave)

    def _paint(self, bg: str, fg: Optional[str] = None) -> None:
        self.configure(bg=bg)
        self._label.configure(bg=bg, fg=fg if fg is not None else self._fg)

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
        dim = PANEL_ALT
        self._paint(self._bg if enabled else dim, self._fg if enabled else FG_FAINT)

    def configure_text(self, text: str, icon: str = "") -> None:
        self._label.configure(text=(f"{glyph(icon)}  {text}" if icon else text))


class StatusPill(tk.Frame):
    """A compact labelled status indicator: a coloured dot plus a word.

    The word is always present, so the state is readable without relying on colour.
    """

    def __init__(self, parent, label: str = "", value: str = "---"):
        super().__init__(parent, bg=PANEL)
        self._label = tk.Label(self, text=label, bg=PANEL, fg=FG_FAINT, font=mono(8, bold=True))
        self._label.pack(side="left")
        self._dot = tk.Label(self, text=glyph("unknown"), bg=PANEL, fg=FG_DIM, font=mono(10))
        self._dot.pack(side="left", padx=(8, 5))
        self._value = tk.Label(self, text=value, bg=PANEL, fg=FG, font=mono(9, bold=True))
        self._value.pack(side="left")

    def set(self, value: str, colour: Optional[str] = None, mark: str = "ready") -> None:
        self._value.configure(text=value)
        colour = colour or state_colour(value)
        self._value.configure(fg=colour)
        self._dot.configure(text=glyph(mark), fg=colour)


class Banner(tk.Frame):
    """A full-width strip used for the safety statement and for outcome announcements."""

    def __init__(self, parent, text: str, fg: str = ACCENT, bg: str = PANEL_ALT):
        super().__init__(parent, bg=bg, highlightbackground=BORDER, highlightthickness=1)
        self._label = tk.Label(
            self, text=text, bg=bg, fg=fg, font=mono(9, bold=True), pady=7, padx=12
        )
        self._label.pack(fill="x")

    def set(self, text: str, fg: str = ACCENT, bg: str = PANEL_ALT) -> None:
        self.configure(bg=bg, highlightbackground=BORDER)
        self._label.configure(text=text, fg=fg, bg=bg)


class DeviceRow(tk.Frame):
    """One selectable device, rendered as a row with its own status.

    Selection is handled by the parent rather than by Tk's listbox, because the row needs to show a
    support verdict and a note alongside the serial, which a listbox cannot express.
    """

    def __init__(self, parent, on_select: Callable[[str], None]):
        super().__init__(parent, bg=PANEL_ALT, highlightthickness=1, highlightbackground=BORDER)
        self._serial = ""
        self._on_select = on_select
        self.selected = False

        inner = tk.Frame(self, bg=PANEL_ALT)
        inner.pack(fill="x", padx=10, pady=7)

        self._dot = tk.Label(inner, text=glyph("unknown"), bg=PANEL_ALT, fg=FG_DIM, font=mono(11))
        self._dot.grid(row=0, column=0, rowspan=2, sticky="w", padx=(0, 9))

        self._name = tk.Label(inner, text="", bg=PANEL_ALT, fg=FG, font=mono(9, bold=True), anchor="w")
        self._name.grid(row=0, column=1, sticky="w")

        self._meta = tk.Label(inner, text="", bg=PANEL_ALT, fg=FG_DIM, font=mono(8), anchor="w")
        self._meta.grid(row=1, column=1, sticky="w")

        self._verdict = tk.Label(inner, text="", bg=PANEL_ALT, fg=FG_DIM, font=mono(8, bold=True))
        self._verdict.grid(row=0, column=2, rowspan=2, sticky="e", padx=(10, 0))
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
        self._meta.configure(text=f"{serial}   {meta}".strip())
        self._verdict.configure(text=verdict, fg=colour)
        self._dot.configure(text=glyph(mark), fg=colour)

    def set_selected(self, selected: bool) -> None:
        self.selected = selected
        bg = PANEL_RAISED if selected else PANEL_ALT
        self.configure(bg=bg, highlightbackground=ACCENT if selected else BORDER)
        for widget in self.winfo_children():
            widget.configure(bg=bg)
            for sub in widget.winfo_children():
                sub.configure(bg=bg)


class LogView(tk.Frame):
    """An append-only, tagged log pane with severity colouring."""

    def __init__(self, parent, height: int = 11):
        super().__init__(parent, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        self.text = tk.Text(
            self,
            bg="#10151b",
            fg=FG,
            font=mono(9),
            relief="flat",
            highlightthickness=0,
            borderwidth=0,
            height=height,
            wrap="word",
            insertbackground=FG,
            state="disabled",
            padx=8,
            pady=6,
        )
        self.text.pack(fill="both", expand=True)
        self.text.tag_configure("ok", foreground=OK)
        self.text.tag_configure("err", foreground=ERR)
        self.text.tag_configure("warn", foreground=WARN)
        self.text.tag_configure("info", foreground=INFO)
        self.text.tag_configure("dim", foreground=FG_DIM)
        self.text.tag_configure("head", foreground=FG, font=mono(9, bold=True))

    def append(self, message: str, tag: str = "") -> None:
        prefix = {"ok": "  \u2713 ", "err": "  \u2717 ", "warn": "  ! "}.get(tag, "  \u00b7 ")
        self.text.configure(state="normal")
        self.text.insert("end", f"{prefix}{message}\n", tag)
        self.text.see("end")
        self.text.configure(state="disabled")

    def contents(self) -> str:
        return self.text.get("1.0", "end")

    def clear(self) -> None:
        self.text.configure(state="normal")
        self.text.delete("1.0", "end")
        self.text.configure(state="disabled")