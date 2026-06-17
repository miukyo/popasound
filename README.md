![Popasound](static/icons/logo.png)

Popasound is a desktop app with an embedded web server and system tray icon. Browse, search, and play sound effects from [MyInstants](https://www.myinstants.com) — or upload your own MP3 files — all controllable from any device on your local network.

## Features

- **Library** — save sounds from MyInstants, upload custom MP3s, set per-sound volume, assign hotkeys
- **Search** — search MyInstants directly from the app
- **Web UI** — accessible from any device on your LAN (Alpine.js + Tailwind)
- **System tray** — runs in the background, left‑click to open browser, right‑click to quit
- **Global hotkeys** — bind keyboard shortcuts to any library sound (observes keys without blocking other apps)
- **Multiple simultaneous playback** — plays several sounds at once via rodio
- **Now‑playing highlights** — currently playing sounds are highlighted in the UI
- **QR code** — scan to open the web UI on your phone
- **Single binary** — all static assets embedded, no external files needed

## Usage

Run the binary — a tray icon appears. Double click the tray icon or open `http://localhost:6677` in your browser (or scan the QR code from the web UI).
