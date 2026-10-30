# Voxlink v0.2.0

Voxlink streams mono microphone audio from Android to Linux over a trusted local Wi-Fi network. The Omarchy desktop app routes the live stream into a PipeWire/PulseAudio virtual microphone, and the standalone Rust receiver can save it to a WAV file.

## Downloads

<a href="https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink-android-debug.apk"><img alt="Download the Android APK" src="https://img.shields.io/badge/Download-Android%20APK-3DDC84?style=for-the-badge&logo=android&logoColor=white"></a>

Visit the [Voxlink download website](https://mrduhlol.github.io/Voxlink/) for the latest app downloads and setup steps.

<a href="https://github.com/mrduhlol/Voxlink/releases/latest/download/voxlink-receiver-linux-x86_64.tar.gz"><img alt="Download Linux receiver" src="https://img.shields.io/badge/Download-Linux_x86__64-0078D4?logo=linux&logoColor=white"></a>
<a href="https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink_0.2.0_amd64.AppImage"><img alt="Download Omarchy desktop app" src="https://img.shields.io/badge/Download-Omarchy_AppImage-8A2BE2?logo=linux&logoColor=white"></a>

Text link: [Voxlink installation guide](docs/installation-guide.md) — step-by-step setup for Android phones, Windows, Linux, and Omarchy.

The Android download is a debug APK; Android may ask you to allow installation from your browser or file manager. The Linux archive is the standalone WAV receiver. The Omarchy AppImage is the real-time desktop microphone app. On first launch, click **Add to app menu** to add Voxlink to your user app launcher without installing system packages or requiring administrator access.

## Connect your phone to the desktop

1. Connect both devices to the same trusted Wi-Fi network.
2. On the PC, choose its **PC Wi-Fi address**, leave **Phone IP address** empty, and click **Start listening**.
3. On Android, tap **Find my computer**. If several PCs appear, choose the one whose pairing code matches the desktop. Or tap **Scan QR** and scan the desktop's QR code; camera permission is only needed for scanning.
4. Tap **Start streaming**, allow microphone access, and select **Voxlink Microphone** in your call or recording app.

If discovery cannot find the PC, enter the PC's Wi-Fi IPv4 address, audio port (default `4950`), and six-digit pairing code manually on the phone. `localhost` and `127.0.0.1` refer to the phone itself, not the PC. You no longer need to find the phone's IP address for normal setup.

The PC keeps the same pairing code across launches. To change it, stop listening and click **Refresh code**. Reconnect the phone using Find, QR, or the new code. Discovery uses UDP `4951`; audio uses UDP `4950` by default. Allow both on your trusted LAN, and check guest-network/client isolation if neither Find nor QR connects. Port `4951` is reserved for discovery.

### Manual IP lock and legacy receiver

To restrict the desktop to a known phone, enter its private Wi-Fi IPv4 address in **Phone IP address** before starting. The phone's Connection tools can display that address. Audio from every other address is filtered. For this mode, or the standalone WAV receiver, enter the PC address and audio port on Android and leave the pairing code empty. The standalone receiver has no discovery or QR support.

Pairing selects an allowed source IP; it is not cryptographic authentication. Discovery exposes the code on the LAN, and audio is unencrypted. Use a trusted network and do not forward either UDP port to the internet.

## Omarchy desktop microphone

The Tauri desktop app creates a temporary **Voxlink Microphone** PipeWire/PulseAudio input and routes live Android audio into it. Download and launch the [Omarchy desktop AppImage](https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink_0.2.0_amd64.AppImage); on first launch, choose **Add to app menu**. Then search for Voxlink from the app launcher, start listening, and select the input in Discord, OBS, a browser, or another recording application. It displays the actual phone connection state, input level, sample rate, packet loss, and audio routing errors.

See [Omarchy desktop setup, build, launch, and troubleshooting](docs/omarchy-desktop.md) for prerequisites and complete instructions. The original standalone WAV receiver and Android sender instructions are below.

## Install and run on Linux

The Linux program is the receiver. It runs on your Linux computer and saves incoming audio as a WAV file; the Android app sends audio to it. Both devices must be connected to the same trusted Wi-Fi network. The prebuilt receiver download is for 64-bit x86 Linux (`x86_64`).

### Install the prebuilt receiver

1. Download the [latest Linux receiver archive](https://github.com/mrduhlol/Voxlink/releases/latest/download/voxlink-receiver-linux-x86_64.tar.gz). If there is no published versioned release yet, use the source-build instructions below.
2. Extract it and make the program executable:

   ```sh
   tar -xzf voxlink-receiver-linux-x86_64.tar.gz
   chmod +x voxlink-receiver
   ```

3. Find your computer's Wi-Fi IP address:

   ```sh
   ip -brief address
   ```

   Look for the `inet` address on your Wi-Fi interface (often named `wlan0` or `wlp...`). Use that address in place of `192.168.1.20` below.

4. Start the receiver, replacing the example address with your own:

   ```sh
   mkdir -p recordings
   ./voxlink-receiver --bind 192.168.1.20 --port 4950 --output recordings/received-audio.wav
   ```

The receiver listens on the chosen Wi-Fi address and UDP port `4950`. If your Linux firewall blocks incoming UDP, allow port `4950` on your trusted home network. Start the receiver before the phone sender. Press `Ctrl+C` after recording to finalize the WAV file.

### Build the receiver from source

Install Rust and Cargo using [rustup](https://rustup.rs/), then run these commands from the repository root:

```sh
cargo build --release --locked --manifest-path receiver/Cargo.toml
mkdir -p recordings
./receiver/target/release/voxlink-receiver --bind 192.168.1.20 --port 4950 --output recordings/received-audio.wav
```

Replace `192.168.1.20` with the Linux computer's Wi-Fi IP address, found with `ip -brief address`.

## Install and run the Android sender

Download the [Android APK](https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink-android-debug.apk) on your phone and open it to install. Android may ask you to allow installation from the browser or file manager you used to download the APK. Then enter the Linux computer's Wi-Fi IP address in the app, use port `4950`, tap **Start**, grant microphone permission, speak into the phone, and tap **Stop** when finished.

To build the app yourself, open the `android/` directory in Android Studio with Android SDK 35 and JDK 17, let Gradle sync, and build or run the `app` module on a connected phone.

The sender tries 48 kHz mono PCM and falls back to 44.1 kHz if the device cannot initialize 48 kHz capture. Keep the app in the foreground while recording in this prototype.

## Packet format

Each UDP datagram contains a 12-byte header followed by signed 16-bit little-endian mono PCM samples:

| Offset | Size | Value |
|---:|---:|---|
| 0 | 4 | ASCII magic `VXL1` |
| 4 | 4 | Packet sequence number, unsigned big-endian |
| 8 | 4 | Sample rate in Hz, unsigned big-endian |
| 12 | variable | PCM16 little-endian audio payload |

Packets are sent in capture order. The receiver reports sequence gaps as a rough measure of UDP packet loss. Missing packets are not reconstructed; the recording will have gaps where UDP data was lost.

## Manual audio check

1. Start the receiver before the phone sender.
2. Record speech for 10–20 seconds, then stop the sender and press `Ctrl+C` in the receiver.
3. Confirm the output file exists and has a plausible size: 48 kHz mono PCM is about 96 KB per second; 44.1 kHz is about 88 KB per second.
4. Play it with `aplay recordings/received-audio.wav` or another WAV player.
5. Listen for clipping, distortion, noise, or dropouts, and repeat with a longer recording.

Audio is unencrypted. Desktop pairing authorizes a source IP on the trusted LAN; the standalone receiver has no pairing authentication. Do not forward either UDP port to the public internet.
