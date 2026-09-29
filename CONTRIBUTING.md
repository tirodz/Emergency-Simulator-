# Contributing

## Before changing code

Read the architecture and safety notes first. The project is a controlled test tool and does not transmit cellular signals or impersonate a carrier.

## Windows application

Install the Node dependencies and build with:

```powershell
npm install
npx tauri build --ci
```

The desktop backend is in `src-tauri/`. The frontend is in `src/`.

## Tests

Rust tests:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
```

Frontend and helper checks are kept under `tests/` and `tools/` where applicable.

When a change depends on Android behaviour, include the device/emulator version and the exact evidence used to verify it.

## Pull requests

Keep changes focused. Describe what changed, how it was tested, and anything that remains unverified.

Do not claim a stock-device capability from an emulator or userdebug result.
