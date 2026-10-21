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

Use this option to select the phone as a microphone in Linux applications. The desktop AppImage is attached to the [latest Voxlink release](https://github.com/mrduhlol/Voxlink/releases/latest). You do not need to clone the repository or install build tools. Voxlink creates a user-session PipeWire/PulseAudio source only while it is running.

1. On Omarchy, confirm the user audio services are running:

   ```sh
   pactl info
   ```

   The server should be local and commonly reports `PulseAudio (on PipeWire ...)`. Omarchy normally has PipeWire and its PulseAudio compatibility service ready. Do not launch Voxlink with `sudo`.

   If `pactl` is unavailable or the PipeWire services are stopped, install/start them before continuing:

   ```sh
   sudo pacman -S --needed libpulse pipewire pipewire-pulse wireplumber
   systemctl --user enable --now pipewire pipewire-pulse wireplumber
   pactl info
   ```

2. Download [Voxlink_0.2.0_amd64.AppImage](https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink_0.2.0_amd64.AppImage) from the release page and save it in `~/Downloads`.

3. Make the downloaded AppImage executable. Open a terminal and run:

   ```sh
   chmod +x ~/Downloads/Voxlink_0.2.0_amd64.AppImage
   ```

4. Launch `Voxlink_0.2.0_amd64.AppImage` from Downloads. On its first launch, click **Add to app menu**. Voxlink copies itself to `~/Applications/Voxlink.AppImage` and creates a launcher for your user; this does not require administrator access.

5. After installation, press the **Super** key, search for **Voxlink**, and open it from the app menu. The downloaded copy can be deleted after the first launch because Voxlink installed its own copy under `~/Applications`.

6. In Voxlink, enter the PC's private Wi-Fi IPv4 address in **PC Wi-Fi address**. Find it with `ip -brief address`. Enter the phone's Wi-Fi IPv4 address in **Phone IP address** and leave the port at `4950`, unless you changed the sender port. Click **Start listening**, then start the Android sender. Voxlink changes from Connecting to Connected after a valid packet arrives; its level meter should move when you speak.

   In Discord, OBS, a browser, or another audio application, select **Voxlink Microphone** as the input. If the app was already open before the source appeared, reopen its audio-device list. Click **Stop** in Voxlink when done; the temporary input is removed.

On other Linux distributions, use their package manager to install PipeWire, the PulseAudio compatibility service, and `pactl`. Voxlink is built with Tauri and WebKitGTK; see [Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux) only if you choose to build from source.

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
