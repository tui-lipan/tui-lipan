#!/usr/bin/env python3
"""Tiny full-screen program that enables SGR mouse mode and bracketed paste.

The capture harness feeds mouse reports and a bracketed paste, and this
program writes those events back out. The recorded stream is what a terminal
emulator would read: the mode switches plus the redraws.
"""

import os
import select
import termios
import tty

PASTE_END = b"\x1b[201~"


def emit(data: bytes) -> None:
    os.write(1, data)


def cleanup() -> None:
    emit(b"\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?2004l\x1b[?1049l")


def handle_mouse(seq: bytes) -> None:
    kind = "release" if seq.endswith(b"m") else "press"
    button = seq[2:-1].split(b";", 1)[0]
    if button.isdigit() and seq.endswith(b"M") and int(button) & 32:
        kind = "motion"
    emit(f"\r\nMOUSE:{kind}:{seq.decode('ascii', 'replace')}\r\n".encode())


def main() -> None:
    emit(b"\x1b[?1049h\x1b[2J\x1b[H")
    emit(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h")
    emit(b"\x1b[1;30;46m mouse + bracketed paste \x1b[0m\r\n")
    emit(b"q quits\r\n")
    emit(b"\x1b[4;2H\x1b[44m          \x1b[0m")
    fd = 0
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    # Written after raw mode so the harness does not type into the cooked line.
    emit(b"\x1b[6;1HREADY\r\n")
    mode = "normal"
    seq = bytearray()
    paste = bytearray()
    try:
        while True:
            ready, _, _ = select.select([fd], [], [], 8)
            if not ready:
                continue
            data = os.read(fd, 256)
            if not data:
                break
            for byte in data:
                if mode == "paste":
                    paste.append(byte)
                    if paste.endswith(PASTE_END):
                        body = bytes(paste[: -len(PASTE_END)])
                        emit(b"\r\nPASTE:" + body + b"\r\n")
                        paste.clear()
                        mode = "normal"
                    elif len(paste) > 4096:
                        paste.clear()
                        mode = "normal"
                    continue
                if mode == "esc":
                    seq.append(byte)
                    if bytes(seq).startswith(b"[200~"):
                        mode = "paste"
                        paste.clear()
                        seq.clear()
                        continue
                    if bytes(seq).startswith(b"[<") and byte in (ord("M"), ord("m")):
                        handle_mouse(bytes(seq))
                        seq.clear()
                        mode = "normal"
                        continue
                    if len(seq) > 48 or byte in (ord("q"), 3):
                        seq.clear()
                        mode = "normal"
                    continue
                if byte == 0x1B:
                    mode = "esc"
                    seq.clear()
                    continue
                if byte in (ord("q"), 3):
                    return
    finally:
        termios.tcsetattr(fd, termios.TCSANOW, old)
        cleanup()


if __name__ == "__main__":
    main()
