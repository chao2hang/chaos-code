#!/usr/bin/env python3
import http.server
import socketserver
import subprocess
import threading
import time
import sys

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
                v = v[:16] + "..." if len(v) > 16 else v
            print(f"{k}: {v}")
        print("\nBODY:")
        print(captured_body[:500])
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
httpd = socketserver.TCPServer(("", PORT), MyHTTPRequestHandler)

server_thread = threading.Thread(target=httpd.serve_forever)
server_thread.daemon = True
server_thread.start()

print(f"Mock server listening on port {PORT}, running Chaos test...")
time.sleep(0.5)

chaos_cmd = [
    "/home/chaos/.chaos/bin/chaos",
    "--client", "workbuddy",
    "--model", "mock_workbuddy/test",
    "-p", "Hello!"
]

try:
    result = subprocess.run(chaos_cmd, capture_output=True, text=True, timeout=60)
    print("\n=== Chaos Output ===")
    print("stdout:", result.stdout)
    print("stderr:", result.stderr)
except Exception as e:
    print("\n=== Chaos Error ===")
    print(f"Exception: {e}")

print("\n=== FINAL CAPTURED HEADERS ===")
for k, v in sorted(captured_headers.items()):
    if k.lower() in ['x-api-key', 'authorization']:
        v = v[:16] + "..."
    print(f"{k}: {v}")

print("\nDone!")
