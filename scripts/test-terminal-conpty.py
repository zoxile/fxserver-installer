"""Windows 10 1809+ native terminal smoke; stdlib only, no installer or services.

API lifecycle: https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session
Run with Python 3: python scripts/test-terminal-conpty.py path/to/fxins.exe
Windows crossterm receives pasted content as key records, so undo is per key;
the Linux PTY scenario separately requires a bracketed paste to undo atomically.
"""

import ctypes as c
from ctypes import wintypes as w
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading

from terminal_smoke import Probe, actions_and_config, assert_unchanged, config_fixture, isolated_environment


class Coord(c.Structure):
    _fields_ = [("X", c.c_short), ("Y", c.c_short)]


class StartupInfo(c.Structure):
    _fields_ = [
        ("cb", w.DWORD), ("lpReserved", w.LPWSTR), ("lpDesktop", w.LPWSTR), ("lpTitle", w.LPWSTR),
        *[(name, w.DWORD) for name in ("dwX", "dwY", "dwXSize", "dwYSize", "dwXCountChars", "dwYCountChars", "dwFillAttribute", "dwFlags")],
        ("wShowWindow", w.WORD), ("cbReserved2", w.WORD), ("lpReserved2", c.POINTER(c.c_byte)),
        ("hStdInput", w.HANDLE), ("hStdOutput", w.HANDLE), ("hStdError", w.HANDLE),
    ]


class StartupInfoEx(c.Structure):
    _fields_ = [("StartupInfo", StartupInfo), ("lpAttributeList", c.c_void_p)]


class ProcessInfo(c.Structure):
    _fields_ = [("hProcess", w.HANDLE), ("hThread", w.HANDLE), ("dwProcessId", w.DWORD), ("dwThreadId", w.DWORD)]


