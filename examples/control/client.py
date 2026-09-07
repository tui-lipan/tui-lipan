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
for index, cmd in enumerate(cmds):
    request_id = f"cli-{index}"
    f.write(f"tui-lipan/1 {request_id} 5000 {cmd}\n".encode())
    f.flush()
    header = f.readline().decode().strip()
    version, response_id, status, code, length = header.split(" ", 4)
    if version != "tui-lipan/1" or response_id != request_id:
        raise RuntimeError(f"unexpected response header: {header}")
    payload = f.read(int(length))
    print(f"--- {cmd!r} -> {status}/{code} ({length} bytes)")
    if payload:
        try:
            text = payload.decode()
        except UnicodeDecodeError:
            text = f"<binary {len(payload)} bytes>"
        print(text if len(text) < 700 else text[:700] + "\n...[truncated]")
