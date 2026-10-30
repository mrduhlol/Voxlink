# Voxlink Pairing & Auto-Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two-address pairing flow with auto-discovery + a persisted 6-digit pairing code + QR, on the desktop (Tauri) receiver and the Android sender, then publish a release with updated docs (no version bumping — owner decision).

**Architecture:** A new UDP discovery protocol (`VXLD/VXLR/VXLC/VXLA`) runs on fixed port 4951 in a responder thread that lives exactly as long as the listener. The desktop learns the phone's IP itself (auto mode) or keeps the existing strict pin (manual mode, byte-identical audio behavior). Pure protocol/policy logic lives in new focused modules (`pairing.rs` on Rust, `PairingProtocol.kt` on Android) so it is unit-testable without sockets, threads, or Android framework; the existing `lib.rs` / `MainActivity.kt` keep only wiring. Audio (`VXL1`) and `receiver/` are untouched.

**Tech Stack:** Rust / Tauri 2 (crossbeam, existing UDP+PulseAudio threads), React+Vite+TypeScript, `qrcode` npm; Kotlin (programmatic views, no appcompat), CameraX 1.3.4 + ZXing 3.5.3, JUnit 4.13.2; Python 3 stdlib for the test responder.

**Spec:** `docs/superpowers/specs/2026-10-30-pairing-connection-design.md` — every value below is copied from it; where the spec left a choice, the choice is already recorded there or pinned here as a builder call.

## Implementation record — 2026-10-30

Implemented desktop pairing, persisted code, optional manual IP lock, discovery responder, QR display, Android discovery/claim/QR scanner, localhost validation, docs, and the README APK button. The original task checklist below is retained as planning history; this record describes actual verification.

Validation: frontend production build passed; 17 Rust unit/integration tests passed (including real UDP discovery and unauthorized audio); 5 Android protocol test groups passed and debug APK assembled; browser checks passed at 1120px and the 780px minimum window width (QR, stop/refresh behavior, disabled refresh while listening, no JavaScript errors); Python responder smoke passed.

Implementation decisions: pairing controls sit within Stream setup; Refresh requires stopping the listener so the displayed and responder codes cannot diverge. AndroidX is enabled for CameraX. The existing app versions, release workflow and artifact names are unchanged. Camera/emulator and physical phone audio checks remain unverified: device_open reported its FFmpeg/libx264 prerequisite unavailable. The mechanical UI detector only flagged the pre-existing Inter font, which was preserved.

## Global Constraints

- Discovery port is fixed **4951** on `0.0.0.0`; audio port range 1024..=65535 but must **not equal 4951** → reject with `Port 4951 is reserved for discovery — pick another UDP port for audio.`; 4951 bind failure → `UDP port 4951 is already in use — another Voxlink may be running.`
- Wire formats (fixed sizes, big-endian): `VXLD` 4B magic; `VXLR` 12B = magic + 6×ASCII code + u16 BE audio port; `VXLC` 10B = magic + 6×ASCII code; `VXLA` 4B magic. Unknown magics on the discovery port are ignored silently and must never touch `packets_invalid`/`packets_filtered`/audio stats (separate socket + thread guarantees this structurally).
- Pairing code: 6 decimal digits, 100000–999999, OS-CSPRNG (`rand = "0.8"`, `gen_range(100_000..1_000_000)`), **persisted** at `$XDG_CONFIG_HOME/voxlink/pairing-code` (fallback `~/.config/voxlink/pairing-code`), generated on first run, reused every launch; only the Refresh action regenerates.
- Claim burst (phone): up to ×5 `VXLC` at ~140 ms to port 4951, wait ≤700 ms for `VXLA`; ack → proceed; no ack → stream anyway; no code → stream as today.
- Find (phone): `VXLD` ×4 at ~250 ms to `255.255.255.255:4951` **and** each site-local IPv4 interface's /24 subnet broadcast (`<ip & 0xffffff00>.255`); collect replies ~1.2 s; 1 reply → auto-fill, >1 → single-choice chooser `ip — code`, 0 → `No PC found. Is Voxlink listening? Same Wi-Fi? Or use Scan QR / enter the address.`
- Exact user-facing strings (copy verbatim):
  - Desktop recovery (auto, listening, never paired): `Waiting for a phone: open Voxlink on the phone and tap Find my computer, or scan the QR code.`
  - Desktop recovery (first unauthorized sender only, no per-packet flicker): `Saw audio from {ip} but it is not paired. Enter this PC's pairing code on the phone, or set Phone IP manually.`
  - Desktop connection copy: `Pairing code {code} · waiting for phone` (auto, listening) → `Paired {ip}` (auto, connected); `Locked to {ip}` (manual, listening).
  - Desktop Phone IP help: `Leave empty to auto-pair (recommended). Set a specific IP only to lock to one known phone.`
  - Desktop pairing hint (idle): `Start listening to pair`.
  - Phone rejection: `That address is this phone (or localhost). Use your computer's Wi-Fi IP — the PC shows it, or tap Find my computer.`
  - Phone statuses: `Found PC {ip} · code {code}` / `Scanned PC {ip}` / `Paired with {ip}` / `No pairing ack — is Voxlink listening on the PC?` / `Streaming to {ip} — pair with a code for auto mode.` / `Not a Voxlink QR code` (toast).