class ConPty:
    def __init__(self, command, cwd, environment):
        self.api = c.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "CreatePipe": ([c.POINTER(w.HANDLE), c.POINTER(w.HANDLE), c.c_void_p, w.DWORD], w.BOOL),
            "CreatePseudoConsole": ([Coord, w.HANDLE, w.HANDLE, w.DWORD, c.POINTER(w.HANDLE)], c.c_long),
            "ClosePseudoConsole": ([w.HANDLE], None),
            "InitializeProcThreadAttributeList": ([c.c_void_p, w.DWORD, w.DWORD, c.POINTER(c.c_size_t)], w.BOOL),
            "UpdateProcThreadAttribute": ([c.c_void_p, w.DWORD, c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p, c.c_void_p], w.BOOL),
            "DeleteProcThreadAttributeList": ([c.c_void_p], None),
            "CreateProcessW": ([w.LPCWSTR, w.LPWSTR, c.c_void_p, c.c_void_p, w.BOOL, w.DWORD, c.c_void_p, w.LPCWSTR, c.POINTER(StartupInfoEx), c.POINTER(ProcessInfo)], w.BOOL),
            "ReadFile": ([w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p], w.BOOL),
            "WriteFile": ([w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p], w.BOOL),
            "WaitForSingleObject": ([w.HANDLE, w.DWORD], w.DWORD),
            "GetExitCodeProcess": ([w.HANDLE, c.POINTER(w.DWORD)], w.BOOL),
            "TerminateProcess": ([w.HANDLE, w.UINT], w.BOOL),
            "CloseHandle": ([w.HANDLE], w.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.api, name)
            function.argtypes, function.restype = arguments, result
        self.input_read, self.input_write, self.output_read, self.output_write, self.console = [w.HANDLE() for _ in range(5)]
        self.process = ProcessInfo()
        self.output, self.input = queue.Queue(), queue.Queue()
        self.win32_input = False
        self.reader = self.writer = None
        try:
            self.check(self.api.CreatePipe(c.byref(self.input_read), c.byref(self.input_write), None, 0))
            self.check(self.api.CreatePipe(c.byref(self.output_read), c.byref(self.output_write), None, 0))
            result = self.api.CreatePseudoConsole(Coord(80, 24), self.input_read, self.output_write, 0, c.byref(self.console))
            if result < 0:
                raise OSError(f"CreatePseudoConsole failed: HRESULT 0x{result & 0xffffffff:08x}")
            self.reader = threading.Thread(target=self.read_loop, daemon=True)
            self.reader.start()
            size = c.c_size_t()
            self.api.InitializeProcThreadAttributeList(None, 1, 0, c.byref(size))
            attributes = c.create_string_buffer(size.value)
            self.check(self.api.InitializeProcThreadAttributeList(attributes, 1, 0, c.byref(size)))
            try:
                self.check(self.api.UpdateProcThreadAttribute(attributes, 0, 0x00020016, self.console, c.sizeof(w.HANDLE), None, None))
                startup = StartupInfoEx()
                startup.StartupInfo.cb = c.sizeof(startup)
                # Explicit null std handles prevent inheriting this test runner's
                # redirected pipes instead of connecting to the pseudoconsole.
                startup.StartupInfo.dwFlags = 0x100 | 1  # USESTDHANDLES, USESHOWWINDOW, SW_HIDE
                startup.lpAttributeList = c.cast(attributes, c.c_void_p)
                line = c.create_unicode_buffer(subprocess.list2cmdline(command))
                block = c.create_unicode_buffer("\0".join(f"{key}={value}" for key, value in sorted(environment.items(), key=lambda item: item[0].upper())) + "\0\0")
                self.check(self.api.CreateProcessW(command[0], line, None, None, False, 0x00080000 | 0x00000400, block, str(cwd), c.byref(startup), c.byref(self.process)))
            finally:
                self.api.DeleteProcThreadAttributeList(attributes)
            self.close_handle(self.input_read)
            self.close_handle(self.output_write)
            self.writer = threading.Thread(target=self.write_loop, daemon=True)
            self.writer.start()
        except BaseException:
            self.close()
            raise

    @staticmethod
    def check(ok):
        if not ok:
            raise c.WinError(c.get_last_error())

    def close_handle(self, handle):
        if handle.value:
            self.api.CloseHandle(handle)
            handle.value = None

    def read_loop(self):
        buffer, count, total = c.create_string_buffer(65536), w.DWORD(), 0
        mode_tail = b""
        while self.api.ReadFile(self.output_read, buffer, len(buffer), c.byref(count), None) and count.value:
            chunk = buffer.raw[:count.value]
            modes = mode_tail + chunk
            enabled, disabled = modes.rfind(b"\x1b[?9001h"), modes.rfind(b"\x1b[?9001l")
            if max(enabled, disabled) >= 0:
                self.win32_input = enabled > disabled
            mode_tail = modes[-16:]
            total += count.value
            if total <= 2 * 1024 * 1024:
                self.output.put(chunk)
            elif total - count.value <= 2 * 1024 * 1024:
                self.output.put(AssertionError("ConPTY exceeded the 2 MiB output limit"))
            # Continue draining after overflow so ClosePseudoConsole cannot block.
        self.output.put(b"")

    def write_loop(self):
        while (data := self.input.get()) is not None:
            count = w.DWORD()
            if not self.api.WriteFile(self.input_write, data, len(data), c.byref(count), None) or count.value != len(data):
                self.output.put(AssertionError("ConPTY input pipe closed or short write"))
                return

    def send(self, data):
        assert len(data) <= 4096, "Test input must be bounded"
        if self.win32_input and data and all(32 <= byte < 127 for byte in data):
            # Honor ConPTY's native key protocol instead of inferring modifiers
            # through the workstation layout (e.g. braces require AltGr in Swedish).
            data = b"".join(f"\x1b[0;0;{byte};1;0;1_\x1b[0;0;{byte};0;0;1_".encode() for byte in data)
        elif self.win32_input and len(data) == 1 and 0 < data[0] <= 27:
            char = data[0]
            key, modifiers = (char, 0) if char in (9, 13, 27) else (char + 64, 8)
            data = f"\x1b[{key};0;{char};1;{modifiers};1_\x1b[{key};0;{char};0;{modifiers};1_".encode()
        self.input.put(data)

    def read(self, timeout):
        try:
            value = self.output.get(timeout=timeout)
        except queue.Empty:
            return None
        if isinstance(value, Exception):
            raise value
        return value

    def wait(self, seconds=15):
        assert self.api.WaitForSingleObject(self.process.hProcess, seconds * 1000) == 0, "ConPTY child did not exit"
        code = w.DWORD()
        self.check(self.api.GetExitCodeProcess(self.process.hProcess, c.byref(code)))
        assert code.value == 0, f"ConPTY child exited with {code.value}"

    def close(self):
        if self.process.hProcess and self.api.WaitForSingleObject(self.process.hProcess, 0) == 258:
            self.api.TerminateProcess(self.process.hProcess, 1)
            self.api.WaitForSingleObject(self.process.hProcess, 5000)
        self.input.put(None)
        if self.writer:
            self.writer.join(5)
        if self.console.value:
            closer = threading.Thread(target=self.api.ClosePseudoConsole, args=(self.console,), daemon=True)
            closer.start()
            closer.join(10)
            assert not closer.is_alive(), "ConPTY shutdown timed out"
            self.console.value = None
        for handle in (self.input_read, self.output_write, self.input_write):
            self.close_handle(handle)
        if self.reader:
            self.reader.join(5)
        self.close_handle(self.output_read)
        for handle in (self.process.hThread, self.process.hProcess):
            if handle:
                self.api.CloseHandle(handle)
        self.process = ProcessInfo()


def main():
    assert os.name == "nt", "ConPTY smoke requires Windows 10 1809 or newer"
    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="fxins-conpty-") as directory:
        root = Path(directory)
        arguments, files = config_fixture(root)
        terminal = ConPty([str(binary), *arguments], root, isolated_environment(root))
        try:
            probe = Probe(terminal.send, terminal.read)
            probe.contains("FXServer Installer")
            actions_and_config(probe, paste_undo_limit=64)
            assert_unchanged(files)
            terminal.send(b"q")
            terminal.wait()
            assert_unchanged(files)
        finally:
            terminal.close()
    print("Native ConPTY actions, typed forms, cfg edit/undo/discard, and clean exit passed.")


if __name__ == "__main__":
    main()
