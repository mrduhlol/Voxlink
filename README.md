# Voxlink v0.1

Voxlink is a small Android-to-Linux audio streaming prototype. The Android app captures mono PCM audio and sends UDP datagrams over the local network. The Rust receiver writes the packets to a WAV file.

## Downloads

<a href="https://github.com/mrduhlol/Voxlink/releases/latest/download/Voxlink-android-debug.apk"><img alt="Download Android APK" src="https://img.shields.io/badge/Download-Android_APK-3DDC84?logo=android&logoColor=white"></a>
<a href="https://github.com/mrduhlol/Voxlink/releases/latest/download/voxlink-receiver-linux-x86_64.tar.gz"><img alt="Download Linux receiver" src="https://img.shields.io/badge/Download-Linux_x86__64-0078D4?logo=linux&logoColor=white"></a>

These links become active after the first versioned release is published. Pushing a `v*` tag builds both files and attaches them to a GitHub release. The Android download is a debug APK; Android may ask you to allow installation from your browser or file manager.

## Requirements

- Android phone and Linux computer on the same trusted Wi-Fi network.
- Android Studio with Android SDK 35 and JDK 17 to build the app.
- Rust and Cargo to build the receiver.

## Build and run the Linux receiver

From the repository root:

```sh
cargo build --release --manifest-path receiver/Cargo.toml
ip -brief address
mkdir -p recordings
./receiver/target/release/voxlink-receiver --bind <PC_WIFI_IP> --port 4950 --output recordings/received-audio.wav
```

Use the PC's Wi-Fi address for `--bind`. The receiver requires an explicit bind address so it does not silently listen on every interface. Keep the port reachable only from your trusted local network. Press `Ctrl+C` to stop and finalize the WAV file.

## Build and run the Android sender

Open the `android/` directory in Android Studio and let Gradle sync. Build and install the `app` module, or use Android Studio's Run action with a connected phone. On the phone, enter the PC Wi-Fi IP and port `4950`, tap **Start**, grant microphone permission, speak into the phone, then tap **Stop**.

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

This prototype has no encryption or authentication. Use it only on a trusted local network; do not forward the receiver port to the public internet.
