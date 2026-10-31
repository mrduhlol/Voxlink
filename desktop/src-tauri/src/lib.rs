mod pairing;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use if_addrs::get_if_addrs;
use libpulse_binding as pulse;
use libpulse_simple_binding::Simple as PulseSimple;
use pairing::{
    AuthGate, ClaimResponse, DiscoveryPacket, PendingPairing, DISCOVERY_PORT, WAITING_FOR_PHONE,
};
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

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PairingState {
    #[default]
    Unpaired,
    Pending,
    Paired,
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
    pairing_code: String,
    pairing_state: PairingState,
    pending_pairing: Option<PendingPairing>,
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
    auth: Option<Arc<Mutex<AuthGate>>>,
    receiver_thread: Option<JoinHandle<()>>,
    discovery_thread: Option<JoinHandle<()>>,
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

fn validate_config(config: &StartConfig) -> Result<(Ipv4Addr, Option<Ipv4Addr>), String> {
    let bind_ip = config
        .bind_ip
        .parse::<Ipv4Addr>()
        .map_err(|_| "Enter a valid PC Wi-Fi IPv4 address.".to_string())?;
    let phone_ip = if config.phone_ip.trim().is_empty() {
        None
    } else {
        Some(
            config
                .phone_ip
                .trim()
                .parse::<Ipv4Addr>()
                .map_err(|_| "Enter a valid phone IPv4 address.".to_string())?,
        )
    };
    if !bind_ip.is_private() || bind_ip.is_loopback() || bind_ip.is_unspecified() {
        return Err("Bind to a private Wi-Fi/LAN IPv4 address on this PC.".to_string());
    }
    if phone_ip.is_some_and(|ip| !ip.is_private() || ip.is_loopback() || ip.is_unspecified()) {
        return Err("The phone address must be a private LAN IPv4 address.".to_string());
    }
    if config.port == DISCOVERY_PORT {
        return Err(
            "Port 4951 is reserved for discovery — pick another UDP port for audio.".into(),
        );
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
fn get_pairing_uri(
    state: State<'_, DesktopState>,
    bind_ip: String,
    port: u16,
) -> Result<String, String> {
    pairing::pairing_uri(
        &bind_ip,
        port,
        &lock_status(&state.status).view.pairing_code,
    )
}

#[tauri::command]
fn refresh_pairing_code(state: State<'_, DesktopState>) -> Result<String, String> {
    let runtime = state
        .runtime
        .lock()
        .map_err(|_| "Voxlink runtime state is unavailable.")?;
    if runtime.receiver_thread.is_some() {
        return Err("Stop listening before refreshing the pairing code.".into());
    }
    let code = pairing::generate_code();
    if let Err(error) = pairing::save_code(&pairing::code_path(), &code) {
        warn!(%error, "Could not save pairing code");
    }
    lock_status(&state.status).view.pairing_code = code.clone();
    Ok(code)
}

fn sync_pairing_status(auth: &AuthGate, status: &Arc<Mutex<SharedStatus>>) {
    let mut shared = lock_status(status);
    shared.view.pending_pairing = auth.pending();
    shared.view.pairing_state = if shared.view.pending_pairing.is_some() {
        PairingState::Pending
    } else if auth.paired_phone().is_some() {
        PairingState::Paired
    } else {
        PairingState::Unpaired
    };
    if let Some(ip) = auth.paired_phone() {
        shared.view.phone_address = Some(ip.to_string());
    }
}

#[tauri::command]
fn respond_to_pairing(
    state: State<'_, DesktopState>,
    request_id: u64,
    allow: bool,
) -> Result<(), String> {
    respond_to_pairing_inner(&state, request_id, allow)
}

fn respond_to_pairing_inner(
    state: &DesktopState,
    request_id: u64,
    allow: bool,
) -> Result<(), String> {
    let runtime = state
        .runtime
        .lock()
        .map_err(|_| "Voxlink runtime state is unavailable.")?;
    let auth = runtime
        .auth
        .as_ref()
        .ok_or("Start listening before pairing a phone.")?;
    let mut auth = auth.lock().unwrap_or_else(|p| p.into_inner());
    let ip = auth.respond(request_id, allow)?;
    sync_pairing_status(&auth, &state.status);
    let mut shared = lock_status(&state.status);
    if allow {
        shared.view.connection = ConnectionState::Connecting;
        shared.last_packet_at = None;
        shared.view.input_level = 0.0;
        shared.view.input_peak_dbfs = None;
        shared.view.sample_rate = None;
        shared.view.recovery = Some(format!("Paired with {ip}. Waiting for audio — start streaming on the phone. If no audio arrives, allow the audio UDP port on this PC's trusted network."));
    } else if auth.paired_phone().is_none() {
        shared.view.recovery = Some(format!(
            "Pairing request from {ip} rejected. No audio from this phone is allowed."
        ));
    }
    Ok(())
}

fn bind_discovery_socket_on(port: u16) -> Result<UdpSocket, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            format!("UDP port {port} is already in use — another Voxlink may be running.")
        } else {
            format!("Could not listen for discovery on UDP port {port}: {error}")
        }
    })?;
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

