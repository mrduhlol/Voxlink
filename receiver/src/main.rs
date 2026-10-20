use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const MAGIC: &[u8; 4] = b"VXL1";
const HEADER_LEN: usize = 12;
const MAX_WAV_DATA: u64 = u32::MAX as u64 - 36;

struct Options {
    bind: SocketAddr,
    output: PathBuf,
}

fn usage() -> &'static str {
    "Usage: voxlink-receiver --bind <PC_WIFI_IP> [--port PORT] [--output FILE]\n\
     Example: voxlink-receiver --bind 192.168.1.20 --port 4950 --output recordings/received-audio.wav"
}

fn parse_options() -> Result<Options, String> {
    let mut bind_ip: Option<String> = None;
    let mut port: u16 = 4950;
    let mut output = PathBuf::from("recordings/received-audio.wav");
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind_ip = Some(args.next().ok_or("--bind requires an IP address")?);
            }
            "--port" => {
                let value = args.next().ok_or("--port requires a port number")?;
                port = value.parse().map_err(|_| "invalid UDP port")?;
            }
            "--output" => {
                output = PathBuf::from(args.next().ok_or("--output requires a file path")?);
            }
            "--help" | "-h" => return Err(usage().to_owned()),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    let ip = bind_ip.ok_or_else(|| format!("--bind is required\n{}", usage()))?;
    let bind: SocketAddr = format!("{ip}:{port}")
        .parse()
        .map_err(|_| format!("invalid bind IP address: {ip}"))?;

    Ok(Options { bind, output })
}

fn write_wav_header(file: &mut File, sample_rate: u32, data_len: u32) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36u32 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // PCM fmt chunk length
    file.write_all(&1u16.to_le_bytes())?; // PCM format
    file.write_all(&1u16.to_le_bytes())?; // mono
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * 2).to_le_bytes())?; // byte rate
    file.write_all(&2u16.to_le_bytes())?; // block align
    file.write_all(&16u16.to_le_bytes())?; // bits per sample
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    file.flush()
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = options.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let socket = UdpSocket::bind(options.bind)?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;
    println!("Listening on {}", socket.local_addr()?);
    println!("Output: {}", options.output.display());
    println!("Press Ctrl+C to stop and finalize the WAV file.");

    let stopping = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stopping);
    ctrlc::set_handler(move || stop_flag.store(true, Ordering::Relaxed))?;

    let mut file: Option<File> = None;
    let mut sample_rate: Option<u32> = None;
    let mut data_bytes: u64 = 0;
    let mut expected_sequence: Option<u32> = None;
    let mut lost_packets: u64 = 0;
    let mut received_packets: u64 = 0;
    let mut malformed_packets: u64 = 0;
    let mut buffer = [0u8; 65_535];

    while !stopping.load(Ordering::Relaxed) {
        let (length, peer) = match socket.recv_from(&mut buffer) {
            Ok(received) => received,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };

        if length <= HEADER_LEN
            || &buffer[..4] != MAGIC
            || (length - HEADER_LEN) % 2 != 0
        {
            malformed_packets += 1;
            continue;
        }

        let sequence = u32::from_be_bytes(buffer[4..8].try_into()?);
        let packet_rate = u32::from_be_bytes(buffer[8..12].try_into()?);
        if !(8_000..=192_000).contains(&packet_rate) {
            malformed_packets += 1;
            continue;
        }

        if let Some(rate) = sample_rate {
            if packet_rate != rate {
                malformed_packets += 1;
                continue;
            }
        } else {
            if let Some(parent) = options.output.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut output = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .read(true)
                .open(&options.output)?;
            write_wav_header(&mut output, packet_rate, 0)?;
            sample_rate = Some(packet_rate);
            file = Some(output);
            println!("Receiving {packet_rate} Hz mono PCM from {peer}");
        }

        if let Some(expected) = expected_sequence {
            let gap = sequence.wrapping_sub(expected);
            if gap > 0 && gap < (1 << 31) {
                lost_packets += u64::from(gap);
            }
        }
        expected_sequence = Some(sequence.wrapping_add(1));

        let payload = &buffer[HEADER_LEN..length];
        let next_len = data_bytes + payload.len() as u64;
        if next_len > MAX_WAV_DATA {
            eprintln!("WAV size limit reached; stopping recording.");
            stopping.store(true, Ordering::Relaxed);
            break;
        }
        file.as_mut().expect("file is initialized with sample rate").write_all(payload)?;
        data_bytes = next_len;
        received_packets += 1;
    }

    if let (Some(mut output), Some(rate)) = (file, sample_rate) {
        write_wav_header(&mut output, rate, data_bytes as u32)?;
        output.seek(SeekFrom::End(0))?;
        output.flush()?;
        println!(
            "Saved {} bytes of PCM to {} ({} packets, {} missing, {} malformed).",
            data_bytes,
            options.output.display(),
            received_packets,
            lost_packets,
            malformed_packets
        );
    } else {
        println!("No valid audio packets received; no WAV file was created.");
    }

    Ok(())
}

fn main() {
    let options = match parse_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = run(options) {
        eprintln!("Receiver error: {error}");
        std::process::exit(1);
    }
}
