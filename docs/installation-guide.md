# Voxlink installation guide

This guide walks through installing the Android sender and setting up a receiving computer on Windows or Linux. Voxlink currently has two Linux receiving options:

- **Voxlink Desktop** creates a temporary PipeWire/PulseAudio microphone input. This is the option for Discord, OBS, browsers, and calls. The desktop app is currently Linux-only.
- **Voxlink Receiver** saves the incoming stream to a WAV file. It works on Linux and can be built from source on Windows.

There is no iPhone app or Windows virtual-microphone app in this version. Keep the phone and receiving computer on the same trusted Wi-Fi network. The UDP stream is not encrypted or authenticated; do not expose port `4950` to the public internet.

## 1. Install Voxlink on an Android phone

1. On the phone, open the [latest Voxlink release](https://github.com/mrduhlol/Voxlink/releases/latest) and download **Voxlink-android-debug.apk**. You can also use the [direct APK download](https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink-android-debug.apk).
2. Open the downloaded APK from Downloads. If Android blocks the installation, open the prompt for **Install unknown apps** and allow the browser or file manager you used, then retry. Turn that permission off again if you prefer after installing.
3. Connect the phone to the same trusted Wi-Fi network as the receiving computer. Do not use a guest network that isolates connected devices.
4. Leave the sender stopped until the receiving computer is ready. You will enter the computer's Wi-Fi IPv4 address and UDP port `4950` in the app.
5. When ready, tap **Start** and grant microphone permission. Keep Voxlink in the foreground while streaming. Tap **Stop** when finished.

Voxlink requests microphone access because it captures audio locally on the phone and sends it to the configured computer. It does not upload the audio to a cloud service.

## 2. Windows receiver (WAV recording)

Windows can run the standalone WAV receiver built from source. This receives the phone stream and writes `recordings\received-audio.wav`; it does not create a virtual microphone for Windows applications.

1. Install Rust using the official [rustup installer](https://rustup.rs/). In the installer, keep the default MSVC toolchain. If prompted, install the Microsoft C++ Build Tools.
2. Install Git for Windows if it is not already installed, then open PowerShell and clone the repository:

   ```powershell
   git clone https://github.com/mrduhlol/Voxlink.git
   cd Voxlink
   ```

   Alternatively, download and extract the repository ZIP from GitHub, then open PowerShell in the extracted `Voxlink` folder.

3. Find the computer's Wi-Fi IPv4 address:

   ```powershell
   ipconfig
   ```

   Under **Wireless LAN adapter Wi-Fi**, copy the **IPv4 Address** (for example `192.168.1.20`). Do not use the Default Gateway address.

4. Build the receiver:

   ```powershell
   cargo build --release --locked --manifest-path receiver/Cargo.toml
   ```

5. Start it, replacing the example IP with the Wi-Fi address from step 3:

   ```powershell
   New-Item -ItemType Directory -Force recordings | Out-Null
   .\receiver\target\release\voxlink-receiver.exe --bind 192.168.1.20 --port 4950 --output recordings\received-audio.wav
   ```

6. If Windows Defender Firewall asks, allow the receiver on **Private networks** only. If there is no prompt and the phone cannot connect, add an inbound UDP rule for local port `4950` scoped to your private network. Do not allow it on Public networks.
7. On the Android sender, enter the Windows Wi-Fi IPv4 address and port `4950`, then tap **Start**. The receiver terminal should report the incoming sample rate.
8. To stop and finish the WAV file, press **Ctrl+C** in the receiver terminal. The recording will be in `recordings\received-audio.wav`; open it with a WAV-compatible audio player.

## 3. Linux receiver (WAV recording)

This option is useful when you only need a recording file, rather than a selectable microphone.

1. Download the [latest Linux receiver archive](https://github.com/mrduhlol/Voxlink/releases/latest/download/voxlink-receiver-linux-x86_64.tar.gz). It is for 64-bit x86 Linux. To build from source instead, install Rust from [rustup](https://rustup.rs/) and clone the repository as shown above.
2. Extract the archive and make the receiver executable:

   ```sh
   tar -xzf voxlink-receiver-linux-x86_64.tar.gz
   chmod +x voxlink-receiver
   ```

3. Find the receiving computer's Wi-Fi address:

   ```sh
   ip -brief address
   ```

   Use the private IPv4 shown on the active Wi-Fi interface, often named `wlan0` or `wlp...`.

4. Start the receiver, replacing the sample address:

   ```sh
   mkdir -p recordings
   ./voxlink-receiver --bind 192.168.1.20 --port 4950 --output recordings/received-audio.wav
   ```

   To build the receiver from a repository checkout, run `cargo build --release --locked --manifest-path receiver/Cargo.toml` and launch `./receiver/target/release/voxlink-receiver` with the same arguments.

5. If a firewall is active, allow inbound UDP `4950` only from the trusted local network.
6. Enter the computer's Wi-Fi IPv4 address and port `4950` in the Android app, then tap **Start**.
7. When finished, press **Ctrl+C** in the receiver terminal. The completed recording is at `recordings/received-audio.wav`.

## 4. Omarchy desktop microphone

Use this option to select the phone as a microphone in Linux applications. It creates a user-session PipeWire/PulseAudio source only while Voxlink is running.

1. Confirm PipeWire's PulseAudio compatibility service is available:

   ```sh
   pactl info
   ```

   The server should be local and commonly reports `PulseAudio (on PipeWire ...)`. If the tools are missing on Omarchy, install the packages in step 2. Do not launch Voxlink with `sudo`.

2. Install build and audio dependencies on Omarchy:

   ```sh
   sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool libpulse pipewire pipewire-pulse wireplumber nodejs npm
   ```

   Install Rust with [rustup](https://rustup.rs/) if `cargo` is not available. On other Linux distributions, use the matching Tauri/WebKitGTK development packages; see the [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

3. Enable the user audio services if they are not already running:

   ```sh
   systemctl --user enable --now pipewire pipewire-pulse wireplumber
   pactl info
   ```

4. Build and launch the desktop app from the repository root:

   ```sh
   cd desktop
   npm ci
   npm run tauri -- build
   ```

   The AppImage is created under `desktop/src-tauri/target/release/bundle/appimage/`. Launch the `.AppImage` from the file manager, or from a terminal. For development, use `npm run tauri -- dev` instead of the build command.

5. In Voxlink, enter the PC's private Wi-Fi IPv4 address in **PC Wi-Fi address**. Find it with `ip -brief address`. Enter the phone's Wi-Fi IPv4 address in **Phone IP address** and leave the port at `4950`, unless you changed the sender port.
6. Click **Start listening**. The app should show the listener ready and the temporary **Voxlink Microphone** input. Start the Android sender. Voxlink changes from Connecting to Connected after a valid packet arrives; its level meter should move when you speak.
7. In Discord, OBS, a browser, or another audio application, select **Voxlink Microphone** as the input. If the app was already open before the source appeared, reopen its audio-device list.
8. Click **Stop** in Voxlink when done. The temporary input is removed. Start Voxlink again before trying to select it in another session.

### Omarchy quick checks

```sh
pactl list short sinks
pactl list short sources
```

While Voxlink is running, the source should be named `voxlink_mic.monitor`. If it is missing, check `pactl info`, confirm the user `pipewire-pulse` service is active, then stop and restart Voxlink. Full troubleshooting is in [omarchy-desktop.md](omarchy-desktop.md).

## Troubleshooting common connection problems

- Verify that the phone and computer are on the same non-guest Wi-Fi network and that client/AP isolation is disabled.
- Enter the computer's IPv4 address in the Android app, not its gateway address. Enter that same computer address in Voxlink's PC address field when using the desktop app.
- Start the receiver or desktop listener before starting the phone sender. Use UDP port `4950` at both ends.
- Allow UDP `4950` through the computer firewall only on the trusted private network.
- Keep the Android app in the foreground. Android battery optimization can stop background capture.
- For the Omarchy virtual microphone, verify `pactl info` works for the logged-in user and select `Voxlink Microphone` after starting the listener.

Audio is sent as mono PCM over UDP on the local network. The current protocol has no encryption or authentication; use a trusted network and never forward the receiver port to the public internet.
