# Voxlink pairing & auto-discovery — design

Date: 2026-10-30
Status: complete — reviewed by builder-check, owner questions resolved; awaiting owner approval to proceed to planning
Scope decided with the owner: auto-discovery **plus** pairing code + QR; the standalone Rust WAV receiver (`receiver/`) is **not** changed — only the desktop (Tauri) app and the Android sender app.

## Problem

Pairing a phone to the PC today requires the user to enter **two** addresses and the flow fails for normal users:

1. The phone asks for the **PC's IP** and sends UDP audio there. This part is reasonable.
2. The **PC desktop app** demands a *Phone IP address* as a mandatory field (`desktop/src-tauri/src/lib.rs`, `StartConfig.phone_ip`), and `validate_config` refuses to start when it is empty ("Enter a valid phone IPv4 address."). Worse, the default frontend settings ship `phoneIp: ""` (`App.tsx`, `DEFAULT_SETTINGS`), so a fresh install literally cannot start the listener.
3. The receiver then drops every packet whose source IP ≠ that exact value (`receive_audio`: `packets_filtered += 1`).

Consequences the owner hit in practice:

- The PC asks for the phone's IP; a phone's IP is buried in Android settings, changes with DHCP, and is **something the receiver could read from any incoming packet** — it never needed to be typed.
- The phone-side instruction said "PC Wi-Fi address" but a user reasonably typed `localhost`, which resolves back to the *phone itself*, so audio never leaves the device.
- Net effect: two mysterious config values, silent packet filtering, no actionable error anywhere.

Secondary issue: even when configured "correctly", neither side ever confirms anything — the desktop shows `packets_filtered` but never says *why*, and the phone happily reports "Sending…" because UDP `send` always succeeds.

## Intent

Make pairing a *recognition* task instead of an *address-typing* task:

> Open Voxlink on the PC, click **Start listening**. The PC shows a big 6-digit pairing code and a QR code. On the phone, tap **Find my computer** (or scan the QR) — the address fills itself in, confirmed by the matching code. Tap start. Audio flows.

The pairing code doubles as a gate: only a phone that proves knowledge of the code may have its audio accepted (in auto mode), so a neighbor's phone on the same Wi-Fi cannot stream into your mic.

## Goals

- **G1** No address typing on the phone when discovery works (auto-fill PC IP + port + code).
- **G2** No phone-IP entry on the PC required; the PC learns/locks the sender itself (manual pin stays available as a fallback/advanced option).
- **G3** QR path: one phone camera scan transfers `ip:port:code` — works even when broadcast discovery is blocked.
- **G4** Pairing code shown on the PC disambiguates multiple Voxlink PCs and gates audio acceptance in auto mode.
- **G5** Both sides fail loudly with *actionable* messages (what to check, which field, where to find the value).
- **G6** Backward compatibility: old phone app ↔ new desktop and new phone app ↔ old desktop must still work through the legacy manual flow; the `VXL1` audio packet format and the standalone Rust receiver are untouched.
- **G7** The phone app keeps its dependency-free core for everything except the QR scanner (explicit owner decision: QR was chosen, accepting camera deps).

## Non-goals / anti-goals

- No encryption or authenticated security beyond a LAN-trust pairing code (the app already states "NO ENCRYPTION" and "trusted local Wi-Fi"; the code stops *accidental* senders, not an active sniffer).
- No mDNS/Bonjour, no cloud, no internet permission changes.
- No changes to `receiver/` (standalone WAV receiver CLI) or the `VXL1` audio wire format.
- No changes to `site/` vs `website/` duplication (separate open debt).
- No auto-migration of stored settings beyond "empty phoneIp now works".
- No iOS/desktop receiver for WAV changes.

## Constraints

- **Android:** Kotlin 2.0.21, AGP 8.7.3, compileSdk 35, minSdk 23, Java 17, programmatic views (no XML layouts, no appcompat). Gradle must be ≥ 8.9 (locally pinned 8.14.6). Current design is deliberately dependency-free; this design adds camera deps **only** for scanning (owner-approved).
- **Desktop:** Tauri 2 + Rust, shared parsing via `voxlink_receiver` crate (AudioPacket), PulseAudio/PipeWire via `pactl` (verified working on target host: pipewire-pulse active). Frontend: React + Vite, no test harness.
- **Audio path today:** phone sends UDP datagrams `VXL1|seq|sample_rate|PCM16LE` to PC; desktop binds a unicast socket to the selected PC IP (interface pinning is an intentional feature — do not remove it).
- **Linux socket fact that shapes the design:** a socket bound to a unicast address does **not** receive subnet-broadcast datagrams. Discovery therefore needs its own socket bound to `0.0.0.0` (separate port), not the audio socket.
- **Emulator testing reality:** the Android emulator (gh-widget-api34, NAT `10.0.2.15`, host at `10.0.2.2`) **cannot broadcast to the host**; unicast works. So discovery must be testable on the host (two local processes), and the emulator E2E path uses manual/QR/unicast claim.
- Repo has CI: `.github/workflows/release.yml` (APK/AppImage) — implementation must not break it.

