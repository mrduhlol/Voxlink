import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";

type Connection = "disconnected" | "connecting" | "connected" | "error";
type LocalAddress = { name: string; ip: string };
type DesktopStatus = {
  connection: Connection;
  listening: boolean;
  virtualMicrophone: "stopped" | "ready" | "error";
  virtualSource: string | null;
  bindAddress: string | null;
  phoneAddress: string | null;
  port: number;
  sampleRate: number | null;
  packetsReceived: number;
  packetsLost: number;
  packetsInvalid: number;
  packetsFiltered: number;
  audioQueueDrops: number;
  inputLevel: number;
  inputPeakDbfs: number | null;
  lastPacketAgoMs: number | null;
  error: string | null;
  recovery: string | null;
};
type Settings = { bindIp: string; phoneIp: string; port: string; timeoutMs: number };

const EMPTY_STATUS: DesktopStatus = {
  connection: "disconnected",
  listening: false,
  virtualMicrophone: "stopped",
  virtualSource: null,
  bindAddress: null,
  phoneAddress: null,
  port: 4950,
  sampleRate: null,
  packetsReceived: 0,
  packetsLost: 0,
  packetsInvalid: 0,
  packetsFiltered: 0,
  audioQueueDrops: 0,
  inputLevel: 0,
  inputPeakDbfs: null,
  lastPacketAgoMs: null,
  error: null,
  recovery: null,
};

const DEFAULT_SETTINGS: Settings = { bindIp: "", phoneIp: "", port: "4950", timeoutMs: 2500 };

