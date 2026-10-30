#!/usr/bin/env python3
"""Local UDP pairing test peer. Run `responder` for Android or `probe` for desktop."""
import argparse
import socket
import struct
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['responder', 'probe'])
    parser.add_argument('--host', default='0.0.0.0')
    parser.add_argument('--port', type=int, default=4951)
    parser.add_argument('--audio-port', type=int, default=4950)
    parser.add_argument('--code', default='482913')
    args = parser.parse_args()
    if len(args.code) != 6 or not args.code.isascii() or not args.code.isdecimal():
        parser.error('--code must be six ASCII digits')
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        sock.settimeout(1)
        if args.mode == 'responder':
            sock.bind((args.host, args.port))
            print(f'Listening on {args.host}:{args.port}; code {args.code}', flush=True)
            while True:
                try:
                    packet, peer = sock.recvfrom(65535)
                except socket.timeout:
                    continue
                if packet == b'VXLD':
                    sock.sendto(b'VXLR' + args.code.encode('ascii') + struct.pack('!H', args.audio_port), peer)
                    print(f'Probe from {peer}', flush=True)
                elif packet == b'VXLC' + args.code.encode('ascii'):
                    sock.sendto(b'VXLA', peer)
                    print(f'Claim accepted from {peer}', flush=True)
        else:
            host = '255.255.255.255' if args.host == '0.0.0.0' else args.host
            sock.sendto(b'VXLD', (host, args.port))
            deadline = time.monotonic() + 2
            found = set()
            while time.monotonic() < deadline:
                try:
                    packet, peer = sock.recvfrom(65535)
                except socket.timeout:
                    continue
                if len(packet) == 12 and packet[:4] == b'VXLR':
                    result = (peer[0], packet[4:10].decode('ascii'), struct.unpack('!H', packet[10:])[0])
                    if result not in found:
                        found.add(result)
                        print(f'PC {result[0]} code {result[1]} audio port {result[2]}')
            if not found:
                raise SystemExit('No PC found')


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        pass
