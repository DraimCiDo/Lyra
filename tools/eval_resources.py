"""Best-effort sampled metrics for the CLI process; never infer missing values.

Linux uses VmHWM; Windows uses GetProcessMemoryInfo. GPU accounting uses only
nvidia-smi rows with the exact CLI PID; WDDM N/A remains unknown. Container,
driver/OS and other process memory are not attributed to Lyra.
"""
import os
from pathlib import Path
import shutil
import subprocess
import threading


def resident_peak(pid):
    if os.name != "nt":
        try:
            for line in Path(f"/proc/{pid}/status").read_text().splitlines():
                if line.startswith("VmHWM:"):
                    return int(line.split()[1]) * 1024
        except (OSError, ValueError):
            pass
        return None
    try:
        import ctypes
        from ctypes import wintypes
        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                        *[(field, ctypes.c_size_t) for field in ["PeakWorkingSetSize", "WorkingSetSize",
                          "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                          "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage"]]]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        api = ctypes.WinDLL("psapi", use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        api.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        api.GetProcessMemoryInfo.restype = wintypes.BOOL
        handle = kernel.OpenProcess(0x0410, False, pid)
        if not handle:
            return None
        try:
            counters = Counters(); counters.cb = ctypes.sizeof(counters)
            return counters.PeakWorkingSetSize if api.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb) else None
        finally:
            kernel.CloseHandle(handle)
    except (OSError, AttributeError, ValueError):
        return None


def gpu_resident(pid, program):
    if not program:
        return None
    try:
        result = subprocess.run([program, "--query-compute-apps=pid,used_gpu_memory", "--format=csv,noheader,nounits"],
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=2, check=False)
        if result.returncode != 0:
            return None
        values = []
        for line in result.stdout[:65536].decode("utf-8", errors="replace").splitlines():
            parts = [value.strip() for value in line.split(",")]
            if len(parts) == 2 and parts[0] == str(pid):
                # Unknown memory on any matching device makes the total unknown.
                if not parts[1].isdigit():
                    return None
                values.append(int(parts[1]) * 1024 * 1024)
        return sum(values) if values else None
    except (OSError, subprocess.TimeoutExpired):
        return None


class Monitor:
    def __init__(self, process, enabled=False):
        self.process, self.enabled = process, enabled
        self.ram, self.vram = None, None
        self.stop_event = threading.Event()
        self.program = shutil.which("nvidia-smi") if enabled else None
        self.thread = threading.Thread(target=self._sample, daemon=True) if enabled else None
        self.gpu_thread = threading.Thread(target=self._sample_gpu, daemon=True) if self.program else None
        if self.thread:
            self.thread.start()
        if self.gpu_thread:
            self.gpu_thread.start()

    def _sample(self):
        while not self.stop_event.is_set() and self.process.poll() is None:
            ram = resident_peak(self.process.pid)
            if ram is not None:
                self.ram = max(self.ram or 0, ram)
            self.stop_event.wait(.1)

    def _sample_gpu(self):
        while not self.stop_event.is_set() and self.process.poll() is None:
            gpu = gpu_resident(self.process.pid, self.program)
            if gpu is not None:
                self.vram = max(self.vram or 0, gpu)
            self.stop_event.wait(.5)

    def finish(self):
        self.stop_event.set()
        if self.thread:
            self.thread.join(timeout=3)
        if self.gpu_thread:
            self.gpu_thread.join(timeout=3)
        return {"peak_ram_bytes": self.ram, "peak_vram_bytes": self.vram,
                "resource_measurement": {"enabled": self.enabled, "scope": "sampled_cli_process_only",
                                         "ram_interval_ms": 100, "gpu_interval_ms": 500, "gpu_source": "nvidia-smi_matching_pid"}}
