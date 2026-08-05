#!/usr/bin/env python3
"""Simple HTTP listener that captures and prints all request headers + body."""
import http.server
import socketserver
import sys
import json
import time

PORT = 9911

SENSITIVE = {"authorization", "cookie", "x-api-key", "apikey", "x-token", "token", "secret"}

def mask(name, value):
    if name.lower() in SENSITIVE and len(value) > 12:
        return value[:8] + "..." + value[-4:]
    return value

class Handler(http.server.BaseHTTPRequestHandler):
    def _handle(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length) if length else b""

        print("\n" + "=" * 90)
        print(f"  {self.command} {self.path}  ({time.strftime('%H:%M:%S')})")
        print("=" * 90)
        print("HEADERS:")
        for k in sorted(self.headers.keys()):
            v = self.headers[k]
            print(f"  {k}: {mask(k, v)}")
        if body:
            try:
                parsed = json.loads(body)
                print("\nBODY (JSON, pretty):")
                print(json.dumps(parsed, indent=2, ensure_ascii=False)[:4000])
            except Exception:
                print("\nBODY (raw, first 4000 bytes):")
                print(body.decode("utf-8", errors="replace")[:4000])
        print("=" * 90)
        sys.stdout.flush()

        # Respond with a valid chat completion so the client doesn't hard-error
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        resp = {
            "id": "chatcmpl-captured",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": "gpt-5.6-luna",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Captured OK"},
                "finish_reason": "stop"
            }]
        }
        self.wfile.write(json.dumps(resp).encode())

    def do_POST(self):
        self._handle()

    def do_GET(self):
        self._handle()

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.end_headers()

    def log_message(self, fmt, *args):
        pass  # silence default logging

class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True

print(f"[listener] Listening on http://0.0.0.0:{PORT}")
print(f"[listener] Point WorkBuddy at: http://localhost:{PORT}/v1/chat/completions")
print(f"[listener] Waiting for requests...\n")
sys.stdout.flush()

with ReusableTCPServer(("0.0.0.0", PORT), Handler) as httpd:
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\n[listener] Shutting down.")
