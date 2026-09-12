"""Мок Gemini SSE для проверки клиента без ключа.

Запуск: python scripts/mock_gemini.py [port]
Приложение: RESTYLE_GEMINI_BASE_URL=http://127.0.0.1:8765 (+ любой ключ в keyring).

Режимы (по заголовку x-goog-api-key):
  bad-key   → 400 API_KEY_INVALID
  rate      → 429
  slow      → чанки раз в 1 с, 60 штук (для проверки отмены: сервер логирует
              обрыв соединения клиентом)
  иначе     → 8 чанков по 150 мс, текст = принятый текст в верхнем регистре
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765


def sse(obj):
    return f"data: {json.dumps(obj, ensure_ascii=False)}\r\n\r\n".encode("utf-8")


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log(self, msg):
        print(f"[mock] {time.strftime('%H:%M:%S')} {msg}", flush=True)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        key = self.headers.get("x-goog-api-key", "")
        parts = body.get("contents", [{}])[0].get("parts", [])
        has_img = any("inlineData" in p for p in parts)
        text = next((p["text"] for p in parts if "text" in p), "")
        sys_prompt = body.get("systemInstruction", {}).get("parts", [{}])[0].get("text", "")
        self.log(f"POST {self.path} key={key[:8]}… img={has_img} sys={len(sys_prompt)}ch text={len(text)}ch")

        if key == "bad-key":
            b = json.dumps({"error": {"code": 400, "status": "INVALID_ARGUMENT", "message": "API key not valid",
                                      "details": [{"reason": "API_KEY_INVALID"}]}}).encode()
            self.send_response(400); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b); return
        if key == "rate":
            b = b'{"error":{"code":429,"status":"RESOURCE_EXHAUSTED"}}'
            self.send_response(429); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b); return

        src = text.split("Текст для переписывания:\n", 1)[-1]
        words = (src.upper() or "ПУСТО").split(" ")
        slow = key == "slow"
        chunks = [f"chunk{i} " for i in range(60)] if slow else [w + " " for w in words]
        delay = 1.0 if slow else 0.15

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        sent = 0
        try:
            for i, c in enumerate(chunks):
                ev = {"candidates": [{"content": {"parts": [{"text": c}], "role": "model"}, "index": 0}]}
                if i == len(chunks) - 1:
                    ev["candidates"][0]["finishReason"] = "STOP"
                data = sse(ev)
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
                self.wfile.flush()
                sent += 1
                time.sleep(delay)
            self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()
            self.log(f"done, {sent} chunks")
        except (ConnectionResetError, ConnectionAbortedError, BrokenPipeError) as e:
            self.log(f"CLIENT ABORTED after {sent} chunks ({type(e).__name__})")

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    print(f"[mock] listening on 127.0.0.1:{PORT}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