function readSavedSettings(): Settings {
  try {
    const saved = localStorage.getItem("voxlink.desktop.settings");
    return saved ? { ...DEFAULT_SETTINGS, ...JSON.parse(saved) } : DEFAULT_SETTINGS;
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function Icon({ name, size = 18 }: { name: "wave" | "wifi" | "mic" | "activity" | "settings" | "arrow" | "check"; size?: number }) {
  const shared = { width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true as const };
  const paths = {
    wave: <><path d="M2 12h3l2.1-6 4.2 12 3.2-9 2.2 6H22" /><path d="M5 19h14" opacity=".35" /></>,
    wifi: <><path d="M5 9.5a11 11 0 0 1 14 0" /><path d="M8 12.5a6.5 6.5 0 0 1 8 0" /><path d="M11 15.5a2 2 0 0 1 2 0" /><path d="M12 19h.01" /></>,
    mic: <><rect x="9" y="3" width="6" height="12" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3m-4 0h8" /></>,
    activity: <><path d="M3 12h4l2-7 4 14 3-7h5" /></>,
    settings: <><path d="M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7Z" /><path d="m19.4 15 .1.1a1.8 1.8 0 0 1-2.5 2.5l-.1-.1a1.8 1.8 0 0 0-3 .9v.2a1.8 1.8 0 0 1-3.6 0v-.2a1.8 1.8 0 0 0-3-.9l-.1.1a1.8 1.8 0 0 1-2.5-2.5l.1-.1a1.8 1.8 0 0 0-.9-3H3.8a1.8 1.8 0 0 1 0-3.6H4a1.8 1.8 0 0 0 .9-3l-.1-.1a1.8 1.8 0 0 1 2.5-2.5l.1.1a1.8 1.8 0 0 0 3-.9v-.2a1.8 1.8 0 0 1 3.6 0V2a1.8 1.8 0 0 0 3 .9l.1-.1a1.8 1.8 0 0 1 2.5 2.5l-.1.1a1.8 1.8 0 0 0 .9 3h.2a1.8 1.8 0 0 1 0 3.6h-.2a1.8 1.8 0 0 0-.9 3Z" transform="translate(0 2) scale(.9)" /></>,
    arrow: <><path d="M5 12h14M13 6l6 6-6 6" /></>,
    check: <path d="m5 12 4 4L19 6" />,
  };
  return <svg {...shared}>{paths[name]}</svg>;
}

function StatusPill({ state }: { state: Connection }) {
  const labels: Record<Connection, string> = { disconnected: "Disconnected", connecting: "Connecting", connected: "Connected", error: "Error" };
  return <span className={`status-pill ${state}`}><span className="status-dot" />{labels[state]}</span>;
}

function prettyNumber(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}

function App() {
  const [settings, setSettings] = useState<Settings>(readSavedSettings);
  const [addresses, setAddresses] = useState<LocalAddress[]>([]);
  const [status, setStatus] = useState<DesktopStatus>(EMPTY_STATUS);
  const [busy, setBusy] = useState(false);
  const [backendError, setBackendError] = useState<string | null>(null);

  useEffect(() => {
    localStorage.setItem("voxlink.desktop.settings", JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    let mounted = true;
    const refresh = async () => {
      try {
        const [nextStatus, localAddresses] = await Promise.all([
          invoke<DesktopStatus>("get_status"),
          invoke<LocalAddress[]>("get_local_addresses"),
        ]);
        if (!mounted) return;
        setStatus(nextStatus);
        setAddresses(localAddresses);
        setBackendError(null);
        setSettings((current) => current.bindIp || !localAddresses.length ? current : { ...current, bindIp: localAddresses[0].ip });
      } catch (error) {
        if (mounted) setBackendError(String(error));
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 200);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  const active = status.listening;
  const meterWidth = useMemo(() => `${Math.min(100, Math.max(0, status.inputLevel * 100))}%`, [status.inputLevel]);
  const lastPacket = status.lastPacketAgoMs == null ? "No packets yet" : status.lastPacketAgoMs < 1000 ? "Just now" : `${(status.lastPacketAgoMs / 1000).toFixed(1)}s ago`;

  const update = (key: keyof Settings, value: string | number) => setSettings((current) => ({ ...current, [key]: value }));

  async function start() {
    setBusy(true);
    try {
      await invoke("start_stream", { config: {
        bindIp: settings.bindIp.trim(),
        phoneIp: settings.phoneIp.trim(),
        port: Number(settings.port),
        timeoutMs: settings.timeoutMs,
      } });
      setBackendError(null);
    } catch (error) {
      setBackendError(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      await invoke("stop_stream");
      setBackendError(null);
    } catch (error) {
      setBackendError(String(error));
    } finally {
      setBusy(false);
    }
  }

  const connectionMessage = status.error ?? backendError ?? (status.connection === "connected"
    ? `Receiving live audio from ${status.phoneAddress ?? "your phone"}.`
    : active ? "Listening on your Wi-Fi interface. Start the Android sender to connect."
      : "Start Voxlink to create the virtual microphone and listen for your phone.");

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <div className="brand-mark"><Icon name="wave" size={23} /></div>
          <div><div className="brand-name">voxlink</div><div className="brand-caption">PHONE AUDIO · LINUX</div></div>
        </div>
        <div className="topbar-right"><span className="privacy-chip"><span className="privacy-dot" />LOCAL ONLY</span><span className="version-label">DESKTOP 0.2</span></div>
      </header>

      <section className="hero-row">
        <div>
          <div className="eyebrow">AUDIO BRIDGE <span className="eyebrow-line" /></div>
          <h1>Your phone,<br /><span>as a microphone.</span></h1>
          <p className="hero-copy">Stream over your trusted Wi-Fi and route straight into a Linux input device.</p>
        </div>
        <div className={`connection-card ${status.connection}`}>
          <div className="connection-orbit"><Icon name="wifi" size={29} /></div>
          <div className="connection-card-copy"><span className="eyebrow">PHONE LINK</span><StatusPill state={status.connection} /><p>{connectionMessage}</p></div>
          <div className="connection-corner" />
        </div>
      </section>

      <div className="content-grid">
        <section className="panel setup-panel">
          <div className="panel-heading"><div><span className="eyebrow">01 / CONNECTION</span><h2>Stream setup</h2></div><span className={`mini-state ${active ? "active" : ""}`}><span />{active ? "LISTENER READY" : "IDLE"}</span></div>
          <div className="field-grid">
            <label className="field"><span>PC Wi-Fi address</span><input list="local-address-options" value={settings.bindIp} placeholder="192.168.1.5" disabled={active} onChange={(event) => update("bindIp", event.target.value)} /><datalist id="local-address-options">{addresses.map((address) => <option key={`${address.name}-${address.ip}`} value={address.ip}>{address.name}</option>)}</datalist><small>Bind only to this network interface.</small></label>
            <label className="field"><span>Phone IP address</span><input value={settings.phoneIp} placeholder="192.168.1.42" disabled={active} inputMode="decimal" onChange={(event) => update("phoneIp", event.target.value)} /><small>Packets from other addresses are ignored.</small></label>
            <label className="field field-port"><span>UDP port</span><input type="number" min="1024" max="65535" value={settings.port} disabled={active} onChange={(event) => update("port", event.target.value)} /><small>Android default: 4950</small></label>
          </div>
          <div className="control-row"><div className="control-hint"><span className="key-indicator"><Icon name="wifi" size={15} /></span><span>{active ? `Port ${status.port} · ${status.bindAddress ?? settings.bindIp}` : "Start the receiver before the phone sender."}</span></div><div className="controls"><button className="button button-secondary" onClick={() => void stop()} disabled={!active || busy}><span className="stop-square" />Stop</button><button className="button button-primary" onClick={() => void start()} disabled={active || busy}><Icon name="wave" size={17} />{busy ? "Starting…" : "Start listening"}<Icon name="arrow" size={16} /></button></div></div>
        </section>

        <section className="panel mic-panel">
          <div className="panel-heading"><div><span className="eyebrow">02 / LINUX INPUT</span><h2>Virtual microphone</h2></div><div className={`device-icon ${status.virtualMicrophone}`}><Icon name="mic" size={21} /></div></div>
          <div className="device-row"><div className="device-bullet"><span /></div><div className="device-copy"><strong>{status.virtualSource ?? "Voxlink Microphone"}</strong><span>{status.virtualMicrophone === "ready" ? "PulseAudio · PipeWire compatibility" : status.virtualMicrophone === "error" ? "Unavailable · see diagnostics" : "Created while the receiver is running"}</span></div><span className={`device-status ${status.virtualMicrophone}`}>{status.virtualMicrophone === "ready" ? "READY" : status.virtualMicrophone === "error" ? "ERROR" : "OFF"}</span></div>
          <div className="meter-wrap"><div className="meter-head"><span>LIVE INPUT LEVEL</span><span>{status.inputPeakDbfs == null ? "— dBFS" : `${status.inputPeakDbfs.toFixed(1)} dBFS`}</span></div><div className="meter-track"><div className="meter-fill" style={{ width: meterWidth }} /><div className="meter-ticks"><i /><i /><i /><i /><i /><i /><i /><i /><i /></div></div><div className="meter-scale"><span>−60</span><span>−36</span><span>−18</span><span>−6</span><span>0 dBFS</span></div></div>
          <p className="mic-footnote"><Icon name="check" size={15} /> Appears in Discord, OBS and browser input lists as a monitor source.</p>
        </section>

        <section className="panel stream-panel">
          <div className="panel-heading"><div><span className="eyebrow">03 / STREAM</span><h2>Audio diagnostics</h2></div><span className="stream-format">PCM16 <b>·</b> MONO</span></div>
          <div className="diagnostic-grid">
            <div className="diagnostic"><span>Sample rate</span><strong>{status.sampleRate ? `${prettyNumber(status.sampleRate)} <i>Hz</i>` : "—"}</strong></div>
            <div className="diagnostic"><span>Packets received</span><strong>{prettyNumber(status.packetsReceived)}</strong></div>
            <div className="diagnostic"><span>Packets lost</span><strong className={status.packetsLost ? "warning-value" : ""}>{prettyNumber(status.packetsLost)}</strong></div>
            <div className="diagnostic"><span>Last audio packet</span><strong>{lastPacket}</strong></div>
          </div>
          <div className="diagnostic-foot"><span><span className="foot-dot" />{status.packetsInvalid} invalid</span><span>{status.packetsFiltered} filtered</span><span>{status.audioQueueDrops} audio queue drops</span></div>
        </section>

        <section className="panel settings-panel">
          <div className="panel-heading"><div><span className="eyebrow">04 / PREFERENCES</span><h2>Connection settings</h2></div><div className="setting-icon"><Icon name="settings" size={19} /></div></div>
          <div className="setting-row"><div><strong>Disconnect timeout</strong><span>Mark the phone disconnected after this much silence.</span></div><label className="timeout-control"><input type="range" min="1000" max="6000" step="500" value={settings.timeoutMs} disabled={active} onChange={(event) => update("timeoutMs", Number(event.target.value))} /><span>{(settings.timeoutMs / 1000).toFixed(1)}s</span></label></div>
          <div className="setting-row"><div><strong>Audio format</strong><span>Uses the sample rate declared by the phone.</span></div><span className="setting-value">AUTO <span>·</span> 8–192 kHz</span></div>
          <div className="setting-row"><div><strong>Packet handling</strong><span>Out-of-order packets are skipped to protect timing.</span></div><span className="setting-value">LOW LATENCY</span></div>
        </section>
      </div>

      <footer className="footer"><div className="footer-left"><span className="footer-wave"><Icon name="activity" size={16} /></span><span>Voxlink runs locally. Audio stays on your network and PC.</span></div><span className="footer-protocol">UDP / VXL1 <b>·</b> NO ENCRYPTION</span></footer>
    </main>
  );
}

export default App;
