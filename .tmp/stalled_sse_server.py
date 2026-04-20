import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHAT_RESPONSE = {
    "id": "chatcmpl-stalled-fallback",
    "object": "chat.completion",
    "created": 1,
    "model": "stalled-test-model",
    "choices": [
        {
            "index": 0,
            "message": {
                "role": "assistant",
                "content": None,
                "tool_calls": [
                    {
                        "index": 0,
                        "id": "call_stalled_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": {
                                "path": "src/lib.rs"
                            }
                        }
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }
    ],
    "usage": {
        "prompt_tokens": 9,
        "completion_tokens": 3,
        "total_tokens": 12
    }
}

MESSAGES_RESPONSE = {
    "id": "msg-stalled-fallback",
    "type": "message",
    "role": "assistant",
    "model": "stalled-test-model",
    "content": [
        {
            "type": "tool_use",
            "id": "toolu_stalled_1",
            "name": "read_file",
            "input": {
                "path": "src/lib.rs"
            }
        }
    ],
    "stop_reason": "tool_use",
    "stop_sequence": None,
    "usage": {
        "input_tokens": 9,
        "output_tokens": 3
    }
}

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        message = "%s - - [%s] %s\n" % (
            self.client_address[0],
            self.log_date_time_string(),
            fmt % args,
        )
        with open(".tmp/stalled-sse-server.log", "a", encoding="utf-8") as handle:
            handle.write(message)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length).decode("utf-8") if length else ""
        with open(".tmp/stalled-sse-server.log", "a", encoding="utf-8") as handle:
            handle.write(json.dumps({
                "path": self.path,
                "headers": dict(self.headers),
                "body": json.loads(body) if body else None,
            }) + "\n")

        parsed = json.loads(body) if body else {}
        wants_stream = parsed.get("stream") is True
        if wants_stream:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "keep-alive")
            self.end_headers()
            self.wfile.flush()
            time.sleep(6.0)
            return

        response = CHAT_RESPONSE if self.path.endswith("/v1/chat/completions") else MESSAGES_RESPONSE
        encoded = json.dumps(response).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)
        self.wfile.flush()

if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", 8011), Handler)
    with open(".tmp/stalled-sse-server.log", "a", encoding="utf-8") as handle:
        handle.write("starting stalled SSE server on 127.0.0.1:8011\n")
    server.serve_forever()