## Design

### 1. Pairing protocol (new, UDP, on the desktop listener)

While listening, the desktop opens a **discovery responder** socket:

- Binds a **fixed discovery port `4951`** on `0.0.0.0`, independent of the audio port. *(Builder call — resolves reviewer Q3.)* The phone therefore always probes 4951, and **Find works no matter what audio port is configured**; `VXLR` returns the real audio port + code, so the phone learns the actual audio port from the reply rather than guessing it. Consequence: audio port must not equal 4951 (reserved) — reject config with "Port 4951 is reserved for discovery — pick another UDP port for audio." Audio port range otherwise 1024..=65535. If the 4951 bind fails (EADDRINUSE → another Voxlink instance), fail `start_stream` with an explicit message: "UDP port 4951 is already in use — another Voxlink may be running."
- Lives exactly as long as the listener (spawn/stop with the runtime; same stop flag discipline as the receive thread).
- **Mode behavior:** the responder runs whenever listening, in *both* modes, and always answers `VXLD` with `VXLR` (so *Find my computer* never false-fails on a healthy listener). `VXLA` is sent only when the claim is honorable: **auto mode** → correct code; **manual-pin mode** → `claimer_ip == pinned_ip`. This prevents the phone from showing *Paired* while the desktop would still filter its audio to a different pin. *(Builder call — resolves reviewer Q1.)* The Pairing card/QR/code stays visible in both modes; only the status copy differs: manual → `Locked to 192.168.1.42`, auto → `waiting for phone` / `Paired 192.168.1.42`.

Packet definitions (all big-endian for new fields; fixed sizes):

| Packet | Direction | Bytes | Layout |
|---|---|---|---|
| `VXLD` probe | phone → broadcast:discPort | 4 | magic only |
| `VXLR` reply | PC → prober (unicast) | 12 | `VXLR` + code `6×ASCII` + audio port `u16 BE` |
| `VXLC` claim | phone → PC (unicast discPort) | 10 | `VXLC` + code `6×ASCII` |
| `VXLA` ack | PC → claimer | 4 | magic only |

- **Code:** 6-digit decimal, range 100000–999999, OS-CSPRNG-generated **once and persisted across app runs** *(owner decision — resolves the stale-code restart problem)*: stored by the desktop backend in a small local file under the OS config/data dir (builder picks the exact path; e.g. `~/.config/voxlink/pairing-code`), generated on first run if absent, reused every launch thereafter. The **Refresh code** button regenerates *and* persists the new value. Because it is long-lived by design, the phone's saved code stays valid across PC restarts — no re-pairing. Tradeoff (accepted): the shared secret rotates only when the user refreshes, so it is not a short-lived token. Using the OS CSPRNG (`getrandom`/`rand`; builder's call) — the code is a shared secret, not a cryptographic boundary.
- Discovery responder answers `VXLD` with `VXLR` **in the clear** — anyone on the LAN can learn the code by probing. Documented and accepted: the gate prevents *unintended* streaming (wrong phone, IoT device, scripts), matching the app's stated no-encryption trust model. The QR and typed-code paths carry the same guarantee level.
- Unknown magics on the discovery port: ignore silently. Discovery packets must never touch the `packets_invalid` / audio statistics.

### 2. Desktop receiver changes (`desktop/src-tauri/src/lib.rs` + `App.tsx`)

**Validation (`validate_config`):**
- `phone_ip` becomes optional: empty string → `None` (auto mode). Non-empty must still parse as a private IPv4 (unchanged rules).
- Audio port range 1024..=65535, but must **not equal 4951** (the fixed discovery port) — reject with "Port 4951 is reserved for discovery — pick another UDP port for audio."
- `bind_ip` rules unchanged.

