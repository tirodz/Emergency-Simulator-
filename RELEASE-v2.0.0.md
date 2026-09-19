# Emergency Simulator 2.0.0

## Desktop rebuild

Emergency Simulator 2.0.0 is the Tauri desktop rebuild.

- Tauri 2 + Rust backend
- HTML/CSS/JavaScript glass frontend
- Custom frameless Windows shell
- Midnight, OLED and Light themes
- Emerald, Cyan and Orange accents
- Local Samsung PNG hardware preview
- ADB device inspection with explicit stock/controlled states
- Non-invasive device dry run
- Android Developer Options shortcut for stock setup
- Windows self-contained ADB resources
- NSIS installer

## Android boundary

The Galaxy A35 stock/user build is treated as a real diagnostic target, but the app does not claim protected CellBroadcast injection support there. The demonstrated genuine system-alert path remains the controlled root/userdebug development path.

## Build verification

GitHub Actions Windows build passed for commit 6be47fec4b9da6a6adc1c12ff90006b3c862acd4.

Standalone EXE:
- Size: 8,602,624 bytes
- SHA-256: 8D1CA2F82573D47C51AA948253635CB784D243546A7239C4454DEEF698299A34

Installer:
- SHA-256: CC5256A60C98A66AB298C25D4B26F45F8E98B70C6652E9EE2F1BC2DC7EEAE39E

The previous v1.1.0 GitHub release is no longer listed.