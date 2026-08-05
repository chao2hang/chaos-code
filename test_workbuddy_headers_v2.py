#!/usr/bin/env python3
import http.server
import socketserver
import subprocess
import threading
import time
import sys
import os

captured_headers = {}
captured_body = ""

class MyHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
    def do_POST(self):
        global captured_headers, captured_body
        content_length = int(self.headers['Content-Length'])
        post_data = self.rfile.read(content_length)
        
        captured_headers = dict(self.headers)
        captured_body = post_data.decode('utf-8', errors='replace')
        
        print("=== CAPTURED FULL REQUEST ===")
        print("HEADERS:")
        for k, v in sorted(self.headers.items()):
            if k.lower() in ['x-api-key', 'authorization']:
                v = v[:16] + "..."
            print(f"{k}: {v}")
        print("\nBODY:")
        print(captured_body)
        print("=== END CAPTURED ===")
        sys.stdout.flush()
        
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(b'''{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1722660856,
            "model": "gpt-5.6-luna",
            "choices": [{"index":0, "message":{"role":"assistant", "content":"Hello!"}, "finish_reason":"stop"}]
        }''')
    
    def log_message(self, format, *args):
        pass

PORT = 9999
try:
    httpd = socketserver.TCPServer(("", PORT), MyHTTPRequestHandler)
except Exception as e:
    print(f"Port {PORT} in use, trying 9998...")
    PORT = 9998
    httpd = socketserver.TCPServer(("", PORT), MyHTTPRequestHandler)

server_thread = threading.Thread(target=httpd.serve_forever)
server_thread.daemon = True
server_thread.start()

print(f"Mock server listening on port {PORT}, updating config...")
sys.stdout.flush()

# Update config base_url
config_path = os.path.expanduser("~/.chaos/config.toml")
with open(config_path, "r") as f:
    config = f.read()

new_config = config.replace(
    'base_url = "http://localhost:9999/v1"',
    f'base_url = "http://localhost:{PORT}/v1"'
)
with open(config_path, "w") as f:
    f.write(new_config)

time.sleep(0.5)

print("\n=== Now running Chaos test... ===")
sys.stdout.flush()

chaos_cmd = [
    "/home/chaos/.chaos/bin/chaos",
    "--client", "workbuddy",
    "--model", "mock_workbuddy/test",
    "-p", "Hello!"
]

try:
    result = subprocess.run(chaos_cmd, capture_output=True, text=True, timeout=120)
    print("\n=== Chaos Output ===")
    print("stdout:", repr(result.stdout))
    print("stderr:", repr(result.stderr))
except Exception as e:
    print("\n=== Chaos Error ===")
    import traceback
    print(f"Exception: {e}")
    traceback.print_exc()

print("\n=== FINAL CAPTURED HEADERS ===")
for k, v in sorted(captured_headers.items()):
    if k.lower() in ['x-api-key', 'authorization']:
        v = v[:16] + "..."
    print(f"{k}: {v}")

print("\nDone!")

# Restore config
with open(config_path, "w") as f:
    f.write(config)
