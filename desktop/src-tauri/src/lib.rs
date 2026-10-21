use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use if_addrs::get_if_addrs;
use libpulse_binding as pulse;
use libpulse_simple_binding::Simple as PulseSimple;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{Manager, State};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use voxlink_receiver::AudioPacket;

const UDP_BUFFER_SIZE: usize = 65_535;
const AUDIO_QUEUE_PACKETS: usize = 10;
const VIRTUAL_SINK: &str = "voxlink_mic";
const VIRTUAL_SOURCE: &str = "voxlink_mic.monitor";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum MicrophoneState {
    #[default]
    Stopped,
    Ready,
    Error,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusView {
    connection: ConnectionState,
    listening: bool,
    virtual_microphone: MicrophoneState,
    virtual_source: Option<String>,
    bind_address: Option<String>,
    phone_address: Option<String>,
    port: u16,
    sample_rate: Option<u32>,
    packets_received: u64,
    packets_lost: u64,
    packets_invalid: u64,
    packets_filtered: u64,
    audio_queue_drops: u64,
    input_level: f32,
    input_peak_dbfs: Option<f32>,
    last_packet_ago_ms: Option<u64>,
    error: Option<String>,
    recovery: Option<String>,
}

#[derive(Default)]
struct SharedStatus {
    view: StatusView,
    last_packet_at: Option<Instant>,
}

impl SharedStatus {
    fn snapshot(&self) -> StatusView {
        let mut view = self.view.clone();
        view.last_packet_ago_ms = self
            .last_packet_at
            .map(|instant| instant.elapsed().as_millis().min(u64::MAX as u128) as u64);
        if view.last_packet_ago_ms.is_some_and(|age| age > 220) {
            view.input_level = 0.0;
            view.input_peak_dbfs = None;
        }
        view
    }
}

#[derive(Default)]
struct DesktopState {
    status: Arc<Mutex<SharedStatus>>,
    runtime: Mutex<RuntimeHandles>,
}

#[derive(Default)]
struct RuntimeHandles {
    stop: Option<Arc<AtomicBool>>,
    receiver_thread: Option<JoinHandle<()>>,
    audio_sender: Option<Sender<AudioFrame>>,
    audio_thread: Option<JoinHandle<()>>,
    module_id: Option<String>,
    owns_module: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartConfig {
    bind_ip: String,
    phone_ip: String,
    port: u16,
    timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAddress {
    name: String,
    ip: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppImageInstallState {
    available: bool,
    installed: bool,
}

struct VirtualSink {
    module_id: Option<String>,
    owned: bool,
}

struct AudioFrame {
    sample_rate: u32,
    pcm: Vec<u8>,
}

fn lock_status(status: &Arc<Mutex<SharedStatus>>) -> MutexGuard<'_, SharedStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_start_error(status: &Arc<Mutex<SharedStatus>>, message: String, recovery: String) {
    let mut shared = lock_status(status);
    shared.view.connection = ConnectionState::Error;
    shared.view.listening = false;
    shared.view.error = Some(message);
    shared.view.recovery = Some(recovery);
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|error| format!("Could not run pactl: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("pactl {} exited with {}", args.join(" "), output.status)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn list_has_name(output: &str, name: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .any(|entry| entry == name)
}

fn ensure_virtual_sink() -> Result<VirtualSink, String> {
    pactl(&["info"])
        .map_err(|error| format!("The PulseAudio compatibility service is unavailable: {error}"))?;

    let sinks = pactl(&["list", "short", "sinks"])?;
    let mut sink = if list_has_name(&sinks, VIRTUAL_SINK) {
        VirtualSink {
            module_id: None,
            owned: false,
        }
    } else {
        let id = pactl(&[
            "load-module",
            "module-null-sink",
            "sink_name=voxlink_mic",
            "rate=48000",
            "channels=1",
            "channel_map=mono",
            "sink_properties=device.description='Voxlink Microphone'",
        ])?;
        if id.parse::<u32>().is_err() {
            return Err(format!("pactl returned an invalid module id: {id}"));
        }
        VirtualSink {
            module_id: Some(id),
            owned: true,
        }
    };

    let source_ready = (0..8).any(|_| match pactl(&["list", "short", "sources"]) {
        Ok(sources) if list_has_name(&sources, VIRTUAL_SOURCE) => true,
        _ => {
            thread::sleep(Duration::from_millis(75));
            false
        }
    });
    if !source_ready {
        if sink.owned {
            let _ = unload_virtual_sink(&sink);
            sink.module_id = None;
        }
        return Err(format!(
            "The null sink loaded but its monitor source `{VIRTUAL_SOURCE}` did not appear."
        ));
    }

    Ok(sink)
}

fn unload_virtual_sink(sink: &VirtualSink) -> Result<(), String> {
    if !sink.owned {
        return Ok(());
    }
    if let Some(module_id) = sink.module_id.as_deref() {
        pactl(&["unload-module", module_id])?;
    }
    Ok(())
}

fn validate_config(config: &StartConfig) -> Result<(Ipv4Addr, Ipv4Addr), String> {
    let bind_ip = config
        .bind_ip
        .parse::<Ipv4Addr>()
        .map_err(|_| "Enter a valid PC Wi-Fi IPv4 address.".to_string())?;
    let phone_ip = config
        .phone_ip
        .parse::<Ipv4Addr>()
        .map_err(|_| "Enter a valid phone IPv4 address.".to_string())?;
    if !bind_ip.is_private() || bind_ip.is_loopback() || bind_ip.is_unspecified() {
        return Err("Bind to a private Wi-Fi/LAN IPv4 address on this PC.".to_string());
    }
    if !phone_ip.is_private() || phone_ip.is_loopback() || phone_ip.is_unspecified() {
        return Err("The phone address must be a private LAN IPv4 address.".to_string());
    }
    if config.port < 1024 {
        return Err(
            "Choose a UDP port from 1024 to 65535; 4950 is the Android default.".to_string(),
        );
    }
    if !(1_000..=6_000).contains(&config.timeout_ms) {
        return Err("Disconnect timeout must be between 1 and 6 seconds.".to_string());
    }
    Ok((bind_ip, phone_ip))
}

#[tauri::command]
fn get_local_addresses() -> Result<Vec<LocalAddress>, String> {
    let mut addresses = get_if_addrs()
        .map_err(|error| format!("Could not inspect network interfaces: {error}"))?
        .into_iter()
        .filter_map(|interface| {
            if !interface.is_oper_up() || interface.is_loopback() || interface.is_link_local() {
                return None;
            }
            let ip = match interface.ip() {
                IpAddr::V4(ip) if ip.is_private() => ip,
                _ => return None,
            };
            let virtual_bridge = ["docker", "veth", "virbr", "podman", "br-", "cni"]
                .iter()
                .any(|prefix| interface.name.starts_with(prefix));
            if virtual_bridge {
                return None;
            }
            Some(LocalAddress {
                name: interface.name,
                ip: ip.to_string(),
            })
        })
        .collect::<Vec<_>>();
    addresses.sort_by_key(|address| {
        let wireless = address.name.starts_with("wl") || address.name.starts_with("wlan");
        (!wireless, address.name.clone())
    });
    addresses.dedup_by(|left, right| left.ip == right.ip);
    Ok(addresses)
}

#[tauri::command]
fn get_status(state: State<'_, DesktopState>) -> StatusView {
    lock_status(&state.status).snapshot()
}

fn appimage_paths() -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let source = PathBuf::from(std::env::var_os("APPIMAGE")?);
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let data_dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    let installed_image = home.join("Applications/Voxlink.AppImage");
    let desktop_entry = data_dir.join("applications/voxlink.desktop");
    let icon = data_dir.join("icons/hicolor/512x512/apps/voxlink.png");
    Some((source, installed_image, desktop_entry, icon))
}

fn desktop_exec_path(path: &std::path::Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

#[tauri::command]
fn get_appimage_install_state() -> AppImageInstallState {
    let Some((source, installed_image, desktop_entry, _)) = appimage_paths() else {
        return AppImageInstallState {
            available: false,
            installed: false,
        };
    };
    let available = source.is_file();
    let installed = installed_image.is_file()
        && fs::read_to_string(&desktop_entry)
            .is_ok_and(|contents| contents.contains(&desktop_exec_path(&installed_image)));
    AppImageInstallState {
        available,
        installed,
    }
}

#[tauri::command]
fn install_appimage_launcher() -> Result<String, String> {
    let (source, installed_image, desktop_entry, icon) = appimage_paths().ok_or_else(|| {
        "Launch Voxlink from its AppImage to add it to the app launcher.".to_string()
    })?;
    if !source.is_file() {
        return Err("The running Voxlink AppImage could not be found.".to_string());
    }

    let source = source
        .canonicalize()
        .map_err(|error| format!("Could not locate the running AppImage: {error}"))?;
    if source != installed_image {
        let parent = installed_image
            .parent()
            .ok_or_else(|| "Could not determine the Voxlink install folder.".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        fs::copy(&source, &installed_image).map_err(|error| {
            format!(
                "Could not copy Voxlink to {}: {error}",
                installed_image.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&installed_image, fs::Permissions::from_mode(0o755)).map_err(
                |error| format!("Could not make the installed AppImage executable: {error}"),
            )?;
        }
    }

    let desktop_parent = desktop_entry
        .parent()
        .ok_or_else(|| "Could not determine the applications folder.".to_string())?;
    let icon_parent = icon
        .parent()
        .ok_or_else(|| "Could not determine the icons folder.".to_string())?;
    fs::create_dir_all(desktop_parent)
        .map_err(|error| format!("Could not create {}: {error}", desktop_parent.display()))?;
    fs::create_dir_all(icon_parent)
        .map_err(|error| format!("Could not create {}: {error}", icon_parent.display()))?;
    fs::write(&icon, include_bytes!("../icons/icon.png"))
        .map_err(|error| format!("Could not install the Voxlink icon: {error}"))?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Voxlink\nComment=Use your Android phone as a Linux microphone\nExec={}\nIcon={}\nTerminal=false\nCategories=AudioVideo;Audio;\nStartupNotify=true\n",
        desktop_exec_path(&installed_image),
        icon.display()
    );
    fs::write(&desktop_entry, entry)
        .map_err(|error| format!("Could not add Voxlink to the app launcher: {error}"))?;
    info!(image = %installed_image.display(), entry = %desktop_entry.display(), "Voxlink app launcher entry installed");
    Ok(installed_image.display().to_string())
}

#[tauri::command]
fn start_stream(state: State<'_, DesktopState>, config: StartConfig) -> Result<(), String> {
    let (bind_ip, phone_ip) = validate_config(&config).map_err(|error| {
        set_start_error(
            &state.status,
            error.clone(),
            "Check the network fields and select a private Wi-Fi address.".to_string(),
        );
        error
    })?;

    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "Voxlink runtime state is unavailable.".to_string())?;
    if runtime.receiver_thread.is_some() {
        return Err("The receiver is already listening.".to_string());
    }

    {
        let mut shared = lock_status(&state.status);
        shared.view = StatusView {
            connection: ConnectionState::Connecting,
            virtual_microphone: MicrophoneState::Stopped,
            port: config.port,
            ..StatusView::default()
        };
        shared.last_packet_at = None;
    }

    let sink = match ensure_virtual_sink() {
        Ok(sink) => sink,
        Err(error) => {
            error!(%error, "virtual microphone setup failed");
            set_start_error(
                &state.status,
                error.clone(),
                "Install and start pipewire-pulse, then confirm `pactl info` works for your user."
                    .to_string(),
            );
            return Err(error);
        }
    };

    let bind_address = SocketAddr::V4(SocketAddrV4::new(bind_ip, config.port));
    let socket = match UdpSocket::bind(bind_address) {
        Ok(socket) => socket,
        Err(error) => {
            let message = format!("Could not listen on {bind_address}: {error}");
            let recovery = "Check that the address belongs to this PC and that UDP port is not already in use.".to_string();
            let _ = unload_virtual_sink(&sink);
            set_start_error(&state.status, message.clone(), recovery);
            return Err(message);
        }
    };
    if let Err(error) = socket.set_read_timeout(Some(Duration::from_millis(50))) {
        let message = format!("Could not configure the UDP listener: {error}");
        let _ = unload_virtual_sink(&sink);
        set_start_error(
            &state.status,
            message.clone(),
            "Restart Voxlink and check the local socket permissions.".to_string(),
        );
        return Err(message);
    }

    let local_address = socket
        .local_addr()
        .map(|address| address.to_string())
        .unwrap_or_else(|_| bind_address.to_string());
    let stop = Arc::new(AtomicBool::new(false));
    let (audio_sender, audio_receiver) = bounded::<AudioFrame>(AUDIO_QUEUE_PACKETS);
    let status_for_audio = Arc::clone(&state.status);
    let audio_thread = match thread::Builder::new()
        .name("voxlink-pulse-writer".to_string())
        .spawn(move || audio_writer(audio_receiver, status_for_audio))
    {
        Ok(thread) => thread,
        Err(error) => {
            let message = format!("Could not start the audio output thread: {error}");
            let _ = unload_virtual_sink(&sink);
            set_start_error(
                &state.status,
                message.clone(),
                "Restart Voxlink; check available memory and process limits.".to_string(),
            );
            return Err(message);
        }
    };

    let stop_for_receiver = Arc::clone(&stop);
    let audio_for_receiver = audio_sender.clone();
    let status_for_receiver = Arc::clone(&state.status);
    let timeout = Duration::from_millis(config.timeout_ms);
    let receiver_thread = match thread::Builder::new()
        .name("voxlink-udp-receiver".to_string())
        .spawn(move || {
            receive_audio(
                socket,
                phone_ip,
                timeout,
                stop_for_receiver,
                audio_for_receiver,
                status_for_receiver,
            )
        }) {
        Ok(thread) => thread,
        Err(error) => {
            stop.store(true, Ordering::Relaxed);
            drop(audio_sender);
            let _ = audio_thread.join();
            let message = format!("Could not start the UDP receive thread: {error}");
            let _ = unload_virtual_sink(&sink);
            set_start_error(
                &state.status,
                message.clone(),
                "Restart Voxlink; check available memory and process limits.".to_string(),
            );
            return Err(message);
        }
    };

    {
        let mut shared = lock_status(&state.status);
        shared.view.listening = true;
        shared.view.virtual_microphone = MicrophoneState::Ready;
        shared.view.virtual_source = Some(VIRTUAL_SOURCE.to_string());
        shared.view.bind_address = Some(local_address.clone());
        shared.view.phone_address = Some(phone_ip.to_string());
        shared.view.port = config.port;
        shared.view.error = None;
        shared.view.recovery = None;
    }
    info!(%local_address, %phone_ip, port = config.port, source = VIRTUAL_SOURCE, "Voxlink listener started");

    runtime.stop = Some(stop);
    runtime.receiver_thread = Some(receiver_thread);
    runtime.audio_sender = Some(audio_sender);
    runtime.audio_thread = Some(audio_thread);
    runtime.module_id = sink.module_id;
    runtime.owns_module = sink.owned;
    Ok(())
}

#[tauri::command]
fn stop_stream(state: State<'_, DesktopState>) -> Result<(), String> {
    stop_runtime(&state)
}

fn stop_runtime(state: &DesktopState) -> Result<(), String> {
    let handles = {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "Voxlink runtime state is unavailable.".to_string())?;
        std::mem::take(&mut *runtime)
    };

    if let Some(stop) = &handles.stop {
        stop.store(true, Ordering::Relaxed);
    }
    {
        let mut shared = lock_status(&state.status);
        shared.view.listening = false;
        shared.view.connection = ConnectionState::Disconnected;
        shared.view.input_level = 0.0;
        shared.view.input_peak_dbfs = None;
        shared.last_packet_at = None;
    }

    if let Some(receiver) = handles.receiver_thread {
        if receiver.join().is_err() {
            warn!("UDP receiver thread panicked during shutdown");
        }
    }
    drop(handles.audio_sender);
    if let Some(audio) = handles.audio_thread {
        if audio.join().is_err() {
            warn!("PulseAudio writer thread panicked during shutdown");
        }
    }

    let sink = VirtualSink {
        module_id: handles.module_id,
        owned: handles.owns_module,
    };
    match unload_virtual_sink(&sink) {
        Ok(()) => {
            let mut shared = lock_status(&state.status);
            shared.view.virtual_microphone = if sink.owned {
                MicrophoneState::Stopped
            } else {
                MicrophoneState::Ready
            };
            if sink.owned {
                shared.view.virtual_source = None;
            }
            shared.view.sample_rate = None;
            info!(owned = sink.owned, "Voxlink audio session stopped");
            Ok(())
        }
        Err(error) => {
            error!(%error, "could not remove Voxlink virtual sink");
            let mut shared = lock_status(&state.status);
            shared.view.virtual_microphone = MicrophoneState::Error;
            shared.view.error = Some(format!(
                "Audio stopped, but the virtual sink could not be removed: {error}"
            ));
            shared.view.recovery = Some("Use `pactl list short modules` to find the Voxlink null-sink module, then unload it with `pactl unload-module MODULE_ID`.".to_string());
            Err(error)
        }
    }
}

fn audio_writer(receiver: Receiver<AudioFrame>, status: Arc<Mutex<SharedStatus>>) {
    let mut output_rate: Option<u32> = None;
    let mut output: Option<PulseSimple> = None;

    while let Ok(frame) = receiver.recv() {
        if output_rate != Some(frame.sample_rate) || output.is_none() {
            output = None;
            output_rate = None;
            let spec = Spec {
                format: Format::S16NE,
                channels: 1,
                rate: frame.sample_rate,
            };
            if !spec.is_valid() {
                set_audio_error(
                    &status,
                    format!("Unsupported PCM sample rate: {} Hz", frame.sample_rate),
                );
                continue;
            }
            match PulseSimple::new(
                None,
                "Voxlink",
                Direction::Playback,
                Some(VIRTUAL_SINK),
                "Phone microphone audio",
                &spec,
                None,
                None,
            ) {
                Ok(stream) => {
                    output = Some(stream);
                    output_rate = Some(frame.sample_rate);
                    let mut shared = lock_status(&status);
                    shared.view.sample_rate = Some(frame.sample_rate);
                    shared.view.virtual_microphone = MicrophoneState::Ready;
                    shared.view.error = None;
                    shared.view.recovery = None;
                    info!(
                        sample_rate = frame.sample_rate,
                        sink = VIRTUAL_SINK,
                        "PulseAudio playback stream ready"
                    );
                }
                Err(error) => {
                    set_audio_error(
                        &status,
                        format!("Could not open the Voxlink PulseAudio sink: {error:?}"),
                    );
                    continue;
                }
            }
        }

        if let Some(stream) = output.as_ref() {
            if let Err(error) = stream.write(&frame.pcm) {
                warn!(
                    ?error,
                    "PulseAudio write failed; will reconnect on the next packet"
                );
                output = None;
                output_rate = None;
                set_audio_error(&status, format!("Audio output failed: {error:?}"));
            }
        }
    }

    if let Some(stream) = output.as_ref() {
        if let Err(error) = stream.drain() {
            warn!(?error, "PulseAudio drain failed during stop");
        }
    }
}

fn set_audio_error(status: &Arc<Mutex<SharedStatus>>, message: String) {
    let mut shared = lock_status(status);
    shared.view.virtual_microphone = MicrophoneState::Error;
    shared.view.error = Some(message);
    shared.view.recovery = Some("Confirm the Voxlink Microphone sink is present in `pactl list short sinks` and restart the listener.".to_string());
}

fn receive_audio(
    socket: UdpSocket,
    expected_phone: Ipv4Addr,
    timeout: Duration,
    stop: Arc<AtomicBool>,
    audio_sender: Sender<AudioFrame>,
    status: Arc<Mutex<SharedStatus>>,
) {
    let start_time = Instant::now();
    let mut ever_connected = false;
    let mut expected_sequence: Option<u32> = None;
    let mut buffer = [0u8; UDP_BUFFER_SIZE];

    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((length, peer)) => {
                if peer.ip() != IpAddr::V4(expected_phone) {
                    lock_status(&status).view.packets_filtered += 1;
                    continue;
                }
                let packet = match AudioPacket::parse(&buffer[..length]) {
                    Ok(packet) => packet,
                    Err(reason) => {
                        let mut shared = lock_status(&status);
                        shared.view.packets_invalid += 1;
                        if shared.view.packets_invalid == 1 {
                            warn!(?reason, %peer, "discarding invalid Voxlink packet");
                        }
                        continue;
                    }
                };

                if let Some(expected) = expected_sequence {
                    let distance = packet.sequence.wrapping_sub(expected);
                    if distance < (1 << 31) {
                        lock_status(&status).view.packets_lost += u64::from(distance);
                    } else {
                        lock_status(&status).view.packets_invalid += 1;
                        continue;
                    }
                }
                expected_sequence = Some(packet.sequence.wrapping_add(1));

                let (rms, peak) = pcm_levels(packet.pcm);
                {
                    let mut shared = lock_status(&status);
                    shared.view.connection = ConnectionState::Connected;
                    shared.view.sample_rate = Some(packet.sample_rate);
                    shared.view.packets_received += 1;
                    shared.view.input_level = rms;
                    shared.view.input_peak_dbfs = Some(peak);
                    shared.last_packet_at = Some(Instant::now());
                }
                ever_connected = true;

                match audio_sender.try_send(AudioFrame {
                    sample_rate: packet.sample_rate,
                    pcm: packet.pcm.to_vec(),
                }) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        lock_status(&status).view.audio_queue_drops += 1;
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        lock_status(&status).view.audio_queue_drops += 1;
                    }
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                let mut shared = lock_status(&status);
                if shared
                    .last_packet_at
                    .is_some_and(|last| last.elapsed() >= timeout)
                    || (!ever_connected && start_time.elapsed() >= timeout)
                {
                    shared.view.connection = ConnectionState::Disconnected;
                    shared.view.input_level = 0.0;
                    shared.view.input_peak_dbfs = None;
                }
            }
            Err(error) => {
                error!(%error, "UDP receiver failed");
                let mut shared = lock_status(&status);
                shared.view.connection = ConnectionState::Error;
                shared.view.error = Some(format!("UDP receiver stopped: {error}"));
                shared.view.recovery = Some("Check the bound Wi-Fi interface and UDP firewall rule, then stop and restart Voxlink.".to_string());
                break;
            }
        }
    }
}

