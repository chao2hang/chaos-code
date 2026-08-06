#!/usr/bin/env python3
import http.server
import socketserver
import subprocess
import threading
import time
import sys
import os
import json

captured_headers = {}
captured_body = ""
request_received = False

class MyHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
    def do_POST(self):
        global captured_headers, captured_body, request_received
        request_received = True
        content_length = int(self.headers['Content-Length'])
        post_data = self.rfile.read(content_length)
        
        captured_headers = dict(self.headers)
        captured_body = post_data.decode('utf-8', errors='replace')
        
        print("\n" + "="*80)
        print("CAPTURED FULL WORKBUDDY REQUEST HEADERS (SENSITIVE VALUES MASKED):")
        print("="*80)
        for k, v in sorted(self.headers.items()):
            val = v
            if k.lower() in ['x-api-key', 'authorization']:
                if len(v) > 16:
                    val = v[:16] + "..."
            print(f"{k}: {val}")
        print("="*80)
        print("\nBODY PREVIEW:")
        print(captured_body[:800])
        print("="*80 + "\n")
        sys.stdout.flush()
        
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(b'''{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1722660856,
            "model": "gpt-5.6-luna",
            "choices": [{"index":0, "message":{"role":"assistant", "content":"Success! Here are your headers!"}, "finish_reason":"stop"}]
        }''')
    
    def log_message(self, format, *args):
        pass

PORT = 12345

# Find an available port
while True:
    try:
        httpd = socketserver.TCPServer(("", PORT), MyHTTPRequestHandler)
        break
    except Exception:
        PORT += 1

print(f"Starting mock server on port {PORT}...")

def run_server():
    httpd.serve_forever()

server_thread = threading.Thread(target=run_server)
server_thread.daemon = True
server_thread.start()

# Create a temporary config for testing
config_path = os.path.expanduser("~/.chaos/config.toml")
with open(config_path, "r") as f:
    original_config = f.read()

temp_config = original_config + f'''

[model_providers.temp_mock_wb]
base_url = "http://localhost:{PORT}/v1"
auth_scheme = "bearer"
api_backend = "chat_completions"
api_key = "fe_oa_282086b9aae3c7ff2c94a2dd54328a31f33ff82527e2a79c"

[model."temp_mock_wb/test"]
model = "gpt-5.6-luna"
model_provider = "temp_mock_wb"
name = "temp_mock_wb/test"
'''

with open(config_path, "w") as f:
    f.write(temp_config)

time.sleep(1)

print("Running Chaos with --client workbuddy and temp mock model...")
sys.stdout.flush()

# Run Chaos in a separate thread to avoid blocking
chaos_output = []
def run_chaos():
    cmd = [
        "/home/chaos/.chaos/bin/chaos",
        "--client", "workbuddy",
        "--model", "temp_mock_wb/test",
        "-p", "Hello, test!"
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        chaos_output.append(("stdout", result.stdout))
        chaos_output.append(("stderr", result.stderr))
        chaos_output.append(("code", result.returncode))
    except Exception as e:
        chaos_output.append(("exception", str(e)))

chaos_thread = threading.Thread(target=run_chaos)
chaos_thread.start()

# Wait up to 60 seconds for request to be received
for _ in range(120):
    if request_received:
        break
    time.sleep(0.5)

print("Waiting for Chaos to finish...")
chaos_thread.join(30)

print("\n=== Chaos Process Output ===")
for t, v in chaos_output:
    print(f"{t}: {v}")

# Restore original config
with open(config_path, "w") as f:
    f.write(original_config)

print("\n=== Done! ===")
