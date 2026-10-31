# Voxlink desktop on Omarchy

## What runs where

`desktop/` contains the Tauri 2 + React/TypeScript application. Tauri commands validate the private IPv4 configuration and own the lifecycle of the receiver. The UDP reader runs on its own thread, passes bounded PCM frames to a separate microphone pipe writer thread, and reports live counters and levels to the UI. `receiver/src/lib.rs` contains the shared VXL1 packet decoder used by both the desktop app and the original WAV-file receiver.

Each Android datagram has a 12-byte header followed by mono signed 16-bit little-endian PCM:

| Offset | Size | Meaning |
| --- | ---: | --- |
| 0 | 4 | ASCII `VXL1` magic |
| 4 | 4 | Sequence number, unsigned big-endian |
| 8 | 4 | Sample rate in Hz, unsigned big-endian |
| 12 | remaining | PCM16 mono samples, little-endian |

The desktop binds audio only to the chosen PC Wi-Fi IPv4. With Phone IP empty it asks you to approve a phone that presents the pairing code, then accepts audio from that approved phone; an optional private Phone IP locks audio to that address. Invalid packets and out-of-order sequence numbers are dropped and counted. A bounded queue keeps the network reader independent from audio playback; overflow is counted in diagnostics.

## PipeWire microphone route

Voxlink uses `pipewire-pulse` to create a real **Voxlink Microphone** input (`voxlink_microphone`) with `module-pipe-source`. The app writes mono PCM to a private, nonblocking FIFO in the user runtime directory. The device is an `Audio/Source`, not a speaker output or speaker monitor. A streaming resampler converts the phone's 44.1 kHz fallback to the microphone's fixed 48 kHz format without losing continuity between packets.

The input exists while the listener is running. Select **Voxlink Microphone** under **Input** in the system audio panel and in Discord, OBS, or your browser. Stop removes the app-owned source and FIFO. The app does not edit system audio configuration or require root privileges.

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

## Connect your phone to the desktop

1. Connect both devices to the same trusted Wi-Fi network.
2. On the PC, choose its **PC Wi-Fi address**, leave **Phone IP address** empty, and click **Start listening**.
3. On Android, tap **Find my computer**. If several PCs appear, choose the one whose pairing code matches the desktop. Or tap **Scan QR** and scan the desktop's QR code; camera permission is only needed for scanning.
4. Tap **Start streaming**, allow microphone access, then choose **Allow pairing** in the confirmation popup on the PC. The phone waits for that approval before capturing or sending audio.
5. Once the PC shows **Receiving audio**, select **Voxlink Microphone** in your call or recording app.

If discovery cannot find the PC, enter the PC's Wi-Fi IPv4 address, audio port (default `4950`), and six-digit pairing code manually on the phone. `localhost` and `127.0.0.1` refer to the phone itself, not the PC. You no longer need to find the phone's IP address for normal setup.

The PC keeps the same pairing code across launches. To change it, stop listening and click **Refresh code**. Reconnect the phone using Find, QR, or the new code. Discovery uses UDP `4951`; audio uses UDP `4950` by default. Allow both on your trusted LAN, and check guest-network/client isolation if neither Find nor QR connects. Port `4951` is reserved for discovery. Pairing requests wait up to 45 seconds on the phone; **Reject** or a timeout sends no microphone audio. A **Paired · no audio** label on the PC means approval succeeded but audio has not arrived; it is separate from **Receiving audio**.

### Manual IP lock and legacy receiver

To restrict the desktop to a known phone, enter its private Wi-Fi IPv4 address in **Phone IP address** before starting. The phone's Connection tools can display that address. Audio from every other address is filtered. For this mode, or the standalone WAV receiver, enter the PC address and audio port on Android and leave the pairing code empty. The standalone receiver has no discovery or QR support.

Pairing selects an allowed source IP; it is not cryptographic authentication. Discovery exposes the code on the LAN, and audio is unencrypted. Use a trusted network and do not forward either UDP port to the internet.


## Diagnostics and troubleshooting

- **No Wi-Fi address appears:** check `ip -brief address`; choose the private IPv4 on the active Wi-Fi interface. Do not bind to `127.0.0.1` or a public address.
- **No pairing popup on a PC with UFW enabled:** allow both Voxlink UDP ports from your trusted Wi-Fi subnet. For example, if `ip -4 address` shows `192.168.0.108/24` on `wlp2s0`, run `sudo ufw allow in on wlp2s0 from 192.168.0.0/24 to any port 4950:4951 proto udp`. Substitute your own interface and subnet. A blocked discovery port prevents the approval popup; a blocked audio port leaves an approved phone paired without audio.
- **Connecting, but no packets arrive:** confirm both devices are on the same Wi-Fi, the Android app is in the foreground, and UDP `4950` and `4951` are allowed on the trusted LAN. In auto mode, use Find or QR again to refresh a stale pairing code. In manual mode, verify the phone IP matches exactly.
- **Virtual microphone unavailable:** run `pactl info` and ensure `pipewire-pulse` is active. Install the `libpulse`/PipeWire packages above and restart the user services if needed. Inspect `pactl list short sinks` and `pactl list short sources`.
- **Source is missing from an application:** start Voxlink first, then reopen the application's device selector. The source is named `Voxlink Microphone` and the PulseAudio source is `voxlink_microphone`.
- **No meter activity or crackling:** inspect packet loss and audio queue drops in Voxlink. Move both devices closer to the Wi-Fi access point, avoid congested networks, and close applications that compete for audio. Restart the listener after correcting the network or audio service.
- **Stale device after a forced crash:** inspect `pactl list short modules`; unload only the orphaned module whose arguments name `voxlink_microphone`. An older release may leave `module-null-sink` with `sink_name=voxlink_mic`; close that older app first, then remove its orphaned module with `pactl unload-module MODULE_ID` as your regular user.

For a source capture check while Voxlink is running, use:

```sh
pactl list short sources
parec --device=voxlink_microphone --format=s16le --rate=48000 --channels=1 /tmp/voxlink-capture.raw
```

Speak into the phone for a few seconds, stop `parec` with Ctrl+C, and check that `/tmp/voxlink-capture.raw` is non-empty and changes when the phone is silent versus speaking.

## Privacy and network scope

Audio processing stays on the PC and local network. Pairing authorizes a source IP and discovery reveals the code on the LAN; it is not cryptographic authentication. Audio has no encryption. Anyone who can spoof the allowed phone IP may send packets. Do not expose UDP ports `4950` or `4951` to the public internet or an untrusted network.

## Development checks

From the repository root:

```sh
npm --prefix desktop ci
npm --prefix desktop run build
cargo test --locked --manifest-path receiver/Cargo.toml
cargo test --locked --manifest-path desktop/src-tauri/Cargo.toml
```

The Rust tests cover pairing packet formats, code persistence, authorization, live UDP discovery and audio filtering, configuration validation, packet parsing, and level calculations. A hardware test still needs an Android phone on the same Wi-Fi and an application such as OBS to verify actual microphone selection and capture.
