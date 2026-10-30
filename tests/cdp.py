"""极简 CDP 客户端：只够发 Runtime.evaluate。

没有 websocket 库，所以手搓握手 + 帧（客户端帧必须 mask）。
"""
import base64
import json
import os
import socket
import struct


def _handshake(sock, host, path):
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    sock.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            raise RuntimeError("握手失败")
        buf += chunk


def _send_text(sock, text):
    data = text.encode()
    header = bytearray([0x81])
    n = len(data)
    if n < 126:
        header.append(0x80 | n)
    elif n < 65536:
        header.append(0x80 | 126)
        header += struct.pack(">H", n)
    else:
        header.append(0x80 | 127)
        header += struct.pack(">Q", n)
    mask = os.urandom(4)
    header += mask
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    sock.sendall(bytes(header) + masked)


def _recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise RuntimeError("连接断开")
        buf += chunk
    return buf


def _recv_text(sock):
    while True:
        b1, b2 = _recv_exact(sock, 2)
        opcode = b1 & 0x0F
        n = b2 & 0x7F
        if n == 126:
            n = struct.unpack(">H", _recv_exact(sock, 2))[0]
        elif n == 127:
            n = struct.unpack(">Q", _recv_exact(sock, 8))[0]
        payload = _recv_exact(sock, n) if n else b""
        if opcode == 1:
            return payload.decode(errors="replace")
        if opcode == 8:
            raise RuntimeError("服务端关闭")


def evaluate(ws_url, expression, timeout=8):
    """在页面上跑一段 JS，返回求值结果（JSON 反序列化后）。"""
    rest = ws_url.split("://", 1)[1]
    hostport, path = rest.split("/", 1)
    path = "/" + path
    host = hostport.split(":")[0]
    port = int(hostport.split(":")[1])
    s = socket.create_connection((host, port), timeout=timeout)
    s.settimeout(timeout)
    try:
        _handshake(s, hostport, path)
        _send_text(
            s,
            json.dumps(
                {
                    "id": 1,
                    "method": "Runtime.evaluate",
                    "params": {
                        "expression": expression,
                        "returnByValue": True,
                        "awaitPromise": True,
                    },
                }
            ),
        )
        while True:
            msg = json.loads(_recv_text(s))
            if msg.get("id") == 1:
                return msg
    finally:
        s.close()


def page_ws(port=9222, title="SnipForge"):
    import urllib.request

    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=5) as r:
        pages = json.load(r)
    for p in pages:
        if p.get("title") == title:
            return p["webSocketDebuggerUrl"]
    return None