fn run_discovery_responder(
    socket: UdpSocket,
    audio_port: u16,
    code: String,
    auth: Arc<Mutex<AuthGate>>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<SharedStatus>>,
) {
    let mut buffer = [0u8; 1024];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((length, peer)) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let IpAddr::V4(source) = peer.ip() else {
                    continue;
                };
                match pairing::parse_discovery(&buffer[..length]) {
                    DiscoveryPacket::Probe => {
                        let _ = socket.send_to(&pairing::encode_reply(&code, audio_port), peer);
                    }
                    DiscoveryPacket::Claim { code: claim } => {
                        let mut gate = auth.lock().unwrap_or_else(|p| p.into_inner());
                        let response = gate.request(source, claim, &code);
                        sync_pairing_status(&gate, &status);
                        let bytes = match response {
                            ClaimResponse::Approved => Some(pairing::encode_ack()),
                            ClaimResponse::Pending => Some(*b"VXLP"),
                            ClaimResponse::Rejected => Some(*b"VXLN"),
                            ClaimResponse::Invalid => None,
                        };
                        if let Some(bytes) = bytes {
                            let _ = socket.send_to(&bytes, peer);
                        }
                    }
                    _ => {}
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                let mut gate = auth.lock().unwrap_or_else(|p| p.into_inner());
                gate.expire_pending();
                sync_pairing_status(&gate, &status);
            }
            Err(error) => {
                warn!(%error, "Discovery listener stopped");
                break;
            }
        }
    }
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
            pairing_code: shared.view.pairing_code.clone(),
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

    let discovery = match bind_discovery_socket_on(DISCOVERY_PORT) {
        Ok(socket) => socket,
        Err(message) => {
            let _ = unload_virtual_sink(&sink);
            set_start_error(
                &state.status,
                message.clone(),
                "Stop the other Voxlink listener and retry.".into(),
            );
            return Err(message);
        }
    };
    let auth = Arc::new(Mutex::new(AuthGate::new(phone_ip)));
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
    let auth_for_receiver = Arc::clone(&auth);
    let receiver_thread = match thread::Builder::new()
        .name("voxlink-udp-receiver".to_string())
        .spawn(move || {
            receive_audio(
                socket,
                auth_for_receiver,
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

    let discovery_stop = Arc::clone(&stop);
    let discovery_status = Arc::clone(&state.status);
    let discovery_auth = Arc::clone(&auth);
    let code = lock_status(&state.status).view.pairing_code.clone();
    let discovery_thread = match thread::Builder::new()
        .name("voxlink-discovery".into())
        .spawn(move || {
            run_discovery_responder(
                discovery,
                config.port,
                code,
                discovery_auth,
                discovery_stop,
                discovery_status,
            )
        }) {
        Ok(handle) => handle,
        Err(error) => {
            stop.store(true, Ordering::Relaxed);
            let _ = receiver_thread.join();
            drop(audio_sender);
            let _ = audio_thread.join();
            let _ = unload_virtual_sink(&sink);
            let message = format!("Could not start discovery: {error}");
            set_start_error(&state.status, message.clone(), "Restart Voxlink.".into());
            return Err(message);
        }
    };
    {
        let mut shared = lock_status(&state.status);
        shared.view.listening = true;
        shared.view.virtual_microphone = MicrophoneState::Ready;
        shared.view.virtual_source = Some(VIRTUAL_SOURCE.to_string());
        shared.view.bind_address = Some(local_address.clone());
        shared.view.phone_address = phone_ip.map(|ip| ip.to_string());
        shared.view.pairing_state = if phone_ip.is_some() {
            PairingState::Paired
        } else {
            PairingState::Unpaired
        };
        shared.view.port = config.port;
        shared.view.error = None;
        shared.view.recovery = phone_ip.is_none().then(|| WAITING_FOR_PHONE.to_string());
    }
    sync_pairing_status(
        &auth.lock().unwrap_or_else(|p| p.into_inner()),
        &state.status,
    );
    info!(%local_address, ?phone_ip, port = config.port, source = VIRTUAL_SOURCE, "Voxlink listener started");

    runtime.auth = Some(auth);
    runtime.stop = Some(stop);
    runtime.receiver_thread = Some(receiver_thread);
    runtime.discovery_thread = Some(discovery_thread);
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
        shared.view.pairing_state = PairingState::Unpaired;
        shared.view.pending_pairing = None;
        shared.view.connection = ConnectionState::Disconnected;
        shared.view.input_level = 0.0;
        shared.view.input_peak_dbfs = None;
        shared.last_packet_at = None;
    }

    if let Some(discovery) = handles.discovery_thread {
        let _ = discovery.join();
    }
    if let Some(receiver) = handles.receiver_thread {
        if receiver.join().is_err() {
            warn!("UDP receiver thread panicked during shutdown");
        }
    }
    {
        let mut shared = lock_status(&state.status);
        shared.view.pending_pairing = None;
        shared.view.pairing_state = PairingState::Unpaired;
        shared.view.connection = ConnectionState::Disconnected;
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
    auth: Arc<Mutex<AuthGate>>,
    timeout: Duration,
    stop: Arc<AtomicBool>,
    audio_sender: Sender<AudioFrame>,
    status: Arc<Mutex<SharedStatus>>,
) {
    let mut last_audio_at: Option<Instant> = None;
    let mut reported_unauthorized = false;
    let mut previous_source = None;
    let mut expected_sequence: Option<u32> = None;
    let mut buffer = [0u8; UDP_BUFFER_SIZE];

    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((length, peer)) => {
                let IpAddr::V4(source) = peer.ip() else {
                    continue;
                };
                if !auth
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .allows(source)
                {
                    let mut shared = lock_status(&status);
                    shared.view.packets_filtered += 1;
                    if !reported_unauthorized {
                        shared.view.recovery = Some(pairing::unauthorized_sender_message(source));
                        reported_unauthorized = true;
                    }
                    continue;
                }
                if previous_source != Some(peer) {
                    expected_sequence = None;
                    previous_source = Some(peer);
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

                // Android starts each new capture at zero, including a restart from the same IP/socket.
                if packet.sequence == 0 && expected_sequence.is_some_and(|expected| expected > 1) {
                    expected_sequence = None;
                }
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
                    shared.view.phone_address = Some(source.to_string());
                    if shared.view.error.is_none() {
                        shared.view.recovery = None;
                    }
                    shared.view.sample_rate = Some(packet.sample_rate);
                    shared.view.packets_received += 1;
                    shared.view.input_level = rms;
                    shared.view.input_peak_dbfs = Some(peak);
                    shared.last_packet_at = Some(Instant::now());
                }
                last_audio_at = Some(Instant::now());

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
                let paired = auth
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .paired_phone();
                let mut shared = lock_status(&status);
                if shared.last_packet_at.is_none() {
                    last_audio_at = None;
                    expected_sequence = None;
                    previous_source = None;
                }
                if last_audio_at.is_some_and(|last| last.elapsed() >= timeout) {
                    expected_sequence = None;
                    previous_source = None;
                    shared.view.connection = ConnectionState::Disconnected;
                    shared.view.input_level = 0.0;
                    shared.view.input_peak_dbfs = None;
                    if let Some(ip) = paired {
                        shared.view.recovery = Some(format!("Paired with {ip}, but audio stopped. Restart streaming on the phone; check the PC audio UDP port if no packets arrive."));
                    }
                } else if last_audio_at.is_none() {
                    shared.view.connection = ConnectionState::Connecting;
                    if let Some(ip) = paired {
                        if shared.view.pending_pairing.is_none() {
                            shared.view.recovery = Some(format!(
                                "Paired with {ip}. Waiting for audio from the phone."
                            ));
                        }
                    }
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

    let state = DesktopState::default();
    lock_status(&state.status).view.pairing_code =
        pairing::load_or_create_code(&pairing::code_path())
            .unwrap_or_else(|_| pairing::generate_code());
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_local_addresses,
            get_pairing_uri,
            refresh_pairing_code,
            respond_to_pairing,
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

#[cfg(test)]
mod pairing_integration_tests {
    use super::*;
    fn config(phone: &str, port: u16) -> StartConfig {
        StartConfig {
            bind_ip: "192.168.1.5".into(),
            phone_ip: phone.into(),
            port,
            timeout_ms: 2500,
        }
    }
    fn socket() -> UdpSocket {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        s
    }
    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(Instant::now() < deadline, "condition timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn auto_mode_and_private_pin_validation() {
        assert_eq!(validate_config(&config("  ", 4950)).unwrap().1, None);
        assert_eq!(
            validate_config(&config("192.168.1.42", 65535)).unwrap().1,
            Some(Ipv4Addr::new(192, 168, 1, 42))
        );
        for host in ["localhost", "8.8.8.8", "0.0.0.0"] {
            assert!(validate_config(&config(host, 4950)).is_err());
        }
        assert!(validate_config(&config("", 1023)).is_err());
        assert_eq!(
            validate_config(&config("", 4951)).unwrap_err(),
            "Port 4951 is reserved for discovery — pick another UDP port for audio."
        );
    }
    #[test]
    fn discovery_probe_claim_and_stop() {
        let responder = socket();
        let address = responder.local_addr().unwrap();
        let auth = Arc::new(Mutex::new(AuthGate::new(None)));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(SharedStatus::default()));
        let (a, s, v) = (auth.clone(), stop.clone(), status.clone());
        let handle = thread::spawn(move || {
            run_discovery_responder(responder, 4950, "482913".into(), a, s, v)
        });
        let client = socket();
        let mut buffer = [0; 32];
        for _ in 0..2 {
            client.send_to(b"VXLD", address).unwrap();
            let (n, _) = client.recv_from(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], &pairing::encode_reply("482913", 4950));
        }
        for bytes in [b"VXLC000000".as_slice(), b"NOPE", b"VXL1"] {
            client.send_to(bytes, address).unwrap();
            assert!(client.recv_from(&mut buffer).is_err());
            assert!(!auth.lock().unwrap().allows(Ipv4Addr::LOCALHOST));
        }
        client.send_to(b"VXLC482913", address).unwrap();
        let (n, _) = client.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"VXLP");
        let request_id = auth.lock().unwrap().pending().unwrap().request_id;
        assert_eq!(
            lock_status(&status).view.pairing_state,
            PairingState::Pending
        );
        assert_eq!(
            lock_status(&status)
                .view
                .pending_pairing
                .as_ref()
                .unwrap()
                .request_id,
            request_id
        );
        assert!(!auth.lock().unwrap().allows(Ipv4Addr::LOCALHOST));
        auth.lock().unwrap().respond(request_id, true).unwrap();
        client.send_to(b"VXLC482913", address).unwrap();
        let (n, _) = client.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"VXLA");
        assert_eq!(
            lock_status(&status).view.pairing_state,
            PairingState::Paired
        );
        assert!(lock_status(&status).view.pending_pairing.is_none());
        assert_eq!(lock_status(&status).view.packets_received, 0);
        assert!(auth.lock().unwrap().allows(Ipv4Addr::LOCALHOST));
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }
    #[test]
    fn discovery_port_conflicts_report_a_useful_error() {
        let held = UdpSocket::bind("0.0.0.0:0").unwrap();
        assert!(bind_discovery_socket_on(held.local_addr().unwrap().port())
            .unwrap_err()
            .contains("already in use"));
    }
    #[test]
    fn unauthorized_audio_is_filtered_until_claimed() {
        let listener = socket();
        let address = listener.local_addr().unwrap();
        let auth = Arc::new(Mutex::new(AuthGate::new(None)));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(SharedStatus::default()));
        let (tx, rx) = bounded(10);
        let (a, s, v) = (auth.clone(), stop.clone(), status.clone());
        let handle =
            thread::spawn(move || receive_audio(listener, a, Duration::from_secs(1), s, tx, v));
        let client = socket();
        let mut packet = b"VXL1".to_vec();
        packet.extend(0u32.to_be_bytes());
        packet.extend(48000u32.to_be_bytes());
        packet.extend([0, 0]);
        for n in 1..=3 {
            client.send_to(&packet, address).unwrap();
            wait_until(|| lock_status(&status).view.packets_filtered == n);
            assert_eq!(
                lock_status(&status).view.recovery.as_deref(),
                Some(pairing::unauthorized_sender_message(Ipv4Addr::LOCALHOST).as_str())
            );
        }
        assert_eq!(lock_status(&status).view.packets_received, 0);
        {
            let mut gate = auth.lock().unwrap();
            gate.request(Ipv4Addr::LOCALHOST, "482913", "482913");
            let id = gate.pending().unwrap().request_id;
            gate.respond(id, true).unwrap();
        }
        client.send_to(&packet, address).unwrap();
        wait_until(|| lock_status(&status).view.packets_received == 1);
        assert_eq!(
            lock_status(&status).view.phone_address.as_deref(),
            Some("127.0.0.1")
        );
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }
    #[test]
    fn phone_capture_restart_recovers_on_same_ip_and_sequence_zero() {
        let listener = socket();
        let address = listener.local_addr().unwrap();
        let auth = Arc::new(Mutex::new(AuthGate::new(Some(Ipv4Addr::LOCALHOST))));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(SharedStatus::default()));
        let (tx, _rx) = bounded(10);
        let (a, s, v) = (auth.clone(), stop.clone(), status.clone());
        let handle =
            thread::spawn(move || receive_audio(listener, a, Duration::from_millis(120), s, tx, v));
        let client = socket();
        let send = |client: &UdpSocket, sequence: u32| {
            let mut packet = b"VXL1".to_vec();
            packet.extend(sequence.to_be_bytes());
            packet.extend(48000u32.to_be_bytes());
            packet.extend([0, 0]);
            client.send_to(&packet, address).unwrap();
        };
        send(&client, 100);
        wait_until(|| lock_status(&status).view.packets_received == 1);
        send(&client, 0);
        wait_until(|| lock_status(&status).view.packets_received == 2);
        send(&client, 1);
        wait_until(|| lock_status(&status).view.packets_received == 3);
        assert_eq!(lock_status(&status).view.packets_invalid, 0);
        wait_until(|| lock_status(&status).view.connection == ConnectionState::Disconnected);
        send(&client, 0);
        wait_until(|| lock_status(&status).view.packets_received == 4);
        let new_socket = socket();
        send(&new_socket, 0);
        wait_until(|| lock_status(&status).view.packets_received == 5);
        assert_eq!(
            lock_status(&status).view.connection,
            ConnectionState::Connected
        );
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }
    #[test]
    fn paired_phone_waiting_for_first_audio_is_not_marked_disconnected() {
        let listener = socket();
        let auth = Arc::new(Mutex::new(AuthGate::new(Some(Ipv4Addr::LOCALHOST))));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(SharedStatus::default()));
        let (tx, _rx) = bounded(10);
        let (a, s, v) = (auth.clone(), stop.clone(), status.clone());
        let handle =
            thread::spawn(move || receive_audio(listener, a, Duration::from_millis(30), s, tx, v));
        wait_until(|| lock_status(&status).view.recovery.is_some());
        assert_eq!(
            lock_status(&status).view.connection,
            ConnectionState::Connecting
        );
        assert_eq!(lock_status(&status).view.packets_received, 0);
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }
    #[test]
    fn approval_resets_old_audio_and_stop_invalidates_pending_requests() {
        let state = DesktopState::default();
        let auth = Arc::new(Mutex::new(AuthGate::new(None)));
        let ip = Ipv4Addr::new(192, 168, 1, 42);
        auth.lock().unwrap().request(ip, "482913", "482913");
        let id = auth.lock().unwrap().pending().unwrap().request_id;
        state.runtime.lock().unwrap().auth = Some(auth.clone());
        {
            let mut shared = lock_status(&state.status);
            shared.view.listening = true;
            shared.view.connection = ConnectionState::Connected;
            shared.view.input_level = 0.8;
            shared.last_packet_at = Some(Instant::now());
        }
        respond_to_pairing_inner(&state, id, true).unwrap();
        {
            let shared = lock_status(&state.status);
            assert_eq!(shared.view.pairing_state, PairingState::Paired);
            assert_eq!(shared.view.connection, ConnectionState::Connecting);
            assert_eq!(shared.view.phone_address.as_deref(), Some("192.168.1.42"));
            assert_eq!(shared.view.packets_received, 0);
            assert_eq!(shared.view.input_level, 0.0);
            assert!(shared.last_packet_at.is_none());
        }
        let other = Ipv4Addr::new(192, 168, 1, 77);
        auth.lock().unwrap().request(other, "482913", "482913");
        let stale_id = auth.lock().unwrap().pending().unwrap().request_id;
        sync_pairing_status(&auth.lock().unwrap(), &state.status);
        stop_runtime(&state).unwrap();
        assert!(lock_status(&state.status).view.pending_pairing.is_none());
        assert_eq!(
            lock_status(&state.status).view.pairing_state,
            PairingState::Unpaired
        );
        assert!(respond_to_pairing_inner(&state, stale_id, true).is_err());
        let new_auth = Arc::new(Mutex::new(AuthGate::new(None)));
        new_auth.lock().unwrap().request(other, "482913", "482913");
        let new_id = new_auth.lock().unwrap().pending().unwrap().request_id;
        state.runtime.lock().unwrap().auth = Some(new_auth.clone());
        assert_ne!(new_id, stale_id);
        assert!(respond_to_pairing_inner(&state, stale_id, true).is_err());
        assert!(!new_auth.lock().unwrap().allows(other));
        respond_to_pairing_inner(&state, new_id, true).unwrap();
        assert!(new_auth.lock().unwrap().allows(other));
    }
}
