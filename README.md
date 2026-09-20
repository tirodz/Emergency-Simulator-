# Emergency Simulator

A Windows desktop laboratory console for controlled Android CellBroadcast testing.

> This application does not transmit cellular signals, inject RF, impersonate a carrier, or operate a cellular network. The controlled test path uses Android's own protected CellBroadcast machinery on a rooted/userdebug development target.

## v2.0 desktop rebuild

The Windows application has been rebuilt around:

- Tauri 2
- Rust
- HTML / CSS / JavaScript
- WebView2
- bundled adb.exe
- a bundled development-only Android test injector

The previous Tkinter/Python window is retired from the product path.

### The visual system

The interface is a real custom glass shell rather than native Tk widgets: translucent layered surfaces, blurred backgrounds, specular top edges, ambient pointer lighting, floating controls, luminous green readiness dots, a glowing primary action, crisp inline SVG icons, spacious typography and theme persistence.

Themes include Midnight, OLED and Light. Accents include Emerald, Cyan and Orange.

## Device states

The console deliberately distinguishes:

- UNAUTHORIZED — accept the USB debugging authorization prompt on the phone.
- STOCK / UNPROVEN — a retail/rootless phone is connected, but the protected injection path is not demonstrated there.
- READY — a controlled root/userdebug target with a detected CellBroadcast receiver.

A mere entry in adb devices is not treated as ready.

## Controlled alert path

~~~text
Windows Tauri console
        |
       ADB
        |
root/userdebug Android device
        |
AlertInjector
        |
protected Android CellBroadcast receiver
        |
CellBroadcast service
        |
system alert audio + UI
~~~

The only message class exposed by the controller is AOSP ETWS TEST channel 4355 (0x1103), and the controller rejects message bodies that do not begin with TEST.

## Stock-device boundary

A normal retail, unrooted Android phone is not claimed supported. The Android protected-broadcast boundary prevents an ordinary sender from simply impersonating the system CellBroadcast path.

A home Wi-Fi router can transport controller traffic, but it cannot become a Cell Broadcast Centre or make cellular towers transmit.

## Windows build

~~~powershell
npm install
npx tauri build --ci
~~~

The released desktop bundle is self-contained for ADB and the development injector. Rebuilding the injector source still requires an Android SDK/JDK.

## Project layout

~~~text
src/                       Tauri frontend
src-tauri/                 Rust/Tauri backend
android/alertinject/       controlled Android test injector
packaging/                 Windows packaging notes
docs/                      research + design notes
tools/                     historical/research utilities
~~~

The old Python desktop package is no longer the runtime for the Windows application.

## Safety

The project stays inside a controlled development boundary:

- no RF transmission
- no carrier network injection
- no modem access
- no rooting instructions
- no bootloader unlocking or firmware flashing
- no fake system-alert UI pretending to be Android
- Android downstream evidence determines whether a test actually reached the system alert UI


## Galaxy A35 device workspace

The Devices workspace is a dedicated screen rather than an Overview scroll target. It scans the bundled ADB server, shows each attached target and opens a detailed hardware profile. For a stock Galaxy A35, the app reads RAM, storage, battery level, display resolution, density, Android version and build identity over ADB, while model facts can be filled from Samsung's published specifications. Samsung documents the A35 as a 6.6-inch FHD+ Super AMOLED device with up to 120Hz, 6/8GB memory options, 128/256GB storage options, a 5,000mAh battery and Exynos 1380. 


## Device workspace

Click **Devices** to open a dedicated hardware workspace. The app scans ADB, shows authorization and capability state, and displays live read-only values from the selected Android device including RAM, storage, battery level, CPU/SoC, display resolution, density, Android version and build identity. Samsung Galaxy A35 model facts are shown from Samsung's published specifications when Android cannot expose them directly.