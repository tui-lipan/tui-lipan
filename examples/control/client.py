import socket, sys, time

sock_path, cmds = sys.argv[1], sys.argv[2:]
for _ in range(100):
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(sock_path)
        break
    except (FileNotFoundError, ConnectionRefusedError):
        time.sleep(0.1)
else:
    print("could not connect")
    sys.exit(1)

f = s.makefile("rwb")
for cmd in cmds:
    f.write((cmd + "\n").encode())
    f.flush()
    header = f.readline().decode().strip()
    status, length = header.split(" ", 1)
    payload = f.read(int(length)).decode()
    print(f"--- {cmd!r} -> {status} ({length} bytes)")
    if payload:
        print(payload if len(payload) < 700 else payload[:700] + "\n...[truncated]")
