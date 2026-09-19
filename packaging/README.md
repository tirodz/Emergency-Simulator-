# Windows packaging

The desktop application is bundled with Tauri 2 as a per-user NSIS installer.

The GitHub workflow stages Google's Android Platform Tools and packages only the runtime files used by Emergency Simulator:

- adb.exe
- AdbWinApi.dll
- AdbWinUsbApi.dll

The controlled Android injector JAR is bundled as a Tauri resource.

Local build after installing Node.js, Rust and the Windows WebView2 runtime:

~~~powershell
npm install
npx tauri build --ci
~~~

The NSIS installer is produced under src-tauri/target/release/bundle/nsis/.