**Auth state in `receive_audio`:**
- Mode **manual pin** (`phone_ip = Some(ip)`): behavior byte-identical to today — filter strictly to that IP.
- Mode **auto** (`phone_ip = None`): maintain `authorized: Option<Ipv4Addr>`, set when a `VXLC` claim carries the current code (claims from any IP with the correct code authorize that IP; a correct code re-authorizes, i.e. the code is the secret). Audio handling:
  - from `authorized` → process, and set `status.phone_address` (lock display).
  - from anyone else → `packets_filtered += 1`, and surface a one-line diagnostic the first time an unauthorized sender is seen, e.g. `recovery: "Saw audio from 192.168.1.77 but it is not paired. Enter this PC's pairing code on the phone, or set Phone IP manually."` (keep it stable — don't flicker on every packet).
- Authorization persists until `stop_stream` (session-scoped). Sequence/loss accounting unchanged and still single-stream (auto mode locks onto the authorized sender only, so interleaved senders remain impossible).
- Discovery/claim packets are consumed by the responder task, never reaching audio accounting.

**Status surface (`StatusView`):**
- New field `pairing_code: String` (e.g. `"482913"`), available even before listening — loaded from the persisted file (generated on first run if absent) per §1.
- `phone_address` semantics: in auto mode, `Some(ip)` only once authorized+receiving; in manual mode the configured pin (as today).
- Existing `recovery` field reused for pairing guidance: while listening+auto and never paired → `"Waiting for a phone: open Voxlink on the phone and tap Find my computer, or scan the QR code."`

**Frontend (`App.tsx`):**
- New **Pairing** card: 6-digit code in large type, QR image, "Refresh code" button, hint "Start listening to pair". QR payload: `voxlink://<bind_ip>:<port>?code=<6 digits>` where `bind_ip` is the currently selected PC address (re-render when the field changes). QR generation via the `qrcode` npm package (renders to data URL → `<img>`); this is the only new frontend dependency.
- **Phone IP address** field becomes optional: label unchanged, help text → "Leave empty to auto-pair (recommended). Set a specific IP only to lock to one known phone." Empty is now a *valid, default* state.
- Connection panel copy updates: when listening in auto mode, show `Pairing code 482913 · waiting for phone` → `Paired 192.168.1.42`.
- All invoke payloads keep camelCase contract (`phoneIp: ""` allowed).

### 3. Android sender changes (`MainActivity.kt` + new `ScannerActivity.kt`)

**Connection card:**
- Two new primary actions above the address fields:
  - **Find my computer** — sends `VXLD` ×4 at ~250 ms intervals to `255.255.255.255:4951` (the fixed discovery port) **and** the per-interface subnet broadcast (`<iface-ip-anded-with-mask>.255` assuming /24 — computed from the interface's own address; document the /24 assumption as acceptable for home LANs) for every site-local IPv4 interface; collects `VXLR` replies for ~1.2 s.
    - 1 reply → fill address/port/code, status: `Found PC 192.168.1.5 · code 482913`.
    - >1 replies → single-choice dialog listing `ip — code` (user matches the code against the PC screen).
    - 0 replies → status: `No PC found. Is Voxlink listening? Same Wi-Fi? Or use Scan QR / enter the address.`
  - **Scan QR** → starts `ScannerActivity`.
- New optional field: **PAIRING CODE** (6 digits, monospace, hint `shown on the Voxlink PC`), prefilled from discovery/QR, persisted in prefs like `host`.
- Manual address entry remains fully supported.

**QR scanner (`ScannerActivity`):** full-screen CameraX preview + slim overlay, decode loop via `ImageAnalysis` (YUV_420_888 → Y-plane luminance) + ZXing `MultiFormatReader` (QR only), `STRATEGY_KEEP_ONLY_LATEST`. On decode: parse `voxlink://<ip>:<port>?code=<n>` (tolerate scheme `voxlink://` host:port, validate IPv4 + port range) → `setResult` → MainActivity fills fields + status `Scanned PC 192.168.1.5`. Malformed QR → toast `Not a Voxlink QR code`, keep scanning. Close/back button. Camera permission requested at launch (rational string); if denied → finish with toast pointing at manual/Find.

**Dependencies (accepted tradeoff):** `androidx.camera:camera-camera2`, `camera-lifecycle`, `camera-view` (current stable 1.3.x) + `com.google.zxing:core` (3.5.x). Manifest: `CAMERA` permission, `uses-feature android.hardware.camera` `required="false"`, `ScannerActivity` declaration. Everything else stays dependency-free.

**Start-stream behavior (all paths):**
1. Validate input: reject `localhost`, loopback (`127.0.0.0/8`), `0.0.0.0`, link-local (`169.254.0.0/16`) with a targeted message: `That address is this phone (or localhost). Use your computer's Wi-Fi IP — the PC shows it, or tap Find my computer.` This directly fixes the original "local host link" confusion.
2. If a pairing code is known → **claim burst**: send `VXLC` to port **4951** up to ×5 at ~140 ms, wait ≤700 ms for `VXLA`.
   - Ack → status `Paired with 192.168.1.5`, proceed to stream.
   - No ack → proceed anyway (legacy/edge compatibility: a PC in manual-pin mode doesn't run claims as a requirement) but status notes `No pairing ack — is Voxlink listening on the PC?`.
   - If no code at all → stream as today (legacy flow), status `Streaming to 192.168.1.5 — pair with a code for auto mode.`
3. Audio streaming loop, packet format, mute/gain/gate, stats: **unchanged**.

### 4. Docs & copy

- `README.md`, `docs/installation-guide.md`, `docs/omarchy-desktop.md`: replace "enter the phone IP on the PC" steps with the new flow (Start listening → code/QR → Find/Scan on phone → Start streaming); keep a short "Manual mode" section (set Phone IP on PC / type addresses by hand).
- Desktop in-app help strings (setup dialog in `MainActivity`, `recovery` texts) updated to match.
- The marketing pages (`site/`, `website/`) are out of scope.

## Compatibility matrix

| Phone \ Desktop | old desktop (mandatory phoneIp) | new desktop |
|---|---|---|
| **old APK** | status quo (both typed) | works: manual-pin mode identical; auto features unused |
| **new APK** | works: legacy manual flow (type PC IP on phone; set Phone IP on PC) | full flow: discovery/QR/code + auto gate |

`VXL1` untouched → standalone Rust receiver unaffected in all columns.

## Decision: stale pairing code after PC restart → **persist the code** (owner-approved)

The pairing code is generated once and **persisted on the PC across app runs** (see §1), so it no longer changes on restart. The phone's saved code therefore stays valid across PC restarts — the stale-code restart cycle described during the builder check **does not occur**, and no silent re-discovery step is needed on the phone. The only way the code changes is an explicit **Refresh code** press, which is a deliberate act (and the phone then re-pairs via Find/QR as usual).

Consequence for tests: the phone's saved code does **not** go stale merely because the desktop restarted; a *deliberate* Refresh does invalidate it (assert the phone surfaces clear re-pair guidance in that case only).

## Verification plan

- **Rust (desktop crate):**
  - Unit: discovery packet encode/decode round-trips + malformed inputs; `validate_config` empty/`localhost`/65535 phone & port cases; auth state machine (claim authorizes, wrong code doesn't, re-claim with correct code switches, manual pin bypasses, unauthorized audio increments filter + sets stable diagnostic); pairing-code format.
  - Unit: QR payload string builder (`voxlink://…`).
  - Loopback integration (host): responder on the fixed discovery port `127.0.0.1:4951` answering `VXLD` → `VXLR`; claim → `VXLA`; use non-loopback-validation internal fns directly (tests bypass `validate_config`'s loopback rule by constructing state directly).
  - `cargo test` + `cargo check` for the Tauri crate; if system GTK/webkit deps are missing locally, at minimum `cargo test` the pure-logic modules and rely on CI `release.yml` — builder verifies which is possible on this machine.
- **Android:** add JUnit tests for the pure logic (`voxlink://` URI parse, address validation rejecting localhost/loopback/link-local, discovery reply parse). APK build via `gradle assembleDebug` (works locally as of today).
- **Emulator E2E (adb against `emulator-5554`):**
  - Install, grant mic, UI walk: Find (expected: no broadcast out of emulator NAT → assert the *no-PF found* guidance renders), manual entry with `localhost` → assert rejection message, pairing-code field prefill.
  - Claim over NAT: host runs a scripted responder (short Python script under `scripts/test/fake-pc.py`, ~60 lines: listens `0.0.0.0:4951`, verifies code, replies `VXLA`; plus optional audio sink on 4950) → phone entered as `10.0.2.2` + code → assert `Paired` status. (Script is a test tool; lives in repo, not shipped.)
  - QR: unit-tested parse path; live camera scan best-effort — the emulator's virtual camera takes an image file (`~/.cache/serve-emu/camera/*.png`), so a rendered QR PNG *may* feed it; if flaky, rely on the unit test + a real-device note. Do not block on this.
  - Desktop discovery→phone direction: host-only test (two processes, real broadcast on `lo`/LAN) since emulator can't broadcast to host.
- **Manual sign-off checklist** (owner): on real hardware — PC starts listening, phone finds it, code matches, Discord shows "Voxlink Microphone" and carries audio; neighbor-phone-gate sanity (second device without code stays filtered).

## Risks / open spots left to the builder

- Broadcast reception on exotic routers (AP/client isolation) — mitigated by QR + manual paths; copy already points there.
- `rand`/`getrandom` dependency choice for the desktop crate and MSRV implications — builder's call (small, dev-visible).
- Emulator virtual-camera QR feeding may not work — explicitly non-blocking.
- Keeping the unauthorized-sender diagnostic stable (no per-packet flicker) — implementation detail.
- `qrcode` npm package size in the Tauri bundle — negligible, but builder should confirm the license (MIT) fits the repo.

## What's left to the builder

Everything in "Verification plan" + concrete version pinning, thread/lock structure for the responder (mirroring the existing receiver thread lifecycle and stop flag), UI layout details within the existing visual language, and the doc-copy edits listed in §4.
