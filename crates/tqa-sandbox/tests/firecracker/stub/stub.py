"""A stand-in for the gateway on the Firecracker test network (test-only).

It listens on every port given as an argument, logs one line per connection (the listening port, the peer address, the
first request line and whether an Authorization header was present, never its value) and answers 503, so a runtime
keeps redialling without ever being served. The KVM tests read these lines to observe dial-in and egress.
"""
import socketserver
import sys
import threading


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(5)
        data = b""
        try:
            while b"\r\n\r\n" not in data and len(data) < 65536:
                chunk = self.request.recv(4096)
                if not chunk:
                    break
                data += chunk
        except OSError:
            pass
        head = data.split(b"\r\n")
        line = head[0].decode("latin-1")[:120] if head and head[0] else "-"
        authorized = any(h.lower().startswith(b"authorization:") for h in head[1:])
        port = self.server.server_address[1]
        print(f"stub port={port} peer={self.client_address[0]} auth={'yes' if authorized else 'no'} line={line}", flush=True)
        try:
            self.request.sendall(b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        except OSError:
            pass


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


for port in [int(argument) for argument in sys.argv[1:]]:
    server = Server(("0.0.0.0", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
print("stub ready", flush=True)
threading.Event().wait()
