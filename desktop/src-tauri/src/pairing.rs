use rand::Rng;
use std::{
    fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub const DISCOVERY_PORT: u16 = 4951;
pub const WAITING_FOR_PHONE: &str =
    "Waiting for a phone: open Voxlink on the phone and tap Find my computer, or scan the QR code.";
pub fn unauthorized_sender_message(ip: Ipv4Addr) -> String {
    format!("Saw audio from {ip} but it is not paired. Approve its pairing request on this PC, or scan the QR code again from the phone.")
}
pub fn generate_code() -> String {
    rand::thread_rng().gen_range(100_000..1_000_000).to_string()
}
pub fn is_valid_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) && !code.starts_with('0')
}
pub fn code_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("voxlink/pairing-code")
}
pub fn save_code(path: &Path, code: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, code).map_err(|e| e.to_string())
}
pub fn load_or_create_code(path: &Path) -> Result<String, String> {
    if let Ok(text) = fs::read_to_string(path) {
        if is_valid_code(text.trim()) {
            return Ok(text.trim().to_owned());
        }
    }
    let code = generate_code();
    if let Err(error) = save_code(path, &code) {
        tracing::warn!(%error, "Pairing code could not be saved; using a temporary code");
    }
    Ok(code)
}
#[derive(Debug, PartialEq, Eq)]
pub enum DiscoveryPacket<'a> {
    Probe,
    Reply { code: &'a str, audio_port: u16 },
    Claim { code: &'a str },
    Ack,
    Unknown,
}
pub fn parse_discovery(bytes: &[u8]) -> DiscoveryPacket<'_> {
    match bytes {
        b"VXLD" => DiscoveryPacket::Probe,
        b"VXLA" => DiscoveryPacket::Ack,
        _ => {
            if (bytes.len() == 10 && bytes.starts_with(b"VXLC"))
                || (bytes.len() == 12 && bytes.starts_with(b"VXLR"))
            {
                if let Ok(code) = std::str::from_utf8(&bytes[4..10]) {
                    if code.bytes().all(|b| b.is_ascii_digit()) {
                        return if bytes.len() == 10 {
                            DiscoveryPacket::Claim { code }
                        } else {
                            DiscoveryPacket::Reply {
                                code,
                                audio_port: u16::from_be_bytes([bytes[10], bytes[11]]),
                            }
                        };
                    }
                }
            }
            DiscoveryPacket::Unknown
        }
    }
}
pub fn encode_reply(code: &str, audio_port: u16) -> [u8; 12] {
    let mut bytes = [0; 12];
    bytes[..4].copy_from_slice(b"VXLR");
    bytes[4..10].copy_from_slice(code.as_bytes());
    bytes[10..].copy_from_slice(&audio_port.to_be_bytes());
    bytes
}
pub fn encode_ack() -> [u8; 4] {
    *b"VXLA"
}
pub fn pairing_uri(bind_ip: &str, port: u16, code: &str) -> Result<String, String> {
    let ip = bind_ip
        .parse::<Ipv4Addr>()
        .map_err(|_| "Enter a valid PC Wi-Fi IPv4 address.".to_string())?;
    if !ip.is_private() || port < 1024 || port == DISCOVERY_PORT || !is_valid_code(code) {
        return Err("Choose a private PC Wi-Fi address and a valid audio port.".into());
    }
    Ok(format!("voxlink://{ip}:{port}?code={code}"))
}
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairing {
    pub request_id: u64,
    pub phone_ip: Ipv4Addr,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ClaimResponse {
    Approved,
    Pending,
    Rejected,
    Invalid,
}
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
const REJECTION_TIMEOUT: Duration = Duration::from_secs(30);
pub struct AuthGate {
    pinned: Option<Ipv4Addr>,
    authorized: Option<Ipv4Addr>,
    pending: Option<(PendingPairing, Instant)>,
    rejected: Option<(Ipv4Addr, Instant)>,
}
impl AuthGate {
    pub fn new(pinned: Option<Ipv4Addr>) -> Self {
        Self {
            pinned,
            authorized: None,
            pending: None,
            rejected: None,
        }
    }
    pub fn request(&mut self, source: Ipv4Addr, code: &str, current_code: &str) -> ClaimResponse {
        self.expire_pending();
        if let Some(pin) = self.pinned {
            return if pin == source {
                ClaimResponse::Approved
            } else {
                ClaimResponse::Rejected
            };
        }
        if !is_valid_code(code) || code != current_code {
            return ClaimResponse::Invalid;
        }
        if self.authorized == Some(source) {
            return ClaimResponse::Approved;
        }
        if self
            .rejected
            .is_some_and(|(ip, until)| ip == source && Instant::now() < until)
        {
            return ClaimResponse::Rejected;
        }
        if let Some((pending, _)) = &self.pending {
            return if pending.phone_ip == source {
                ClaimResponse::Pending
            } else {
                ClaimResponse::Rejected
            };
        }
        self.pending = Some((
            PendingPairing {
                request_id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
                phone_ip: source,
            },
            Instant::now() + APPROVAL_TIMEOUT,
        ));
        ClaimResponse::Pending
    }
    pub fn expire_pending(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            let (pending, _) = self.pending.take().unwrap();
            self.rejected = Some((pending.phone_ip, Instant::now() + REJECTION_TIMEOUT));
        }
    }
    pub fn pending(&self) -> Option<PendingPairing> {
        self.pending.as_ref().map(|(request, _)| request.clone())
    }
    pub fn respond(&mut self, request_id: u64, allow: bool) -> Result<Ipv4Addr, String> {
        self.expire_pending();
        let Some((request, _)) = self.pending.as_ref() else {
            return Err("This pairing request has expired. Ask the phone to try again.".into());
        };
        if request.request_id != request_id {
            return Err("This pairing request is no longer current.".into());
        }
        let ip = request.phone_ip;
        self.pending = None;
        if allow {
            self.authorized = Some(ip);
            self.rejected = None;
        } else {
            self.rejected = Some((ip, Instant::now() + REJECTION_TIMEOUT));
        }
        Ok(ip)
    }
    pub fn paired_phone(&self) -> Option<Ipv4Addr> {
        self.pinned.or(self.authorized)
    }
    pub fn allows(&self, source: Ipv4Addr) -> bool {
        self.pinned.or(self.authorized()) == Some(source)
    }
    pub fn authorized(&self) -> Option<Ipv4Addr> {
        self.authorized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_round_trips_code_and_audio_port() {
        let bytes = encode_reply("482913", 4950);
        assert_eq!(bytes.len(), 12);
        assert_eq!(&bytes[..4], b"VXLR");
        assert_eq!(&bytes[4..10], b"482913");
        assert_eq!(&bytes[10..12], &[0x13u8, 0x56]); // 4950 big-endian
        match parse_discovery(&bytes) {
            DiscoveryPacket::Reply { code, audio_port } => {
                assert_eq!(code, "482913");
                assert_eq!(audio_port, 4950);
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn ack_is_exactly_four_magic_bytes() {
        assert_eq!(encode_ack(), *b"VXLA");
        assert_eq!(parse_discovery(&encode_ack()), DiscoveryPacket::Ack);
    }

    #[test]
    fn probe_and_claim_parse_from_the_spec_layout() {
        assert_eq!(parse_discovery(b"VXLD"), DiscoveryPacket::Probe);
        let claim: [u8; 10] = *b"VXLC482913";
        match parse_discovery(&claim) {
            DiscoveryPacket::Claim { code } => assert_eq!(code, "482913"),
            other => panic!("expected Claim, got {other:?}"),
        }
    }

    #[test]
    fn unknown_and_malformed_packets_are_ignored() {
        let audio_packet: [u8; 12] = *b"VXL1\0\0\0\0\0\0\xBB\x80";
        assert_eq!(parse_discovery(&audio_packet), DiscoveryPacket::Unknown);
        assert_eq!(parse_discovery(b"VXL"), DiscoveryPacket::Unknown);
        assert_eq!(parse_discovery(b"VXLR"), DiscoveryPacket::Unknown); // short reply
        assert_eq!(parse_discovery(b"VXLC12ab!"), DiscoveryPacket::Unknown); // non-digit code
        assert_eq!(parse_discovery(b"NOPE"), DiscoveryPacket::Unknown);
        assert_eq!(parse_discovery(b""), DiscoveryPacket::Unknown);
    }

    #[test]
    fn generated_code_is_six_digits_in_range() {
        for _ in 0..64 {
            let code = generate_code();
            assert!(is_valid_code(&code), "generated invalid code {code}");
        }
        assert!(is_valid_code("100000"));
        assert!(is_valid_code("999999"));
        assert!(!is_valid_code("48291"));
        assert!(!is_valid_code("0482913"));
        assert!(!is_valid_code("48291a"));
        assert!(!is_valid_code(""));
    }

    #[test]
    fn load_or_create_persists_then_reuses() {
        let dir = std::env::temp_dir().join(format!("voxlink-plan-persist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("voxlink").join("pairing-code");
        let first = load_or_create_code(&path).expect("first call should succeed");
        let written = fs::read_to_string(&path).expect("file should exist after first call");
        assert_eq!(written, first);
        let second = load_or_create_code(&path).expect("second call should succeed");
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_code_creates_parent_directories() {
        let dir =
            std::env::temp_dir().join(format!("voxlink-plan-save-{}/nested", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("pairing-code");
        save_code(&path, "482913").expect("save should create parents");
        assert_eq!(fs::read_to_string(&path).expect("file exists"), "482913");
        let _ = fs::remove_dir_all(
            std::env::temp_dir().join(format!("voxlink-plan-save-{}", std::process::id())),
        );
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    #[test]
    fn approval_is_explicit_and_retries_are_idempotent() {
        let a = Ipv4Addr::new(192, 168, 1, 42);
        let b = Ipv4Addr::new(192, 168, 1, 77);
        let mut auth = AuthGate::new(None);
        assert_eq!(auth.request(a, "000000", "482913"), ClaimResponse::Invalid);
        assert!(auth.pending().is_none());
        assert_eq!(auth.request(a, "482913", "482913"), ClaimResponse::Pending);
        let request = auth.pending().unwrap();
        assert!(!auth.allows(a));
        assert_eq!(auth.request(a, "482913", "482913"), ClaimResponse::Pending);
        assert_eq!(auth.pending().unwrap(), request);
        assert!(auth.respond(request.request_id + 1, true).is_err());
        auth.respond(request.request_id, true).unwrap();
        assert!(auth.allows(a));
        assert_eq!(auth.request(a, "482913", "482913"), ClaimResponse::Approved);
        assert!(auth.pending().is_none());
        assert_eq!(auth.request(b, "482913", "482913"), ClaimResponse::Pending);
        assert!(auth.allows(a));
        assert!(!auth.allows(b));
        auth.respond(auth.pending().unwrap().request_id, true)
            .unwrap();
        assert!(!auth.allows(a));
        assert!(auth.allows(b));
    }
    #[test]
    fn rejected_and_expired_requests_never_authorize_or_reprompt_retries() {
        let ip = Ipv4Addr::new(192, 168, 1, 42);
        let mut auth = AuthGate::new(None);
        auth.request(ip, "482913", "482913");
        let first = auth.pending().unwrap().request_id;
        auth.respond(first, false).unwrap();
        assert!(!auth.allows(ip));
        assert_eq!(
            auth.request(ip, "482913", "482913"),
            ClaimResponse::Rejected
        );
        assert!(auth.pending().is_none());
        auth.rejected = None;
        auth.request(ip, "482913", "482913");
        let second = auth.pending().unwrap().request_id;
        assert_ne!(first, second);
        assert!(auth.respond(first, true).is_err());
        auth.pending.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        assert!(auth.respond(second, true).is_err());
        assert!(!auth.allows(ip));
        assert!(auth.pending().is_none());
    }
    #[test]
    fn unreadable_or_invalid_persistence_does_not_block_startup() {
        let parent =
            std::env::temp_dir().join(format!("voxlink-unwritable-{}", std::process::id()));
        fs::write(&parent, b"not a directory").unwrap();
        assert!(is_valid_code(
            &load_or_create_code(&parent.join("pairing-code")).unwrap()
        ));
        fs::remove_file(parent).unwrap();
    }
    #[test]
    fn pinned_mode_only_acks_and_accepts_the_pin() {
        let a = Ipv4Addr::new(192, 168, 1, 42);
        let b = Ipv4Addr::new(192, 168, 1, 77);
        let mut auth = AuthGate::new(Some(a));
        assert!(auth.allows(a));
        assert!(!auth.allows(b));
        assert_eq!(auth.request(a, "wrong", "482913"), ClaimResponse::Approved);
        assert_eq!(auth.request(b, "482913", "482913"), ClaimResponse::Rejected);
        assert_eq!(auth.authorized(), None);
    }
    #[test]
    fn pairing_uri_formats_the_canonical_payload_and_rejects_invalid_settings() {
        assert_eq!(
            pairing_uri("192.168.1.5", 4950, "482913").unwrap(),
            "voxlink://192.168.1.5:4950?code=482913"
        );
        for host in ["", "not-an-ip", "127.0.0.1", "0.0.0.0"] {
            assert!(pairing_uri(host, 4950, "482913").is_err());
        }
        assert!(pairing_uri("192.168.1.5", 4951, "482913").is_err());
        assert!(
            unauthorized_sender_message(Ipv4Addr::new(192, 168, 1, 77)).contains("192.168.1.77")
        );
    }
}
