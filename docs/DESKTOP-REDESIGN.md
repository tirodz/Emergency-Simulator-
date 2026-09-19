# Desktop redesign — v2.0.0

The desktop controller is rebuilt around Tauri 2 + Rust + HTML/CSS/JavaScript. The previous Tkinter/Python window is no longer the product UI.

## Visual direction

The redesign was based on the separate CMF Ringtone Tool frontend in tirodz/CMF-Ringtone-Tool/app/index.html, which is a Tauri desktop frontend with an undecorated window, translucent surfaces, backdrop-filter, rounded glass cards, soft highlights and compact controls.

It also follows public Liquid Glass principles from Apple: navigation and controls form a distinct functional layer, translucency is used to create hierarchy, and larger floating surfaces get more depth and lensing. The default Emergency Simulator accent is emerald green, with cyan and orange alternatives.

## Interaction contract

- UNAUTHORIZED is actionable: unlock the Android phone, accept the USB debugging prompt, then refresh.
- STOCK / UNPROVEN is explicit: a retail/rootless phone is not represented as ready.
- READY requires the controlled root/userdebug path and a detected CellBroadcast receiver.
- The Send button stays disabled until a target is actually ready.
- Send failures remain inside the application as a result card/toast instead of closing the process.
- The only exposed message class is ETWS TEST channel 4355; message text must begin with TEST.
- Success is claimed only after downstream Android CellBroadcast evidence.
- STOP / CANCEL stops pending controller work; it does not pretend to dismiss a displayed Android alert.
- The local transaction gate survives application restart.

## Build stack

- Tauri 2
- Rust
- HTML/CSS/JavaScript
- WebView2 on Windows
- bundled adb.exe + required DLLs
- bundled development-only Android injector JAR

The Java injector remains because Android's SmsCbMessage constructors are hidden framework APIs. It is a tiny device-side test mechanism, not the desktop UI or controller language.

Python research utilities from the earlier investigation remain only where they are useful as historical tooling. They are not part of the Windows desktop runtime.

## References

- CMF reference frontend: https://github.com/tirodz/CMF-Ringtone-Tool/blob/main/app/index.html
- Apple Liquid Glass: https://developer.apple.com/documentation/TechnologyOverviews/liquid-glass
- Apple Materials: https://developer.apple.com/design/human-interface-guidelines/materials
- Liquid Glass prompt gallery: https://liquidglassdesign.com/gallery/26930230-liquid-glass
