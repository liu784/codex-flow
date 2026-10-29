"""Exercise a built Stop hook against a private pipe using synthetic data only.

The temporary executable has only its pipe prefix changed, so this never
connects to a running Codex Flow instance or touches real messages.
"""
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import uuid


kernel = ctypes.WinDLL("kernel32", use_last_error=True)
advapi = ctypes.WinDLL("advapi32", use_last_error=True)
advapi.GetUserNameW.argtypes = [wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
advapi.GetUserNameW.restype = wintypes.BOOL
kernel.CreateNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
    wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p]
kernel.CreateNamedPipeW.restype = wintypes.HANDLE
kernel.ConnectNamedPipe.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
kernel.ConnectNamedPipe.restype = wintypes.BOOL
kernel.ReadFile.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
    ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
kernel.ReadFile.restype = wintypes.BOOL
kernel.CloseHandle.argtypes = [wintypes.HANDLE]


def user_suffix():
    name = ctypes.create_unicode_buffer(257)
    size = wintypes.DWORD(len(name))
    assert advapi.GetUserNameW(name, ctypes.byref(size))
    value = 0xcbf29ce484222325
    for byte in name.value.encode("utf-16le"):
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f"{value:016x}"


def exchange(executable, name, reply, expected, powershell=False):
    handle = kernel.CreateNamedPipeW(name, 1, 8, 1, 65536, 65536, 0, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    received = bytearray()
    errors = []

    def receive():
        try:
            if not kernel.ConnectNamedPipe(handle, None) and ctypes.get_last_error() != 535:
                raise ctypes.WinError(ctypes.get_last_error())
            while True:
                buffer = ctypes.create_string_buffer(65536)
                count = wintypes.DWORD()
                if not kernel.ReadFile(handle, buffer, len(buffer), ctypes.byref(count), None):
                    if ctypes.get_last_error() == 109:
                        break
                    raise ctypes.WinError(ctypes.get_last_error())
                if not count.value:
                    break
                received.extend(buffer.raw[:count.value])
        except Exception as error:
            errors.append(error)
        finally:
            kernel.CloseHandle(handle)

    receiver = threading.Thread(target=receive, daemon=True)
    receiver.start()
    payload = json.dumps({"hook_event_name": "Stop", "last_assistant_message": reply},
                         ensure_ascii=False).encode("utf-8")
    start = time.perf_counter()
    if powershell:
        # The real setup uses a single-quoted PowerShell executable path.
        quote = lambda value: "'" + str(value).replace("'", "''") + "'"
        command = ["pwsh", "-NoProfile", "-Command",
                   f"& {quote(executable)} --codex-flow-stop-hook {quote(sys.executable)}"]
    else:
        command = [str(executable), "--codex-flow-stop-hook", sys.executable]
    result = subprocess.run(command,
                            input=payload, capture_output=True, timeout=10)
    elapsed = time.perf_counter() - start
    receiver.join(5)
    if receiver.is_alive():
        raise RuntimeError("private test pipe did not receive the synthetic reply")
    if errors:
        raise errors[0]
    assert result.returncode == 0 and result.stdout.strip() == b"{}"
    assert json.loads(received.decode("utf-8")) == expected
    assert elapsed < 3, f"hook exceeded its configured 3-second timeout: {elapsed:.2f}s"
    print(f"synthetic characters={len(reply)}, complete={expected == reply}, seconds={elapsed:.3f}")


def main():
    binary = Path(sys.argv[1]).read_bytes()
    original = b"CodexFlow-Stop-"
    replacement = ("CFProbe-" + uuid.uuid4().hex[:7]).encode("ascii")
    assert len(original) == len(replacement) and binary.count(original) == 1
    name = "\\\\.\\pipe\\" + replacement.decode("ascii") + user_suffix()
    # Place this disposable copy beside the project, never over a user executable.
    with tempfile.TemporaryDirectory(prefix="codex-flow-pipe-probe-", dir=Path.cwd()) as tmp:
        tricky = Path(tmp) / "空格 O'Brien $literal"
        tricky.mkdir()
        copy = tricky / "probe.exe"
        copy.write_bytes(binary.replace(original, replacement))
        for reply in ["测试开头 " + "猫🐱" * 150 + " 测试结尾",
                      "测试开头 " + '猫🐱"\\' * 4000 + " 测试结尾",
                      "A" * 200000 + " 测试结尾"]:
            exchange(copy, name, reply, reply)
        exchange(copy, name, "A" * (1024 * 1024 + 1),
                 "Codex 回复过长，请打开 Codex 查看完整内容")
        exchange(copy, name, "特殊路径与长消息测试 " + "猫🐱" * 150,
                 "特殊路径与长消息测试 " + "猫🐱" * 150, powershell=True)


if __name__ == "__main__":
    main()
