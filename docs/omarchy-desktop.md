# Voxlink desktop on Omarchy

## What runs where

`desktop/` contains the Tauri 2 + React/TypeScript application. Tauri commands validate the private IPv4 configuration and own the lifecycle of the receiver. The UDP reader runs on its own thread, passes bounded PCM frames to a separate PulseAudio client thread, and reports live counters and levels to the UI. `receiver/src/lib.rs` contains the shared VXL1 packet decoder used by both the desktop app and the original WAV-file receiver.

Each Android datagram has a 12-byte header followed by mono signed 16-bit little-endian PCM:

| Offset | Size | Meaning |
| --- | ---: | --- |
| 0 | 4 | ASCII `VXL1` magic |
| 4 | 4 | Sequence number, unsigned big-endian |
| 8 | 4 | Sample rate in Hz, unsigned big-endian |
| 12 | remaining | PCM16 mono samples, little-endian |

The desktop binds only to the chosen PC Wi-Fi IPv4 and accepts packets only from the configured private phone IPv4. Invalid packets and out-of-order sequence numbers are dropped and counted. A bounded queue keeps the network reader independent from audio playback; overflow is counted in diagnostics.

## PipeWire microphone route

Voxlink talks to the user's PulseAudio socket through `pipewire-pulse`. On Start it creates (or reuses) a PulseAudio `module-null-sink` named `voxlink_mic`, then plays the phone PCM into that sink. PipeWire exposes the sink's monitor source as **Voxlink Microphone** (`voxlink_mic.monitor`), which is the selectable input seen by Discord, OBS, browsers, and other PulseAudio clients. Stop closes playback and unloads only the sink module created by Voxlink. It does not edit system audio configuration and does not need root privileges.

The monitor source exists only while the Voxlink listener is running. Select **Voxlink Microphone** in the target application's input device list. Keep the Voxlink window open while using the source.

## Install the released AppImage

Download [Voxlink_0.2.0_amd64.AppImage](https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink_0.2.0_amd64.AppImage) from the [latest release](https://github.com/mrduhlol/Voxlink/releases/latest) and save it in `~/Downloads`. Make it executable and launch it:

```sh
chmod +x ~/Downloads/Voxlink_0.2.0_amd64.AppImage
~/Downloads/Voxlink_0.2.0_amd64.AppImage
```

On first launch, click **Add to app menu**. Voxlink copies itself to `~/Applications/Voxlink.AppImage` and registers a launcher in your user profile. Later, press **Super**, search for Voxlink, and launch it from the app menu. This install is per-user and does not require root access or a source checkout.

Voxlink uses the logged-in user's PipeWire/PulseAudio session. Check it with:

```sh
pactl info
wpctl status
```

`pactl info` should show a local PulseAudio server, commonly `PulseAudio (on PipeWire ...)`. If the audio tools or services are missing, install/start them:

```sh
sudo pacman -S --needed libpulse pipewire pipewire-pulse wireplumber
systemctl --user enable --now pipewire pipewire-pulse wireplumber
```

Do not run Voxlink with `sudo`; root would not have access to the logged-in user's audio server.

## Build from source (developers)

Most users should install the released AppImage above. For development, install the Tauri/Linux build libraries and tools:

```sh
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool libpulse pipewire pipewire-pulse wireplumber nodejs npm
```

Install Rust with [rustup](https://rustup.rs/) if `cargo` is unavailable. From the repository root:

```sh
cd desktop
npm ci
npm run tauri -- dev
```

For a release build:

```sh
npm run tauri -- build
```

Tauri writes the AppImage under `desktop/src-tauri/target/release/bundle/appimage/`. Launch the `.AppImage` it produces from the file manager or terminal. It runs as the current user and uses that user's PipeWire/PulseAudio session.

To connect a phone:

1. On the PC, get its Wi-Fi address with `ip -brief address` and enter that address as **PC Wi-Fi address**.
2. Enter the phone's Wi-Fi IPv4 address as **Phone IP address**. Both addresses must be on the same trusted LAN.
3. Keep UDP port `4950` unless the Android sender is configured with a different port. If a firewall is enabled, allow UDP `4950` from the trusted LAN only.
4. Click **Start listening**, then start the Android sender. Voxlink reports Connecting while waiting and Connected only after a valid packet arrives.
5. Choose **Voxlink Microphone** in Discord, OBS, or the browser's microphone selector. Speak into the phone and verify that the live input meter moves.
6. Click **Stop** when finished; Voxlink removes the temporary input source.

The Android sender currently sends 48 kHz mono PCM when available and falls back to 44.1 kHz. The desktop uses the packet's declared sample rate.

## Diagnostics and troubleshooting

- **No Wi-Fi address appears:** check `ip -brief address`; choose the private IPv4 on the active Wi-Fi interface. Do not bind to `127.0.0.1` or a public address.
- **Connecting, but no packets arrive:** confirm both devices are on the same Wi-Fi, the phone IP matches exactly, the Android app is running in the foreground, and UDP `4950` is allowed on the trusted LAN. Voxlink filters packets from all other source addresses.
- **Virtual microphone unavailable:** run `pactl info` and ensure `pipewire-pulse` is active. Install the `libpulse`/PipeWire packages above and restart the user services if needed. Inspect `pactl list short sinks` and `pactl list short sources`.
- **Source is missing from an application:** start Voxlink first, then reopen the application's device selector. The source is named `Voxlink Microphone` and the PulseAudio source is `voxlink_mic.monitor`.
- **No meter activity or crackling:** inspect packet loss and audio queue drops in Voxlink. Move both devices closer to the Wi-Fi access point, avoid congested networks, and close applications that compete for audio. Restart the listener after correcting the network or audio service.
- **Stale sink after a forced crash:** inspect `pactl list short modules`; if an orphaned `module-null-sink` has argument `sink_name=voxlink_mic`, unload that module with `pactl unload-module MODULE_ID` as your regular user.

For a source capture check while Voxlink is running, use:

```sh
pactl list short sources
parec --device=voxlink_mic.monitor --format=s16le --rate=48000 --channels=1 /tmp/voxlink-capture.raw
```

Speak into the phone for a few seconds, stop `parec` with Ctrl+C, and check that `/tmp/voxlink-capture.raw` is non-empty and changes when the phone is silent versus speaking.

## Privacy and network scope

Audio processing stays on the PC and local network. The current UDP prototype has no authentication or encryption. Anyone who can spoof the configured phone IP on the trusted LAN may send audio packets; do not expose UDP port `4950` to the public internet or an untrusted network. Voxlink validates packet structure and filters by the configured phone IP, but this is not cryptographic protection.

## Development checks

From the repository root:

```sh
npm --prefix desktop ci
npm --prefix desktop run build
cargo test --locked --manifest-path receiver/Cargo.toml
cargo test --locked --manifest-path desktop/src-tauri/Cargo.toml
```

The Rust tests cover packet parsing and level calculations. A hardware test still needs an Android phone on the same Wi-Fi and an application such as OBS to verify actual microphone selection and capture.
