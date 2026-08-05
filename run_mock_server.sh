#!/usr/bin/env bash

echo "=== Mock server listening on port 8787 ==="
while true; do
  # 使用 nc 监听并捕获请求
  if [ -x "$(command -v nc)" ]; then
    echo -e "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n" | nc -l -p 8787 > /tmp/mock_request.txt
    echo "=== CAPTURED REQUEST ==="
    cat /tmp/mock_request.txt
    echo "=== END CAPTURED ==="
  else
    echo "nc (netcat) not found, using Python..."
    python3 -c "
import http.server
import socketserver
import sys

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
        print('\\nBODY:')
        print(post_data.decode('utf-8', errors='replace'))
        print('=== END CAPTURED REQUEST ===')
        sys.stdout.flush()
        
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(b'{\"id\": \"chatcmpl-123\", \"object\": \"chat.completion\", \"created\": 123456, \"model\": \"gpt-5.6-luna\", \"choices\": [{\"index\": 0, \"message\": {\"role\": \"assistant\", \"content\": \"Hello!\"}, \"finish_reason\": \"stop\"}]}')

with socketserver.TCPServer(('', 8787), MyHTTPRequestHandler) as httpd:
    print('Listening on port 8787...')
    httpd.serve_forever()
"
  fi
done