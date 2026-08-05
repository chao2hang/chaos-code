#!/usr/bin/env python3
import http.server
import socketserver
import sys
import threading
import time

class MyHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers['Content-Length'])
        post_data = self.rfile.read(content_length)
        print('=== CAPTURED FULL REQUEST ===')
        print('HEADERS:')
        for k, v in self.headers.items():
            if k.lower() in ['x-api-key', 'authorization']:
                v = v[:8] + '...' if len(v) > 8 else v
            print(f'{k}: {v}')
        print('\nBODY:')
        print(post_data.decode('utf-8', errors='replace'))
        print('=== END CAPTURED REQUEST ===')
        sys.stdout.flush()
        
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(b'{"id": "chatcmpl-123", "object": "chat.completion", "created": 123456, "model": "gpt-5.6-luna", "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hello!"}, "finish_reason": "stop"}]}')
    
    def log_message(self, format, *args):
        pass  # 不输出默认日志

PORT = 8787
httpd = socketserver.TCPServer(("", PORT), MyHTTPRequestHandler)
print(f"Mock server listening on port {PORT}...")

# 在后台线程运行服务器
def run_server():
    httpd.serve_forever()

server_thread = threading.Thread(target=run_server, daemon=True)
server_thread.start()

# 保持主进程运行
try:
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    print("\nShutting down...")
    httpd.shutdown()