fn pcm_levels(pcm: &[u8]) -> (f32, f32) {
    let mut square_sum = 0.0f64;
    let mut peak = 0i32;
    let samples = pcm.len() / 2;
    if samples == 0 {
        return (0.0, -96.0);
    }
    for bytes in pcm.chunks_exact(2) {
        let sample = i16::from_le_bytes([bytes[0], bytes[1]]) as i32;
        let magnitude = sample.abs();
        peak = peak.max(magnitude);
        let normalized = sample as f64 / 32768.0;
        square_sum += normalized * normalized;
    }
    let rms = (square_sum / samples as f64).sqrt() as f32;
    let peak_linear = peak as f32 / 32768.0;
    let peak_dbfs = if peak_linear <= 0.000_015_85 {
        -96.0
    } else {
        (20.0 * peak_linear.log10()).max(-96.0)
    };
    (rms.clamp(0.0, 1.0), peak_dbfs)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            get_local_addresses,
            get_status,
            get_appimage_install_state,
            install_appimage_launcher,
            start_stream,
            stop_stream
        ])
        .build(tauri::generate_context!())
        .expect("error while building Voxlink desktop app")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<DesktopState>();
                if let Err(error) = stop_runtime(&state) {
                    error!(%error, "Voxlink shutdown cleanup failed");
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::pcm_levels;
    use std::net::Ipv4Addr;

    #[test]
    fn private_address_validation_rejects_public_bind_targets() {
        assert!(Ipv4Addr::new(192, 168, 1, 5).is_private());
        assert!(!Ipv4Addr::new(8, 8, 8, 8).is_private());
    }

    #[test]
    fn level_meter_reports_silence_and_full_scale() {
        assert_eq!(pcm_levels(&[0, 0, 0, 0]), (0.0, -96.0));
        let (rms, peak_dbfs) = pcm_levels(&[0xff, 0x7f, 0x00, 0x80]);
        assert!((rms - 1.0).abs() < 0.00002);
        assert_eq!(peak_dbfs, 0.0);
    }
}
