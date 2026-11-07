"""Explicit public-network TCP and DNS/UDP controls; never sends workspace content."""
import ipaddress
import json
import os
import socket
import struct
import sys


def address(host):
    values = socket.getaddrinfo(host, None, socket.AF_INET, socket.SOCK_STREAM)
    for value in values:
        candidate = value[4][0]
        if ipaddress.ip_address(candidate).is_global:
            return candidate
    raise ValueError("The public probe host has no public IPv4 address")


def probe(tcp, dns, blocked):
    identifier = os.urandom(2)
    question = identifier + struct.pack("!HHHHH", 0x0100, 1, 0, 0, 0)
    question += b"\x07example\x03com\x00\x00\x01\x00\x01"
    for kind, destination in [(socket.SOCK_STREAM, (tcp, 443)), (socket.SOCK_DGRAM, (dns, 53))]:
        with socket.socket(socket.AF_INET, kind) as connection:
            connection.settimeout(3)
            try:
                connection.connect(destination)
                if kind == socket.SOCK_DGRAM:
                    connection.send(question)
                    response = connection.recv(4096)
                    assert len(response) >= 12 and response[:2] == identifier
                    flags, _, answers = struct.unpack("!HHH", response[2:8])
                    assert flags & 0x8000 and flags & 0x000f == 0 and answers > 0
            except OSError:
                if blocked:
                    continue
                raise
            assert not blocked, "Public traffic bypassed the internal network"


def main():
    if sys.argv[1] == "control":
        tcp, dns = address(sys.argv[2]), address(sys.argv[3])
        probe(tcp, dns, False)
        print(json.dumps({"tcp": tcp, "dns": dns}))
    elif sys.argv[1] == "blocked":
        probe(sys.argv[2], sys.argv[3], True)
        print("public TCP and UDP blocked")
    else:
        raise ValueError("Unknown public probe mode")


if __name__ == "__main__":
    main()