- QR payload: `voxlink://<bind_ip>:<port>?code=<6 digits>` — built once in Rust, parsed in Kotlin; both sides pinned to the same literal in tests.
- Android toolchain: Kotlin 2.0.21, AGP 8.7.3, compileSdk 35, minSdk 23, Java 17, programmatic views (no XML layouts, no appcompat), Gradle ≥ 8.9 (locally pinned 8.14.6 via mise). New deps allowed **only**: `androidx.camera:camera-camera2|camera-lifecycle|camera-view:1.3.4`, `com.google.zxing:core:3.5.3`, `junit:junit:4.13.2` (test scope).
- Desktop: new runtime dep **only** `rand = "0.8"`; frontend dep **only** `qrcode` (+ dev `@types/qrcode`). `desktop/package-lock.json` and `desktop/src-tauri/Cargo.lock` must be updated **and committed** — CI runs `npm ci`.
- `receiver/` crate and the `VXL1` audio format are untouched; manual-pin mode audio filtering stays byte-identical to v0.2.0.
- Marketing pages (`site/`, `website/`) are out of scope.
- Release (owner decision): **no version bumping anywhere** — version fields (`0.2.0` / `versionCode 2`) and `.github/workflows/release.yml` stay untouched, so artifacts keep their current names (`Voxlink-android-debug.apk`, `Voxlink_0.2.0_amd64.AppImage`). The release is triggered only by a fresh `v*` tag (the workflow's trigger pattern).

## Review Focus

Five conditions the spec implies but no happy-path test exercises, each pinned to the task that owns the code:

1. **User sets the audio port to 4951 (collides with discovery)** → config must be rejected with the reserved-port message, and a second Voxlink's held 4951 must fail `start_stream` loudly, not hang. Tests: Task 4 `audio_port_4951_is_reserved_for_discovery`; Task 5 `discovery_bind_fails_loudly_when_port_is_taken`.
2. **Claim with a wrong or stale code** → no `VXLA` reaches the phone, its audio stays filtered, and a later correct-code claim unlocks it. Tests: Task 2 `wrong_code_does_not_authorize_and_a_later_correct_code_does`; Task 5 `claim_with_wrong_code_gets_no_ack`.
3. **Unauthorized phone streams anyway (the original silent-failure)** → `packets_filtered` counts every packet but the recovery diagnostic is written exactly once (no flicker) and names the offending IP. Test: Task 4 `unauthorized_audio_sets_a_single_stable_diagnostic`.
4. **User types `localhost`/`127.0.0.1`/`0.0.0.0`/link-local as the PC address on the phone** (the user's actual bug) → rejected before any socket work, with the exact targeted message. Test: Task 7 `validate_destination_rejects_localhost_loopback_zero_and_link_local`.
5. **QR payload drift between the desktop builder and the phone parser** → both pinned to the literal `voxlink://192.168.1.5:4950?code=482913`. Tests: Task 3 `pairing_uri_formats_the_canonical_payload`; Task 7 `parse_pairing_uri_accepts_the_desktop_payload`.

## File Map

| File | Action | Responsibility |
|---|---|---|
| `desktop/src-tauri/src/pairing.rs` | Create | Pure pairing logic: packet codecs, code persistence, `AuthGate`, `pairing_uri`, pairing copy constants. No threads, no Tauri types. |
| `desktop/src-tauri/src/lib.rs` | Modify | Wiring only: `validate_config`, `receive_audio` filtering, discovery responder thread, startup code load, two new commands. |
| `desktop/src-tauri/Cargo.toml` + `Cargo.lock` | Modify | Add `rand = "0.8"` (version fields unchanged). |
| `desktop/src/App.tsx` | Modify | Pairing card, `pairingCode`/`pairingUri` state, optional Phone IP copy, connection copy. |
| `desktop/src/styles.css` | Modify | `.pairing-*` styles matching the existing panel visual language. |
| `desktop/package.json` + `package-lock.json` | Modify | Add `qrcode`/`@types/qrcode` (version field unchanged). |
| `android/.../sender/PairingProtocol.kt` | Create | Pure JVM protocol + validation: build/parse packets, `discover`, `claimWithAck`, URI parse, destination validation, subnet broadcast. |
| `android/.../sender/MainActivity.kt` | Modify | UI wiring: Find button, pairing-code field, claim step, prefs, statuses. |
| `android/.../sender/ScannerActivity.kt` | Create | CameraX preview + ZXing QR decode → `setResult`. |
| `android/app/src/main/AndroidManifest.xml` | Modify | CAMERA permission, `uses-feature` camera not required, `ScannerActivity` declaration (Task 9). |
| `android/app/build.gradle` | Modify | Add `dependencies` block (junit in 7; camera+zxing in 9). |
| `android/app/src/test/java/com/voxlink/sender/PairingProtocolTest.kt` | Create | JUnit tests for `PairingProtocol`. |
| `scripts/test/fake-pc.py` | Create | Test-only scripted desktop: `responder` (answers VXLD/VXLC, optional VXL1 sink) and `probe` (sends VXLD, prints VXLR). |
| `README.md`, `docs/installation-guide.md`, `docs/omarchy-desktop.md` | Modify | New pairing flow + manual-mode section (Task 11). The prominent **Download Android APK** button at the top of the README Downloads section is already added — keep it. |
| `.github/workflows/release.yml` | No change | Stays untouched (no version bump); its `Voxlink_0.2.0_amd64.AppImage` path remains correct. |

---

### Task 1: Pairing packet codecs + persisted pairing code (Rust)

**Files:**
- Create: `desktop/src-tauri/src/pairing.rs`
- Modify: `desktop/src-tauri/src/lib.rs` (add `mod pairing;` at the top), `desktop/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: nothing (first task).
- Produces (used by Tasks 2–5):
  ```rust
  pub const DISCOVERY_PORT: u16 = 4951;
  pub fn generate_code() -> String;                       // 6 digits, 100000..=999999, CSPRNG
  pub fn is_valid_code(code: &str) -> bool;
  pub fn code_path() -> PathBuf;                          // XDG config dir / voxlink/pairing-code
  pub fn load_or_create_code(path: &Path) -> Result<String, String>; // reads, or generates + persists
  pub fn save_code(path: &Path, code: &str) -> Result<(), String>;  // creates parent dirs
  pub fn encode_reply(code: &str, audio_port: u16) -> [u8; 12];
  pub fn encode_ack() -> [u8; 4];
  pub enum DiscoveryPacket<'a> { Probe, Reply { code: &'a str, audio_port: u16 },
                                 Claim { code: &'a str }, Ack, Unknown }
  pub fn parse_discovery(bytes: &[u8]) -> DiscoveryPacket<'_>;
  ```
  Note (builder call, YAGNI): Rust never *sends* `VXLD`/`VXLC`, so there is no `encode_probe`/`encode_claim` in production code — tests build those bytes literally to pin the format.

- [ ] **Step 1: Write the failing tests** — add `#[cfg(test)] mod tests` at the bottom of `pairing.rs` with:
  - `reply_round_trips_code_and_audio_port`: `encode_reply("482913", 4950)` has len 12, starts `b"VXLR"`, bytes 4..10 are ASCII `482913`, bytes 10..12 are `4950u16.to_be_bytes()`; `parse_discovery` of it returns `Reply { code: "482913", audio_port: 4950 }`.
  - `ack_is_exactly_four_magic_bytes`: `encode_ack() == *b"VXLA"`.
  - `probe_and_claim_parse_from_the_spec_layout`: `parse_discovery(b"VXLD") == Probe`; a literal 10-byte `b"VXLC"` + `482913` returns `Claim { code: "482913" }`.
  - `unknown_and_malformed_packets_are_ignored`: `parse_discovery` returns `Unknown` for `b"VXL1\0\0\0\0\0\0\xBB\x80"` (audio packet), `b"VXL"`, `b"VXLR"` (short reply), `b"VXLC12ab!"` (non-digit code), `b"NOPE"`, and `b""`.
  - `generated_code_is_six_digits_in_range`: `generate_code()` passes `is_valid_code`; assert `is_valid_code` accepts `"100000"`/`"999999"`, rejects `"48291"`/`"0482913"`/`"48291a"`/`""`.
  - `load_or_create_persists_then_reuses`: under a fresh temp dir path (`std::env::temp_dir().join(format!("voxlink-plan-{}", std::process::id()))`, removed at start), first `load_or_create_code` generates and writes the file (file contents == returned value), second call returns the same string.
  - `save_code_creates_parent_directories`: save to `<tmp>/nested/pairing-code`, assert file exists with the code.

- [ ] **Step 2: Run the tests, expect failure**
  Run: `cd desktop/src-tauri && cargo test pairing`
  Expected: FAIL — `file not found for module pairing` (or `cannot find function` if the module exists but is empty).

- [ ] **Step 3: Implement `pairing.rs`**
  Add `mod pairing;` to `lib.rs`. Add `rand = "0.8"` to `[dependencies]`. Implement the interface above: `generate_code` via `rand::thread_rng().gen_range(100_000..1_000_000).to_string()`; `code_path` honors `XDG_CONFIG_HOME` (must be absolute) else `~/.config`, joined with `voxlink/pairing-code`; `load_or_create_code` reads + trims + validates with `is_valid_code`, otherwise generates and `save_code`s (on persist failure still return the generated code — a read-only config dir must not brick startup); `parse_discovery` is a pure length+magic match returning `Unknown` for anything else.

- [ ] **Step 4: Run the tests, expect pass**
  Run: `cd desktop/src-tauri && cargo test pairing && cargo check`
  Expected: PASS (all 7 tests), `cargo check` clean.

- [ ] **Step 5: Commit**
  ```bash
  git add desktop/src-tauri/src/pairing.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock
  git commit -m "Add pairing protocol codecs and persisted pairing code"
  ```

---

### Task 2: `AuthGate` authorization state machine

**Files:**
- Modify: `desktop/src-tauri/src/pairing.rs`

**Interfaces:**
- Consumes: `is_valid_code` (Task 1).
- Produces (used by Tasks 4–5):
  ```rust
  pub struct AuthGate { /* mode + authorized */ }
  impl AuthGate {
      pub fn new(phone_ip: Option<Ipv4Addr>) -> Self;   // None => auto, Some => pinned
      pub fn claim(&mut self, claimer: Ipv4Addr, code: &str, current_code: &str) -> bool;
      pub fn allows(&self, source: Ipv4Addr) -> bool;
      pub fn authorized(&self) -> Option<Ipv4Addr>;
  }
  ```
  Semantics (from spec §2/§1 mode behavior): `claim` in auto mode returns true and sets `authorized = Some(claimer)` iff `code == current_code` (a correct code re-authorizes, i.e. the code *is* the secret); in pinned mode it returns `claimer == pinned` and ignores the code (the ack must stay truthful — the desktop would filter audio to the pin anyway). `allows` in pinned mode is `source == pin`; in auto mode `source == authorized` (`None` → nothing allowed until claimed).

- [ ] **Step 1: Write the failing tests** — extend `pairing.rs` tests:
  - `auto_mode_starts_unauthorized`: `AuthGate::new(None)` → `authorized() == None`, `allows(ip) == false` for any IP.
  - `correct_code_authorizes_the_claimer`: `claim(192.168.1.42, "482913", "482913")` → true; `allows(192.168.1.42)` → true; `allows(192.168.1.77)` → false.
  - `wrong_code_does_not_authorize_and_a_later_correct_code_does`: claim with `"000000"` → false, still unauthorized; then claim with `"482913"` → true.
  - `reclaim_with_correct_code_moves_the_lock`: authorize A, then `claim(B, correct, correct)` → true, `allows(B)` true, `allows(A)` false.
  - `pinned_mode_allows_only_the_pin`: `AuthGate::new(Some(192.168.1.42))` → `allows(192.168.1.42)`, rejects `192.168.1.77`, `authorized()` stays `None`.
  - `pinned_mode_acks_only_the_pin_regardless_of_code`: `claim(192.168.1.42, "wrong", "482913")` → true; `claim(192.168.1.77, "482913", "482913")` → false.

- [ ] **Step 2: Run the tests, expect failure**
  Run: `cd desktop/src-tauri && cargo test auth`
  Expected: FAIL — `cannot find type AuthGate`.

- [ ] **Step 3: Implement `AuthGate` in `pairing.rs`**
  A struct holding `enum { Auto, Pinned(Ipv4Addr) }` + `authorized: Option<Ipv4Addr>`; implement the four methods with the semantics above (no locking inside — callers wrap it in `Arc<Mutex<_>>`).

- [ ] **Step 4: Run the tests, expect pass**
  Run: `cd desktop/src-tauri && cargo test pairing`
  Expected: PASS (all Task 1 + Task 2 tests).

- [ ] **Step 5: Commit**
  ```bash
  git add desktop/src-tauri/src/pairing.rs
  git commit -m "Add pairing authorization state machine"
  ```

---

### Task 3: Pairing code surface — status field, URI builder, commands

**Files:**
- Modify: `desktop/src-tauri/src/pairing.rs` (add `pairing_uri` + copy constant), `desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 (`code_path`, `load_or_create_code`, `generate_code`, `save_code`).
- Produces (consumed by Task 4 startup glue and Task 6 frontend):
  ```rust
  // pairing.rs
  pub fn pairing_uri(bind_ip: &str, port: u16, code: &str) -> Result<String, String>;
  pub const WAITING_FOR_PHONE: &str =
      "Waiting for a phone: open Voxlink on the phone and tap Find my computer, or scan the QR code.";
  pub fn unauthorized_sender_message(ip: Ipv4Addr) -> String; // spec's "Saw audio from {ip}…" line
  // lib.rs — StatusView gains: pairing_code: String
  // new Tauri commands (registered in generate_handler![]):
  #[tauri::command] fn get_pairing_uri(state: State<DesktopState>, bind_ip: String, port: u16) -> Result<String, String>;
  #[tauri::command] fn refresh_pairing_code(state: State<DesktopState>) -> Result<String, String>;
  ```

- [ ] **Step 1: Write the failing tests** — in `pairing.rs` tests:
  - `pairing_uri_formats_the_canonical_payload`: `pairing_uri("192.168.1.5", 4950, "482913") == Ok("voxlink://192.168.1.5:4950?code=482913")`.
  - `pairing_uri_rejects_an_unparseable_bind_ip`: `pairing_uri("not-an-ip", 4950, "482913")` is `Err`, and `pairing_uri("", 4950, "482913")` is `Err`.
  - `unauthorized_message_names_the_source_ip`: `unauthorized_sender_message(Ipv4Addr::new(192,168,1,77))` contains `192.168.1.77` and the substring `not paired`.

- [ ] **Step 2: Run the tests, expect failure**
  Run: `cd desktop/src-tauri && cargo test pairing_uri`
  Expected: FAIL — `cannot find function pairing_uri`.

- [ ] **Step 3: Implement `pairing_uri` + copy helpers in `pairing.rs`** (format exactly `format!("voxlink://{bind_ip}:{port}?code={code}")`, `Err("Enter a valid PC Wi-Fi IPv4 address.")` when `bind_ip.parse::<Ipv4Addr>()` fails).

- [ ] **Step 4: Wire the status field and commands in `lib.rs`**
  - `StatusView` gains `pairing_code: String` (serializes as `pairingCode` — already `camelCase`).
  - In `run()`: build the state first, load the code with `load_or_create_code(&code_path()).unwrap_or_else(|_| generate_code())`, store it into `lock_status(&state.status).view.pairing_code`, then `.manage(state)`.
  - `get_pairing_uri`: read `pairing_code` from status, call `pairing_uri`, return.
  - `refresh_pairing_code`: `generate_code()`, `save_code(&code_path(), &code)` (ignore persist failure for the return value but `warn!`), write into `status.view.pairing_code`, return the new code.
  - Register both in `generate_handler![]`.
  Run: `cd desktop/src-tauri && cargo check` → clean.

- [ ] **Step 5: Run all tests, expect pass**
  Run: `cd desktop/src-tauri && cargo test`
  Expected: PASS (everything so far).

- [ ] **Step 6: Commit**
  ```bash
  git add desktop/src-tauri/src/pairing.rs desktop/src-tauri/src/lib.rs
  git commit -m "Expose pairing code and QR payload through status and commands"
  ```

---

### Task 4: Optional phone IP (auto mode) + audio filtering with diagnostics

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs` (`validate_config`, `start_stream`, `receive_audio`)

**Interfaces:**
- Consumes: Tasks 2–3 (`AuthGate`, `WAITING_FOR_PHONE`, `unauthorized_sender_message`).
- Produces (consumed by Task 5): `auth: Arc<Mutex<AuthGate>>` created in `start_stream` and passed into `receive_audio`; `validate_config` now returns `(Ipv4Addr, Option<Ipv4Addr>)`.
  - `receive_audio(socket, auth: Arc<Mutex<AuthGate>>, timeout, stop, audio_sender, status)` — replaces the `expected_phone: Ipv4Addr` parameter.

- [ ] **Step 1: Write the failing tests** — extend `lib.rs` `mod tests`:
  - `empty_phone_ip_means_auto_mode`: `StartConfig { bind_ip: "192.168.1.5", phone_ip: "", port: 4950, timeout_ms: 2500 }` → `Ok((bind, None))`.
  - `non_empty_phone_ip_still_must_be_a_private_ipv4`: `phone_ip: "192.168.1.42"` → `Ok((_, Some(ip)))`; `"localhost"` → `Err`; `"8.8.8.8"` → `Err`; `"0.0.0.0"` → `Err` (reuse the existing private/unspecified rules).
  - `audio_port_4951_is_reserved_for_discovery`: `port: 4951` → `Err` with message exactly `Port 4951 is reserved for discovery — pick another UDP port for audio.`; `port: 65535` → `Ok`; `port: 1023` → `Err`.
  - `unauthorized_audio_sets_a_single_stable_diagnostic` (loopback): bind a `UdpSocket` on `127.0.0.1:0`, set a 50 ms read timeout, create `Arc<Mutex<AuthGate::new(None)>>`, `SharedStatus`, and a `bounded(10)` audio channel; spawn `receive_audio(...)`; send 3 minimal valid `VXL1` packets (magic + seq BE + `48000u32.to_be_bytes()` + 2 bytes PCM) from a second socket; poll ≤2 s until `packets_filtered == 3`; assert `packets_received == 0`, `recovery == Some(unauthorized_sender_message(127.0.0.1))` and that it did not change between the 1st and 3rd packet; then `auth.lock().claim(127.0.0.1, "482913", "482913")`, send one more packet, poll until `packets_received == 1`, assert `phone_address == Some("127.0.0.1")`.
  - `pinned_mode_filters_like_before`: same harness with `AuthGate::new(Some(192.168.1.99))` → a packet from `127.0.0.1` increments `packets_filtered`, never `packets_received`, regardless of any claim.
  - Stop each spawned thread by setting the stop flag and joining.

- [ ] **Step 2: Run the tests, expect failure**
  Run: `cd desktop/src-tauri && cargo test --lib`
  Expected: FAIL — compile errors (`StartConfig`/`receive_audio` signatures not yet changed, tests call `AuthGate` paths that exist but `validate_config` still returns two `Ipv4Addr`s).

- [ ] **Step 3: Change `validate_config`**
  Empty `phone_ip` (after trim) → `None`; non-empty keeps today's parse + private/loopback/unspecified checks with the existing messages; add the 4951 reserved-port check (keep the existing `< 1024` check and message, keep timeout bounds). Return `(Ipv4Addr, Option<Ipv4Addr>)`.

- [ ] **Step 4: Change `receive_audio` filtering**
  Replace the `peer.ip() != expected_phone` check with: non-IPv4 peer → filtered; `!auth.lock().unwrap_or_else(|p| p.into_inner()).allows(source)` → `packets_filtered += 1`, and **only the first time** (local `reported_unauthorized` flag) set `recovery = Some(unauthorized_sender_message(source))`; accepted path additionally sets `view.phone_address = Some(source.to_string())`. Everything else (parse, sequence accounting, queueing, timeout handling) unchanged.

- [ ] **Step 5: Change `start_stream` glue**
  `let (bind_ip, phone_ip) = validate_config(...)` (same error handling); create `let auth = Arc::new(Mutex::new(AuthGate::new(phone_ip)));`; pass to `receive_audio`; `shared.view.phone_address = phone_ip.map(|ip| ip.to_string())`; after `listening = true`, set `shared.view.recovery = phone_ip.is_none().then_some(WAITING_FOR_PHONE.to_string())` (replace the unconditional `recovery = None`); update the `info!` log to `?phone_ip`.

- [ ] **Step 6: Run all tests, expect pass**
  Run: `cd desktop/src-tauri && cargo test && cargo check`
  Expected: PASS (Tasks 1–4), clean check.

- [ ] **Step 7: Commit**
  ```bash
  git add desktop/src-tauri/src/lib.rs
  git commit -m "Make phone IP optional with auto-mode audio authorization"
  ```

---

### Task 5: Discovery responder thread + listener wiring

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 1–3 (`DISCOVERY_PORT`, `parse_discovery`, `encode_reply`, `encode_ack`, `code_path`/status code), Task 4 (`AuthGate` already shared with `receive_audio`).
- Produces (final desktop backend surface — Tasks 6+ rely on this running):
  ```rust
  fn bind_discovery_socket() -> Result<UdpSocket, String>;        // = bind_discovery_socket_on(DISCOVERY_PORT)
  fn bind_discovery_socket_on(port: u16) -> Result<UdpSocket, String>; // 0.0.0.0:port, 50 ms read timeout,
                                                                       // EADDRINUSE => "UDP port {port} is already in use — another Voxlink may be running."
  fn run_discovery_responder(socket: UdpSocket, audio_port: u16, code: String,
                             auth: Arc<Mutex<AuthGate>>, stop: Arc<AtomicBool>);
  ```
  `RuntimeHandles` gains `discovery_thread: Option<JoinHandle<()>>`.

- [ ] **Step 1: Write the failing tests** — extend `lib.rs` `mod tests` (each binds an **ephemeral** `127.0.0.1:0` port and passes it in — production pins 4951, tests must stay parallel-safe):
  - `responder_answers_probes_with_code_and_audio_port`: bind ephemeral, spawn `run_discovery_responder(sock, 4950, "482913", auth, stop)`; client sends `b"VXLD"`; expect a 12B reply `VXLR` + `482913` + `4950u16.to_be_bytes()`; assert `status`-free by construction — also assert a follow-up probe still answers (idempotent).
  - `claim_with_wrong_code_gets_no_ack`: with the same responder, send `VXLC`+`"000000"` ×2, then read with a 400 ms timeout → no `VXLA` arrives, and `auth.allows(127.0.0.1)` is false.
  - `claim_with_correct_code_gets_an_ack_and_authorizes`: send `VXLC`+`"482913"` → expect 4B `VXLA`, `auth.allows(127.0.0.1)` true; audio accepted afterwards.
  - `pinned_responder_acks_only_the_pin`: responder built with `AuthGate::new(Some(127.0.0.1))` + correct code → ack; with `AuthGate::new(Some(192.168.1.99))` → no ack.
  - `unknown_packets_are_ignored`: send `b"NOPE"` and a `VXL1` audio packet → nothing back within 300 ms.
  - `discovery_bind_fails_loudly_when_port_is_taken`: bind an ephemeral socket yourself, then `bind_discovery_socket_on(that_port)` → `Err` containing `already in use`.
  - `responder_thread_stops_on_the_stop_flag`: spawn, set stop, `join()` returns within 1 s (50 ms read timeout guarantees it).

- [ ] **Step 2: Run the tests, expect failure**
  Run: `cd desktop/src-tauri && cargo test --lib`
  Expected: FAIL — `cannot find function bind_discovery_socket_on`.

- [ ] **Step 3: Implement `bind_discovery_socket_on` and `run_discovery_responder`**
  Bind `0.0.0.0:port`, set 50 ms read timeout; map bind errors to the spec message. The responder loop: while `!stop`, `recv_from` with timeout tolerance (same `WouldBlock | TimedOut` arm as `receive_audio`); on `Probe` → `send_to(encode_reply(&code, audio_port), peer)`; on `Claim` → `auth.lock().claim(peer_ipv4, claim_code, &code)` and if true `send_to(encode_ack(), peer)`; on anything else → ignore. Non-IPv4 peers ignored.

- [ ] **Step 4: Wire into `start_stream` / `stop_runtime`**
  After the audio socket binds successfully: `let discovery = bind_discovery_socket()?` — on `Err`, unload sink, `set_start_error`, return `Err` (same pattern as the audio-bind failure). Spawn thread `voxlink-discovery` with the status's `pairing_code`, `config.port`, shared `auth` and the same `stop` flag; on spawn failure mirror the receiver-thread failure cleanup. Store the handle in `runtime.discovery_thread`. In `stop_runtime`: after stopping the flag, join `discovery_thread` (it shares the flag with the receiver, so it exits in ≤50 ms).

- [ ] **Step 5: Run all tests, expect pass**
  Run: `cd desktop/src-tauri && cargo test && cargo check`
  Expected: PASS (all Tasks 1–5).

- [ ] **Step 6: Manual smoke test on this host**
  Run: `cd desktop/src-tauri && cargo run &` (or `cd desktop && npm run tauri dev`), click **Start listening** with empty Phone IP, then from another terminal `python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.settimeout(1);s.sendto(b'VXLD',('127.0.0.1',4951));print(s.recv(12))"`.
  Expected: prints `b'VXLR'…` containing the on-screen pairing code; a second probe with `b'VXLC'+b'000000'` gets no reply; stop the app.

- [ ] **Step 7: Commit**
  ```bash
  git add desktop/src-tauri/src/lib.rs
  git commit -m "Answer discovery probes and claims while listening"
  ```

---

### Task 6: Desktop frontend — Pairing card, QR, optional Phone IP

**Files:**
- Modify: `desktop/package.json` + `desktop/package-lock.json`, `desktop/src/App.tsx`, `desktop/src/styles.css`

**Interfaces:**
- Consumes: Task 3 commands (`get_pairing_uri(bindIp, port) -> string`, `refresh_pairing_code() -> string`) and `status.pairingCode`; Task 4/5 backend behavior.
- Produces: none for later tasks (terminal UI task).

- [ ] **Step 1: Add the QR dependency**
  Run: `cd desktop && npm install qrcode && npm install -D @types/qrcode`
  Expected: `package.json` lists both; `package-lock.json` updated.

- [ ] **Step 2: Extend types and state in `App.tsx`**
  `DesktopStatus` gains `pairingCode: string`. New state: `pairingUri: string | null`, `qrDataUrl: string | null`. Effect on `[settings.bindIp, settings.port, status.pairingCode]`: skip when either field is empty/non-numeric, else `invoke<string>("get_pairing_uri", { bindIp: settings.bindIp.trim(), port: Number(settings.port) })` → set state, catch → `setPairingUri(null)`. Effect on `[pairingUri]`: `QRCode.toDataURL(pairingUri, { margin: 1, width: 160 })` → `setQrDataUrl`.

- [ ] **Step 3: Add the Pairing card**
  Insert a new `<section className="panel pairing-panel">` immediately after the setup panel; renumber the later eyebrows: setup stays `01 / CONNECTION`, pairing = `02 / PAIRING`, then `03 / LINUX INPUT`, `04 / STREAM`, `05 / PREFERENCES`. Card content: heading `Pair to your phone`; the code as `<strong className="pairing-code">{status.pairingCode}</strong>`; the QR `<img src={qrDataUrl} alt="Pairing QR code" />` when `qrDataUrl`, else `<p className="pairing-placeholder">Enter a valid PC address to show the QR code</p>`; a **Refresh code** button (`button-secondary`) invoking `refresh_pairing_code` then updating `status.pairingCode` from the returned value; hint `{active ? "Scan with Voxlink on your phone" : "Start listening to pair"}`; small print `Same code on every launch — only Refresh changes it.`. Add `.pairing-*` rules to `styles.css` reusing the panel/field/button classes' visual language.

- [ ] **Step 4: Update the Phone IP field and connection copy**
  - Phone IP `<small>` text → `Leave empty to auto-pair (recommended). Set a specific IP only to lock to one known phone.`
  - `connectionMessage` becomes: `backendError` first; then `status.connection === "connected"` → auto (`!settings.phoneIp.trim()`) ? `Paired ${status.phoneAddress ?? "your phone"}` : `Receiving live audio from ${status.phoneAddress ?? "your phone"}.`; then `active` → auto ? `` `Pairing code ${status.pairingCode} · waiting for phone` `` : `` `Locked to ${settings.phoneIp.trim()}` ``; else the existing idle text.

- [ ] **Step 5: Build and eyeball**
  Run: `cd desktop && npm run build`
  Expected: `tsc --noEmit` clean, vite build succeeds. Then `npm run tauri dev` and confirm: card shows a 6-digit code before listening, QR appears once a valid address is selected, Refresh changes both, Start listening with empty Phone IP now succeeds (the original bug), copy switches to `Pairing code … · waiting for phone`.

- [ ] **Step 6: Commit**
  ```bash
  git add desktop/package.json desktop/package-lock.json desktop/src/App.tsx desktop/src/styles.css
  git commit -m "Add desktop pairing card with code and QR"
  ```

---

### Task 7: Android protocol module + JUnit tests

**Files:**
- Create: `android/app/src/main/java/com/voxlink/sender/PairingProtocol.kt`, `android/app/src/test/java/com/voxlink/sender/PairingProtocolTest.kt`
- Modify: `android/app/build.gradle` (add `dependencies { testImplementation 'junit:junit:4.13.2' }`)

**Interfaces:**
- Consumes: the wire formats from Global Constraints (phone-side counterpart of Task 1).
- Produces (used by Tasks 8–9):
  ```kotlin
  object PairingProtocol {
      const val DISCOVERY_PORT = 4951
      data class Reply(val host: String, val audioPort: Int, val code: String)
      data class PairingTarget(val host: String, val port: Int, val code: String)
      fun buildProbe(): ByteArray                       // b"VXLD"
      fun buildClaim(code: String): ByteArray            // b"VXLC" + 6 digits; IllegalArgumentException if not 6 digits
      fun parseReply(data: ByteArray, length: Int, fromHost: String): Reply?
      fun isAck(data: ByteArray, length: Int): Boolean
      fun parsePairingUri(uri: String): PairingTarget?
      fun validateDestination(host: String): String?     // null = OK, else the exact rejection message
      fun subnetBroadcastOf(address: InetAddress): String?  // /24, e.g. 192.168.1.42 -> "192.168.1.255"
      fun discover(targets: List<String>, port: Int = DISCOVERY_PORT, windowMs: Long = 1200): List<Reply>
      fun claimWithAck(host: String, port: Int = DISCOVERY_PORT, code: String, timeoutMs: Long = 700): Boolean
  }
  ```

- [ ] **Step 1: Add the test dependency** — in `android/app/build.gradle` append:
  ```groovy
  dependencies { testImplementation 'junit:junit:4.13.2' }
  ```

- [ ] **Step 2: Write the failing tests** — `PairingProtocolTest.kt`:
  - `build_probe_is_the_vxld_magic`: `buildProbe().contentEquals(byteArrayOf(86,88,76,68))`.
  - `build_claim_encodes_magic_plus_six_digits`: len 10, `VXLC` + `482913`; `assertThrows(IllegalArgumentException) { buildClaim("4829") }` (JUnit 4.13 has `assertThrows`).
  - `parse_reply_accepts_the_canonical_layout`: bytes `VXLR` + `482913` + `4950.toShort().toBytesBE`, `length = 12`, `fromHost = "192.168.1.5"` → `Reply("192.168.1.5", 4950, "482913")`.
  - `parse_reply_rejects_wrong_magic_short_and_non_digit`: `NOPE…`, `length = 8`, and `VXLR` + `48291a` → all `null`.
  - `is_ack_matches_only_vxla`: `VXLA` (4B) true; `VXLD`, `VXLR…`, 3-byte `VXLA` → false.
  - `parse_pairing_uri_accepts_the_desktop_payload`: `parsePairingUri("voxlink://192.168.1.5:4950?code=482913") == PairingTarget("192.168.1.5", 4950, "482913")`.
  - `parse_pairing_uri_rejects_garbage`: `"https://192.168.1.5:4950?code=482913"`, `"voxlink://192.168.1.5"`, `"voxlink://192.168.1.5:99999?code=482913"`, `"voxlink://localhost:4950?code=482913"` (spec: validate IPv4 + port range; code required, 6 digits) → all `null`.
  - `validate_destination_rejects_localhost_loopback_zero_and_link_local`: `"localhost"`, `"127.0.0.1"`, `"127.1.2.3"`, `"0.0.0.0"`, `"169.254.10.10"` → non-null result **equal to** `That address is this phone (or localhost). Use your computer's Wi-Fi IP — the PC shows it, or tap Find my computer.`; `"192.168.1.5"`, `"10.0.2.2"` → null.
  - `subnet_broadcast_of_uses_slash_24`: `InetAddress.getByName("192.168.1.42")` → `"192.168.1.255"`; `10.0.2.15` → `"10.0.2.255"`.
  - `claim_with_ack_returns_true_when_the_responder_acks`: bind a test `DatagramSocket(0)` on the loopback, thread replies `VXLA` to any `VXLC` whose code is `482913`; `claimWithAck("127.0.0.1", localPort, "482913")` → true.
  - `claim_with_ack_returns_false_without_a_responder`: bound port, nobody answers; `claimWithAck("127.0.0.1", localPort, "482913", timeoutMs = 400)` → false and returns in well under 2 s.
  - `discover_returns_replies_from_targets`: test responder on an ephemeral loopback port answers `VXLD` with `VXLR`+`482913`+`4950`; `discover(listOf("127.0.0.1"), port = localPort, windowMs = 600)` → single `Reply("127.0.0.1", 4950, "482913")`.

- [ ] **Step 3: Run the tests, expect failure**
  Run: `gradle -p android :app:testDebugUnitTest`
  Expected: FAIL — `Unresolved reference: PairingProtocol`.

- [ ] **Step 4: Implement `PairingProtocol.kt`**
  Pure `java.*` code (no `android.*`) so it runs on the JVM test runner. Details the signatures don't fix: `discover` sets `broadcast = true` on its socket, sends the probe to every target at ~250 ms spacing (skip sleeps once `windowMs` elapses), collects `VXLR` replies until the deadline, dedupes by host; `claimWithAck` sends `buildClaim(code)` up to 5× at ~140 ms, returns true on the first `VXLA`, false after `timeoutMs`; each call uses its own short-lived `DatagramSocket` (an unconnected socket, so ICMP port-unreachable on legacy desktops cannot poison a later send). `parsePairingUri` uses `java.net.URI` + strict4-octet IPv4 + port 1..65535 + `is_valid` 6-digit code.

- [ ] **Step 5: Run the tests, expect pass**
  Run: `gradle -p android :app:testDebugUnitTest`
  Expected: PASS (all ~14 tests).

- [ ] **Step 6: Commit**
  ```bash
  git add android/app/build.gradle android/app/src/main/java/com/voxlink/sender/PairingProtocol.kt android/app/src/test/java/com/voxlink/sender/PairingProtocolTest.kt
  git commit -m "Add Android pairing protocol module with tests"
  ```

---

### Task 8: Android UI — Find my computer, pairing-code field, claim on start

**Files:**
- Modify: `android/app/src/main/java/com/voxlink/sender/MainActivity.kt`

**Interfaces:**
- Consumes: all of `PairingProtocol` (Task 7).
- Produces: none (UI task; Task 9 adds the Scan button beside Find).

- [ ] **Step 1: Add the pairing-code field and Find button to the connection card**
  In `buildControls()`'s connection card: add a **Find my computer** button (use the existing `action(...)` helper, full width) **above** the `COMPUTER IP ADDRESS` label; after the `UDP PORT` field add label `PAIRING CODE` + `codeInput = input(saved.getString("code", "") ?: "", "shown on the Voxlink PC", true)` (numeric, 6 digits — set `filters`/length hint in code: `inputFilters = arrayOf(android.text.InputFilter.LengthFilter(6))`), height `dp(54)` like the others, `imeOptions = IME_ACTION_DONE`. Keep the existing "Both devices must be on the same Wi-Fi network." line.

- [ ] **Step 2: Implement the Find flow**
  The button spawns a background thread: collect targets = `["255.255.255.255"]` plus `subnetBroadcastOf(addr)` for every up, non-loopback, site-local IPv4 interface address (reuse the enumeration pattern already in `refreshAddresses`); call `PairingProtocol.discover(targets)`; marshal to UI:
  - exactly 1 reply → `addressInput.setText(host)`, `portInput.setText(audioPort)`, `codeInput.setText(code)`, `setStatus("Found PC ${host} · code ${code}", limeColor)`;
  - >1 → `AlertDialog` single-choice listing `"${host} — ${code}"`, on pick fill the three fields (same as above);
  - 0 → `setStatus("No PC found. Is Voxlink listening? Same Wi-Fi? Or use Scan QR / enter the address.", orangeColor)`.
  Disable the Find button while running (`running` guard like `Choose saved computer`).

- [ ] **Step 3: Validate the destination in `requestStart`**
  After the existing empty/port check, call `PairingProtocol.validateDestination(address)`; if non-null → `setStatus(it, Color.rgb(237,189,102))` and return (before any permission/socket work).

- [ ] **Step 4: Claim burst before streaming**
  Thread the code through: `startRecording(address, port)` reads `codeInput` (`null`/blank → null) and passes it to `streamAudio(address, port, code)`. At the top of the worker (after `running = true`, before creating the recorder):
  - `code != null` → UI `setStatus("Pairing with $host…", blue)`; `val acked = PairingProtocol.claimWithAck(host, PairingProtocol.DISCOVERY_PORT, code)`; if acked → `setStatus("Paired with $host", limeColor)` and let the existing `showStatus("Sending …")` line follow; if not → `setStatus("No pairing ack — is Voxlink listening on the PC?", orange)` and **suppress** the subsequent `showStatus("Sending …")` so the warning stays visible.
  - `code == null` → replace the existing `showStatus("Sending ${rate} Hz mono audio to $host:$port")` with `showStatus("Streaming to $host — pair with a code for auto mode.")`.
  - Packet loop, `VXL1` format, mute/gain/gate, stats: unchanged.

- [ ] **Step 5: Persist and toggle the new field**
  In `startRecording` prefs write add `putString("code", codeInput.text.toString().trim())`; disable `codeInput` alongside `addressInput`/`portInput` while running and re-enable it in the worker's `finally`.

- [ ] **Step 6: Build and unit tests**
  Run: `gradle -p android :app:testDebugUnitTest && gradle -p android :app:assembleDebug`
  Expected: tests PASS; APK at `android/app/build/outputs/apk/debug/app-debug.apk`.

- [ ] **Step 7: Commit**
  ```bash
  git add android/app/src/main/java/com/voxlink/sender/MainActivity.kt
  git commit -m "Add Find my computer and pairing-code flow to the sender"
  ```

---

### Task 9: QR scanner (CameraX + ZXing)

**Files:**
- Create: `android/app/src/main/java/com/voxlink/sender/ScannerActivity.kt`
- Modify: `android/app/build.gradle`, `android/app/src/main/AndroidManifest.xml`, `android/.../MainActivity.kt` (Scan button + result handling)

**Interfaces:**
- Consumes: `PairingProtocol.parsePairingUri` (Task 7).
- Produces: activity result extras `host: String`, `port: Int`, `code: String` (Intent data), consumed by MainActivity.

- [ ] **Step 1: Add dependencies and manifest entries**
  `build.gradle` `dependencies`: `implementation 'androidx.camera:camera-camera2:1.3.4'`, `implementation 'androidx.camera:camera-lifecycle:1.3.4'`, `implementation 'androidx.camera:camera-view:1.3.4'`, `implementation 'com.google.zxing:core:3.5.3'`. Manifest: `<uses-permission android:name="android.permission.CAMERA" />`, `<uses-feature android:name="android.hardware.camera" android:required="false" />`, and `<activity android:name=".ScannerActivity" android:exported="false" />` inside `<application>`.

- [ ] **Step 2: Write `ScannerActivity`**
  Plain `android.app.Activity`, programmatic views: full-screen `FrameLayout` (dark background) holding a CameraX `PreviewView`, a slim custom overlay `View` (dim the outside, transparent scan window + corner hints), a top-left `✕` close `TextView`, and a hint `TextView` (`Point the camera at the QR code on the Voxlink PC`). `onCreate`: request `CAMERA` if not granted (request code 1002, rational: `Camera access is needed to scan the Voxlink QR code.`); denial → toast `Camera denied — use Find my computer or enter the address.` + `finish()`. On grant: `ProcessCameraProvider.getInstance(this)` → bind `Preview` + `ImageAnalysis` (`setBackpressureStrategy(KEEP_ONLY_LATEST)`, single-thread executor). Analyzer: Y-plane luminance (honor `rowStride`/`pixelStride`) → `MultiFormatReader` with a QR-only hint map; on decode: `PairingProtocol.parsePairingUri(raw)` → `setResult(RESULT_OK, Intent().putExtra("host", …).putExtra("port", …).putExtra("code", …))` + `finish()`; `null` → toast `Not a Voxlink QR code` (suppress repeats for the same raw value), keep scanning. Close on `setResult(RESULT_CANCELED)`/back. Unbind/release in `onDestroy`.

- [ ] **Step 3: Wire the Scan button in `MainActivity`**
  Add a **Scan QR** `action(...)` button beside **Find my computer** (same row, weight 1 each) → `startActivityForResult(Intent(this, ScannerActivity::class.java), REQUEST_SCAN)`. In `onActivityResult`: on OK fill `addressInput`/`portInput`/`codeInput` from extras and `setStatus("Scanned PC ${host}", limeColor)`. (Use the platform `startActivityForResult` — the app is a plain `Activity`, consistent with the existing dependency-free `onRequestPermissionsResult` pattern.)

- [ ] **Step 4: Build and unit tests**
  Run: `gradle -p android :app:testDebugUnitTest && gradle -p android :app:assembleDebug`
  Expected: PASS + APK built. Live camera scan is best-effort only (emulator takes a static camera image); the parse path is already pinned by Task 7 tests — do not block on it.

- [ ] **Step 5: Commit**
  ```bash
  git add android/app/build.gradle android/app/src/main/AndroidManifest.xml android/app/src/main/java/com/voxlink/sender/ScannerActivity.kt android/app/src/main/java/com/voxlink/sender/MainActivity.kt
  git commit -m "Add QR scanner to the sender"
  ```

---

### Task 10: Test responder script + emulator E2E

**Files:**
- Create: `scripts/test/fake-pc.py`

**Interfaces:**
- Consumes: the wire formats (Task 1/7); the Android APK (Task 9).
- Produces: none (test tooling; not shipped).

- [ ] **Step 1: Write `scripts/test/fake-pc.py`** (Python 3 stdlib only, ~90 lines)
  Subcommands:
  - `responder [--code 482913] [--audio-port 4950] [--bind 0.0.0.0] [--port 4951] [--sink]`: binds the discovery port; answers `VXLD` → `VXLR`(code, audio-port), `VXLC` → `VXLA` iff the code matches (else silence); with `--sink` also binds the audio port and counts/prints `VXL1` packets (seq, bytes) every 2 s. Prints each exchange to stdout.
  - `probe [--target 127.0.0.1] [--port 4951]`: sends `VXLD`, waits 1.5 s, prints `FOUND <ip> code=<code> port=<port>` or `NO REPLY`.

- [ ] **Step 2: Self-test the script on the host**
  Run: `python3 scripts/test/fake-pc.py responder --code 482913 --port 4951 & sleep 1; python3 scripts/test/fake-pc.py probe; kill %1`
  Expected: `FOUND 127.0.0.1 code=482913 port=4950`. Then start the responder with `--code 111111` and run a one-shot Python check that a `VXLC`+`482913` claim gets **no** `VXLA` while `VXLC`+`111111` gets one (this is the wrong-code path from Review Focus #2, exercised end-to-end).

- [ ] **Step 3: Emulator E2E — install and Find guidance**
  `ADB=/home/abhishek/Android/Sdk/platform-tools/adb` (not on PATH).
  ```bash
  $ADB install -r android/app/build/outputs/apk/debug/app-debug.apk
  $ADB shell pm grant com.voxlink.sender android.permission.RECORD_AUDIO
  $ADB shell am start -n com.voxlink.sender/com.voxlink.sender.MainActivity
  ```
  Tap **Find my computer** (emulator NAT blocks broadcast to the host — 0 replies is the *expected* outcome here).
  Expected: status reads `No PC found. Is Voxlink listening? Same Wi-Fi? Or use Scan QR / enter the address.` (verify with `adb shell uiautomator dump` + `grep`).

- [ ] **Step 4: Emulator E2E — localhost rejection and pairing-code field**
  Type `localhost` into COMPUTER IP ADDRESS (`adb shell input text localhost` after tapping the field) and tap **Start streaming**.
  Expected: status shows exactly `That address is this phone (or localhost). Use your computer's Wi-Fi IP — the PC shows it, or tap Find my computer.` and no microphone permission prompt fires. Confirm the PAIRING CODE field exists, accepts 6 digits, and survives an app restart (prefs).

- [ ] **Step 5: Emulator E2E — claim over NAT against the fake PC**
  Start `python3 scripts/test/fake-pc.py responder --code 482913 --sink` on the host; in the emulator set address `10.0.2.2`, port `4950`, code `482913`; tap **Start streaming**; grant mic if prompted.
  Expected: status passes through `Paired with 10.0.2.2` and the fake PC prints received `VXL1` packets; then stop, change the code to `000000`, restart the responder with `--code 482913`, start again → status shows `No pairing ack — is Voxlink listening on the PC?` while packets still flow (no-ack → stream anyway, per spec).

- [ ] **Step 6: Host-side discovery (real broadcast)**
  Run the desktop app (Task 5 smoke), then `python3 scripts/test/fake-pc.py probe` from the host — the probe must print the real pairing code the app displays (broadcast path on the host LAN). If the machine blocks limited broadcast, retry the probe against the app's LAN IP explicitly and note the result.

- [ ] **Step 7: Commit**
  ```bash
  git add scripts/test/fake-pc.py
  git commit -m "Add scripted pairing responder for E2E checks"
  ```

---

### Task 11: Docs and in-app copy

**Files:**
- Modify: `README.md`, `docs/installation-guide.md`, `docs/omarchy-desktop.md`, `android/.../MainActivity.kt` (Setup dialog only)

**Interfaces:**
- Consumes: the finished flows from Tasks 6–9 (read them for the final wording; the spec §4 defines the intent).
- Produces: none (documentation).

- [ ] **Step 1: Rewrite the README connection flow**
  - `## Install and run the Android sender`: replace "enter the Linux computer's Wi-Fi IP address…" with: desktop **Start listening** → 6-digit code + QR shown → phone **Find my computer** or **Scan QR** (or type the PC address + code by hand) → **Start streaming**.
  - Add a short `## Pairing (discovery) packets` subsection after `## Packet format` with the four-row table (`VXLD` probe 4B, `VXLR` reply 12B, `VXLC` claim 10B, `VXLA` ack 4B, port 4951) and two sentences: the code gates audio in auto mode; leave **Phone IP address** empty on the desktop to auto-pair, or set it to lock to one phone (manual mode). State clearly the discovery traffic is unauthenticated on the LAN, matching the existing "no encryption" warning.
  - Keep the standalone receiver sections (`Install and run on Linux`, `Manual audio check`) untouched.

- [ ] **Step 2: Update `docs/installation-guide.md`**
  Line 29 area (Connection tools bullet): drop the "Use the Wi-Fi address in the desktop app's Phone IP field" instruction — the addresses list is informational; pairing no longer needs it. Line 144 area (step 7): replace with the new flow (Start listening → code/QR → Find/Scan on phone → Start streaming) and keep an explicit manual-mode sentence (set Phone IP on the PC to lock to one phone; type addresses by hand otherwise).

- [ ] **Step 3: Update `docs/omarchy-desktop.md`**
  - Line 16 (binding/filtering paragraph): describe both modes — auto (empty Phone IP, locks to the phone that claims the code) and manual pin (unchanged strict filter).
  - Lines 77–78 (connection steps): the new pairing flow; manual as a numbered alternative.
  - Line 89 (troubleshooting): "no packets" → check the pairing code matched, same Wi-Fi, app foreground, UDP 4950 + 4951 allowed; mention `packets_filtered` climbing while the desktop shows `waiting for phone` means the phone never claimed with the right code.
  - Line 106 (security note): add that the 6-digit code is shared in the clear over LAN and gates (but does not encrypt) who may stream.

- [ ] **Step 4: Rewrite the in-app Setup dialog in `MainActivity`**
  Replace the `Setup & troubleshooting` message with the new flow: same Wi-Fi → desktop Start listening → note the 6-digit code (or scan its QR) → tap **Find my computer** here (or Scan QR / type the address) → Start streaming. Keep the troubleshooting lines for `Cannot assign requested address` and add the `No PC found` / `No pairing ack` hints. Keep "Muting sends silence and keeps the stream open."

- [ ] **Step 5: Verify no stale flow instructions remain**
  Run: `grep -rn "Phone IP" README.md docs/ | grep -v -i "manual\|lock"` — expected: no hit that tells the user the PC *requires* the phone IP. Run: `grep -c "Find my computer" README.md docs/installation-guide.md docs/omarchy-desktop.md` — expected: ≥1 each. Re-read both docs end-to-end once for step numbering, and confirm the prominent **Download Android APK** button at the top of the README's Downloads section renders and links to `releases/latest/download/Voxlink-android-debug.apk` (added in advance — do not remove it).

- [ ] **Step 6: Commit**
  ```bash
  git add README.md docs/installation-guide.md docs/omarchy-desktop.md android/app/src/main/java/com/voxlink/sender/MainActivity.kt
  git commit -m "Document the pairing flow and refresh setup copy"
  ```

---

### Task 12: Publish the release (no version bump)

**Files:** none modified — owner decision: no version fields change anywhere, `.github/workflows/release.yml` stays untouched (its `Voxlink_0.2.0_amd64.AppImage` path remains correct).

**Interfaces:**
- Consumes: everything green from Tasks 1–11.
- Produces: a fresh `v*` tag → CI builds APK + receiver + AppImage and publishes the GitHub release.

- [ ] **Step 1: Full verification before tagging**
  Run: `cd desktop && npm run build && cd src-tauri && cargo test`, then `cd /home/abhishek/Voxlink && gradle -p android :app:testDebugUnitTest :app:assembleDebug`.
  Expected: all green (frontend build, all Rust tests, all Android unit tests, APK assembled).

- [ ] **Step 2: Commit all remaining work**
  Run: `git status --short` first — expected: no leftover uncommitted changes besides intentional ones. Then:
  ```bash
  git add -A
  git commit -m "Add pairing code, QR, and auto-discovery"
  ```
  (Skip the commit if the tree is already clean from the last task's commit.)

- [ ] **Step 3: Tag with a fresh unused `v*` name**
  Run: `git tag` — pick the first unused name in sequence (`v0.2.1`, then `v0.2.2`, …; all prior tags `v0.1.0`/`v0.2.0` exist). Then:
  ```bash
  git tag -a v0.2.1 -m "Voxlink release — pairing code, QR, and auto-discovery"
  git push origin main v0.2.1
  ```
  Note: the tag name is only the workflow trigger; no file versions change, so artifacts keep their current names (`Voxlink-android-debug.apk`, `voxlink-receiver-linux-x86_64.tar.gz`, `Voxlink_0.2.0_amd64.AppImage`) and the workflow needs no edit.

- [ ] **Step 4: Confirm CI published the release**
  Run: `gh run list --repo mrduhlol/Voxlink --limit 5` (or open https://github.com/mrduhlol/Voxlink/actions if `gh` is unauthenticated).
  Expected: `Build downloads` workflow triggered by the new tag runs the linux/android/desktop/release jobs; once green, `gh release view v0.2.1` lists exactly `Voxlink-android-debug.apk`, `voxlink-receiver-linux-x86_64.tar.gz`, `Voxlink_0.2.0_amd64.AppImage`. If the desktop job fails, the first suspect is a missing `desktop/package-lock.json` entry for `qrcode` (Task 6) breaking `npm ci`.
