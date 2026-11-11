#!/usr/bin/env python3
"""넷엑스 터미널을 리눅스에서 눌러 볼 때 쓰는 가짜 상대(127.0.0.1에서만 듣는다).

  Telnet 2323   옵션 협상(ECHO, SGA, NAWS, TTYPE)을 하는 가짜 스위치 콘솔
  FTP    2121   사용자 ftpuser / 암호 ftp-test, 수동 모드, LIST·RETR·STOR·CWD·PWD
  TFTP   6969   UDP, 받기·올리기
  시리얼        socat으로 만든 /dev/ttyNETX1이 있으면 가짜 라우터 콘솔

파일은 인자로 준 폴더(기본 /tmp/netx-fakes)에서 주고받는다.
시리얼 준비: socat pty,raw,echo=0,link=/dev/ttyNETX0 pty,raw,echo=0,link=/dev/ttyNETX1 &
NetX는 NETX_TEST_SERIAL=/dev/ttyNETX0 환경 변수로 띄운다.
"""
import os
import socket
import struct
import sys
import threading

ROOT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/netx-fakes"
HOST = "127.0.0.1"
IAC, DO, WILL, SB, SE = 255, 253, 251, 250, 240


def inside(name):
    path = os.path.realpath(os.path.join(ROOT, name.lstrip("/")))
    if not path.startswith(os.path.realpath(ROOT)):
        raise ValueError(name)
    return path


def telnet_client(c):
    c.sendall(bytes([IAC, WILL, 1, IAC, WILL, 3, IAC, DO, 31, IAC, DO, 24, IAC, DO, 34, IAC, SB, 24, 1, IAC, SE]))
    c.sendall(b"\r\nSW1 console (fake)\r\nSW1> ")
    line = b""
    while True:
        d = c.recv(1024)
        if not d:
            return
        out, i = b"", 0
        while i < len(d):
            b = d[i]
            if b == IAC and i + 1 < len(d):
                if d[i + 1] == SB:
                    j = d.find(bytes([IAC, SE]), i)
                    sub = d[i + 2 : j]
                    if sub[:1] == bytes([31]):
                        out += b"[naws %dx%d]" % (sub[1] * 256 + sub[2], sub[3] * 256 + sub[4])
                    if sub[:1] == bytes([24]):
                        out += b"[ttype %s]" % sub[2:]
                    i = j + 2
                else:
                    i += 3
                continue
            if b == 13:
                if line == b"exit":
                    c.sendall(out + b"\r\nbye\r\n")
                    c.close()
                    return
                out += b"\r\nyou typed: " + line + b"\r\nSW1> "
                line = b""
            elif b == 127:
                if line:
                    line = line[:-1]
                    out += b"\b \b"
            elif b >= 32:
                line += bytes([b])
                out += bytes([b])
            i += 1
        c.sendall(out)


def ftp_client(c):
    f = c.makefile("rb")
    say = lambda s: c.sendall(s.encode() + b"\r\n")
    say("220 NetX fake FTP")
    cwd, ok, pasv = "/", False, None
    while True:
        line = f.readline().decode(errors="replace").rstrip("\r\n")
        if not line:
            return
        verb, _, arg = line.partition(" ")
        verb = verb.upper()
        if verb == "USER":
            user = arg
            say("331 password please")
        elif verb == "PASS":
            ok = user == "ftpuser" and arg == "ftp-test"
            say("230 logged in" if ok else "530 login incorrect")
        elif verb == "QUIT":
            say("221 bye")
            return
        elif not ok:
            say("530 log in first")
        elif verb == "TYPE":
            say("200 ok")
        elif verb == "PWD":
            say('257 "%s"' % cwd)
        elif verb == "CWD":
            say("250 ok")
        elif verb == "PASV":
            pasv = socket.socket()
            pasv.bind((HOST, 0))
            pasv.listen(1)
            p = pasv.getsockname()[1]
            say("227 Entering Passive Mode (127,0,0,1,%d,%d)" % (p // 256, p % 256))
        elif verb in ("LIST", "RETR", "STOR") and pasv:
            try:
                if verb == "LIST":
                    body = "".join("-rw-r--r-- 1 netx netx %9d %s\r\n" % (os.path.getsize(os.path.join(ROOT, n)), n) for n in sorted(os.listdir(ROOT))).encode()
                elif verb == "RETR":
                    body = open(inside(arg), "rb").read()
                else:
                    target = inside(arg)
            except (OSError, ValueError):
                say("550 no such file")
                continue
            say("150 opening data connection")
            d, _ = pasv.accept()
            if verb == "STOR":
                with open(target, "wb") as out:
                    while chunk := d.recv(65536):
                        out.write(chunk)
            else:
                d.sendall(body)
            d.close()
            pasv.close()
            pasv = None
            say("226 transfer complete")
        else:
            say("500 unknown command")


def tcp(port, handler):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((HOST, port))
    s.listen()
    while True:
        c, _ = s.accept()
        threading.Thread(target=handler, args=(c,), daemon=True).start()


def tftp_transfer(op, name, client):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((HOST, 0))
    s.settimeout(3)
    try:
        path = inside(name)
        if op == 1:
            data = open(path, "rb").read()
            blocks = [data[i : i + 512] for i in range(0, len(data), 512)]
            if len(data) % 512 == 0:
                blocks.append(b"")
            for n, chunk in enumerate(blocks, 1):
                for _ in range(5):
                    s.sendto(struct.pack(">HH", 3, n & 0xFFFF) + chunk, client)
                    try:
                        if s.recvfrom(600)[0] == struct.pack(">HH", 4, n & 0xFFFF):
                            break
                    except socket.timeout:
                        pass
        else:
            with open(path, "wb") as out:
                n = 0
                while True:
                    s.sendto(struct.pack(">HH", 4, n), client)
                    pkt = s.recvfrom(600)[0]
                    if pkt[:4] == struct.pack(">HH", 3, (n + 1) & 0xFFFF):
                        n = (n + 1) & 0xFFFF
                        out.write(pkt[4:])
                        if len(pkt) < 516:
                            s.sendto(struct.pack(">HH", 4, n), client)
                            return
    except (OSError, ValueError):
        s.sendto(struct.pack(">HH", 5, 1) + b"File not found\0", client)


def tftp(port):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((HOST, port))
    while True:
        pkt, client = s.recvfrom(600)
        op = struct.unpack(">H", pkt[:2])[0]
        name = pkt[2:].split(b"\0")[0].decode(errors="replace")
        threading.Thread(target=tftp_transfer, args=(op, name, client), daemon=True).start()


def serial(path):
    fd = os.open(path, os.O_RDWR)
    os.write(fd, b"\r\nPress RETURN to get started.\r\n")
    while True:
        os.write(fd, os.read(fd, 100).replace(b"\r", b"\r\nRouter> "))


os.makedirs(ROOT, exist_ok=True)
threading.Thread(target=tcp, args=(2323, telnet_client), daemon=True).start()
threading.Thread(target=tcp, args=(2121, ftp_client), daemon=True).start()
threading.Thread(target=tftp, args=(6969,), daemon=True).start()
if os.path.exists("/dev/ttyNETX1"):
    threading.Thread(target=serial, args=("/dev/ttyNETX1",), daemon=True).start()
print("telnet 2323, ftp 2121 (ftpuser / ftp-test), tftp 6969, files in", ROOT, flush=True)
threading.Event().wait()
