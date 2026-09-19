# Device imagery research

The desktop UI uses local PNG assets so device discovery remains fast and remains functional offline.

External projects investigated for future expansion include:

- PhoneMockup: an open-source phone mockup generator built with Next.js, React and Three.js, with multiple device frames and high-resolution export.
- Prestige: an open-source Tauri screenshot generator with a Samsung Galaxy S25 Ultra device frame.
- phone-specs-api: an open-source REST API that exposes a phone image URL backed by GSMArena content.

Emergency Simulator keeps the current hardware art local instead of making the application depend on a remote API. A future remote provider should be optional and should be reviewed for licensing and uptime before third-party product photography is bundled.

The current Samsung PNG is visual hardware reference art only; live device identity and capability are still obtained from ADB.
