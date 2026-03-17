import React, { useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { save } from "@tauri-apps/api/dialog";
import {
  Search,
  ShieldOff,
  LayoutDashboard,
  RefreshCw,
  Play,
  Square,
  Cpu,
  Database,
  Activity,
} from "lucide-react";

import { DiskSelector } from "./components/DiskSelector";
import { HexTerminal, buildLogEntry } from "./components/HexTerminal";
import { RecoveryTable } from "./components/RecoveryTable";
import { ShredPanel } from "./components/ShredPanel";
import { useTauriEvents } from "./hooks/useTauriEvents";
import {
  AppView,
  DiskInfo,
  RecoveredFile,
  ScanProgress,
  ScanState,
  ShredProgress,
  ShredState,
} from "./types";

export default function App() {
  const [view, setView] = useState<AppView>("dashboard");

  // Disk state
  const [disks, setDisks] = useState<DiskInfo[]>([]);
  const [disksLoading, setDisksLoading] = useState(false);
  const [selectedDisk, setSelectedDisk] = useState<string | null>(null);

  // Scan state
  const [scanState, setScanState] = useState<ScanState>("idle");
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [recoveredFiles, setRecoveredFiles] = useState<RecoveredFile[]>([]);
  const [terminalLogs, setTerminalLogs] = useState<ReturnType<typeof buildLogEntry>[]>([]);
  const prevFilesRef = useRef(0);

  // Shred state
  const [shredState, setShredState] = useState<ShredState>("idle");
  const [shredProgress, setShredProgress] = useState<ShredProgress | null>(null);
  const [shredError, setShredError] = useState<string | null>(null);

  // ── Tauri event listeners ────────────────────────────────────────────────
  useTauriEvents({
    onScanProgress: useCallback((p: ScanProgress) => {
      setScanProgress(p);
      const entry = buildLogEntry(p, prevFilesRef.current);
      if (entry) {
        setTerminalLogs((l) => [...l.slice(-200), entry]);
        prevFilesRef.current = p.files_found;
      }
    }, []),

    onScanComplete: useCallback((files: RecoveredFile[]) => {
      setRecoveredFiles(files);
      setScanState("complete");
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: `✓ Scan complete. ${files.length} file(s) recovered.`,
          type: "found" as const,
        },
      ]);
    }, []),

    onScanError: useCallback((err: string) => {
      setScanState("error");
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: `✗ Error: ${err}`,
          type: "error" as const,
        },
      ]);
    }, []),

    onShredProgress: useCallback((p: ShredProgress) => {
      setShredProgress(p);
    }, []),

    onShredComplete: useCallback(() => {
      setShredState("complete");
      setShredError(null);
    }, []),

    onShredError: useCallback((err: string) => {
      setShredState("error");
      setShredError(err);
    }, []),
  });

  // ── Commands ─────────────────────────────────────────────────────────────
  async function loadDisks() {
    setDisksLoading(true);
    try {
      const list = await invoke<DiskInfo[]>("list_disks");
      setDisks(list);
    } catch (e) {
      console.error(e);
    } finally {
      setDisksLoading(false);
    }
  }

  async function startScan() {
    if (!selectedDisk || scanState === "scanning") return;
    setScanState("scanning");
    setRecoveredFiles([]);
    setTerminalLogs([]);
    prevFilesRef.current = 0;
    await invoke("start_scan", { devicePath: selectedDisk });
  }

  async function cancelScan() {
    await invoke("cancel_scan");
    setScanState("idle");
  }

  async function recoverFile(file: RecoveredFile) {
    const dest = await save({
      defaultPath: `recovered_${file.file_type.toLowerCase()}_${file.id}.${file.file_type.toLowerCase()}`,
      title: "Save Recovered File",
    });
    if (!dest) return;
    try {
      const msg = await invoke<string>("recover_file", {
        fileId: file.id,
        destinationPath: dest,
      });
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: msg,
          type: "found" as const,
        },
      ]);
    } catch (e) {
      console.error(e);
    }
  }

  async function startShred(path: string, algorithm: string, verify: boolean) {
    setShredState("shredding");
    await invoke("start_shred", {
      targetPath: path,
      algorithm,
      verify,
    });
  }

  async function cancelShred() {
    await invoke("cancel_shred");
    setShredState("idle");
    setShredError(null);
  }

  function resetShred() {
    setShredState("idle");
    setShredProgress(null);
    setShredError(null);
  }

  // ── Render ───────────────────────────────────────────────────────────────
  return (
    <div
      className="min-h-screen text-white select-none"
      style={{ backgroundColor: "#0a0a0a", fontFamily: "'Inter', sans-serif" }}
    >
      {/* ── Scanlines overlay ── */}
      <div
        className="fixed inset-0 pointer-events-none z-0 opacity-[0.03]"
        style={{
          backgroundImage:
            "repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(0,212,255,0.3) 2px, rgba(0,212,255,0.3) 3px)",
        }}
      />

      {/* ── Header ── */}
      <header className="relative z-10 flex items-center justify-between px-6 py-3 border-b border-[#1a1a2e] bg-[#0a0a0a]/90 backdrop-blur">
        <div className="flex items-center gap-3">
          <div className="relative">
            <Cpu size={22} className="text-[#00d4ff]" />
            <span className="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 rounded-full bg-[#00d4ff] animate-pulse" />
          </div>
          <div>
            <h1 className="text-sm font-bold tracking-widest uppercase text-white">
              Aeon Data Systems
            </h1>
            <p className="text-[10px] text-[#00d4ff]/50 tracking-widest uppercase">
              Forensic Recovery &amp; Military-Grade Shredder
            </p>
          </div>
        </div>

        <nav className="flex items-center gap-1">
          {(
            [
              { id: "dashboard", label: "Dashboard", Icon: LayoutDashboard },
              { id: "hunter", label: "The Hunter", Icon: Search },
              { id: "oblivion", label: "The Oblivion", Icon: ShieldOff },
            ] as const
          ).map(({ id, label, Icon }) => (
            <button
              key={id}
              onClick={() => setView(id)}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs transition-all ${
                view === id
                  ? "bg-[#00d4ff]/10 text-[#00d4ff] border border-[#00d4ff]/30"
                  : "text-gray-500 hover:text-gray-300 border border-transparent"
              }`}
            >
              <Icon size={13} />
              {label}
            </button>
          ))}
        </nav>

        <div className="flex items-center gap-2 text-[10px] text-gray-700 font-mono">
          <Activity size={11} className="text-green-500" />
          CORE ENGINE v1.0.0
        </div>
      </header>

      {/* ── Body ── */}
      <main className="relative z-10 flex h-[calc(100vh-49px)]">
        {/* ── Sidebar ── */}
        <aside className="w-60 shrink-0 border-r border-[#1a1a2e] flex flex-col">
          {/* Header — fixed, not scrollable */}
          <div className="px-4 pt-4 pb-2 shrink-0 flex items-center justify-between">
            <div>
              <span className="text-[10px] text-gray-600 uppercase tracking-wider">
                Storage Devices
              </span>
              <p className="text-[9px] text-gray-700 mt-0.5">
                Select one to scan or shred
              </p>
            </div>
            <button
              onClick={loadDisks}
              className="p-1 rounded hover:bg-[#1a1a2e] text-gray-600 hover:text-[#00d4ff] transition-colors"
              title="Refresh devices"
            >
              <RefreshCw size={12} className={disksLoading ? "animate-spin" : ""} />
            </button>
          </div>

          {/* Disk list — scrollable */}
          <div className="flex-1 overflow-y-auto px-4 pb-2">
            <DiskSelector
              disks={disks}
              selected={selectedDisk}
              onSelect={setSelectedDisk}
              loading={disksLoading}
            />
            {disks.length === 0 && !disksLoading && (
              <button
                onClick={loadDisks}
                className="w-full mt-2 py-2 text-xs rounded-lg border border-[#1a1a2e] text-gray-600 hover:border-[#00d4ff]/30 hover:text-[#00d4ff] transition-all"
              >
                Enumerate Devices
              </button>
            )}
          </div>

          {/* Session Stats — sticky at bottom, always visible */}
          <div className="shrink-0 border-t border-[#1a1a2e] px-4 py-3 space-y-1.5">
            <span className="text-[9px] text-gray-700 uppercase tracking-wider">
              Session Stats
            </span>
            <div className="space-y-1">
              {[
                {
                  label: "Files Found",
                  value: recoveredFiles.length > 0 ? String(recoveredFiles.length) : "—",
                  color: recoveredFiles.length > 0 ? "text-[#00d4ff]" : "text-gray-700",
                },
                {
                  label: "Progress",
                  value: scanProgress
                    ? `${Math.round((scanProgress.bytes_scanned / Math.max(scanProgress.total_bytes, 1)) * 100)}%`
                    : "—",
                  color: "text-green-400",
                },
                {
                  label: "Speed",
                  value: scanProgress && scanProgress.scan_speed_mb > 0
                    ? `${scanProgress.scan_speed_mb.toFixed(1)} MB/s`
                    : "—",
                  color: "text-yellow-400",
                },
              ].map(({ label, value, color }) => (
                <div key={label} className="flex justify-between text-[10px]">
                  <span className="text-gray-600">{label}</span>
                  <span className={`font-mono ${color}`}>{value}</span>
                </div>
              ))}
            </div>
          </div>
        </aside>

        {/* ── Main Content ── */}
        <div className="flex-1 overflow-y-auto">
          {/* ── Dashboard ── */}
          {view === "dashboard" && (
            <div className="p-6 space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-white">System Overview</h2>
                <p className="text-xs text-gray-600 mt-1 leading-relaxed max-w-2xl">
                  <strong className="text-gray-500">Aeon Data Systems</strong> is a forensic recovery and secure data-destruction tool.
                  Use <span className="text-[#00d4ff]/70">The Hunter</span> to scan a storage device for deleted or hidden files,
                  and <span className="text-red-400/70">The Oblivion</span> to permanently destroy sensitive data beyond recovery.
                  Start by selecting a storage device from the left sidebar.
                </p>
              </div>
              <div className="grid grid-cols-3 gap-4">
                {[
                  {
                    label: "Total Devices",
                    value: disks.length,
                    icon: Database,
                    color: "#00d4ff",
                  },
                  {
                    label: "Recovered Files",
                    value: recoveredFiles.length,
                    icon: Search,
                    color: "#22c55e",
                  },
                  {
                    label: "Scan Status",
                    value:
                      scanState === "scanning"
                        ? "Active"
                        : scanState === "complete"
                        ? "Done"
                        : "Idle",
                    icon: Activity,
                    color: scanState === "scanning" ? "#facc15" : "#6b7280",
                  },
                ].map(({ label, value, icon: Icon, color }) => (
                  <div
                    key={label}
                    className="bg-[#0d0d1a] border border-[#1a1a2e] rounded-xl p-5"
                  >
                    <div className="flex items-center justify-between mb-3">
                      <span className="text-xs text-gray-600">{label}</span>
                      <Icon size={16} style={{ color }} />
                    </div>
                    <div
                      className="text-3xl font-bold font-mono"
                      style={{ color }}
                    >
                      {value}
                    </div>
                  </div>
                ))}
              </div>

              <div className="bg-[#0d0d1a] border border-[#1a1a2e] rounded-xl p-5">
                <h3 className="text-sm text-gray-400 mb-4">Quick Actions</h3>
                <div className="grid grid-cols-2 gap-3">
                  <button
                    onClick={() => { loadDisks(); setView("hunter"); }}
                    className="flex items-start gap-2 p-3 rounded-lg border border-[#1a1a2e] hover:border-[#00d4ff]/40 hover:bg-[#00d4ff]/5 text-gray-400 hover:text-[#00d4ff] text-sm transition-all text-left"
                  >
                    <Search size={16} className="mt-0.5 shrink-0" />
                    <div>
                      <div>Start Forensic Scan</div>
                      <div className="text-[10px] text-gray-600 mt-0.5 font-normal">
                        Recover deleted files via sector-level carving
                      </div>
                    </div>
                  </button>
                  <button
                    onClick={() => setView("oblivion")}
                    className="flex items-start gap-2 p-3 rounded-lg border border-[#1a1a2e] hover:border-red-700/40 hover:bg-red-900/10 text-gray-400 hover:text-red-400 text-sm transition-all text-left"
                  >
                    <ShieldOff size={16} className="mt-0.5 shrink-0" />
                    <div>
                      <div>Secure Shred</div>
                      <div className="text-[10px] text-gray-600 mt-0.5 font-normal">
                        Permanently destroy data (DoD / Gutmann / NVMe)
                      </div>
                    </div>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* ── Hunter View ── */}
          {view === "hunter" && (
            <div className="p-6 space-y-4">
              <div className="flex items-center justify-between">
                <div>
                  <h2 className="text-lg font-semibold text-white">
                    The Hunter — Forensic Recovery
                  </h2>
                  <p className="text-xs text-gray-600 mt-0.5 max-w-lg">
                    Reads the selected device sector by sector, searching for JPEG, PNG, PDF, ZIP, EXE, MP4 and MP3 file signatures.
                    Deleted or hidden files are listed below for recovery.
                    {!selectedDisk && (
                      <span className="text-yellow-500/80 ml-1">← Select a device from the sidebar first.</span>
                    )}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {scanState !== "scanning" ? (
                    <button
                      onClick={startScan}
                      disabled={!selectedDisk}
                      className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#00d4ff]/10 border border-[#00d4ff]/30 text-[#00d4ff] text-sm font-medium hover:bg-[#00d4ff]/20 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                    >
                      <Play size={14} />
                      {scanState === "complete" ? "Re-scan" : "Start Scan"}
                    </button>
                  ) : (
                    <button
                      onClick={cancelScan}
                      className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#1a1a2e] border border-[#1a1a2e] text-gray-400 text-sm font-medium hover:border-red-700/50 hover:text-red-400 transition-all"
                    >
                      <Square size={14} />
                      Cancel
                    </button>
                  )}
                </div>
              </div>

              {/* Rich scan progress card */}
              {scanState === "scanning" && scanProgress && (() => {
                const pct = scanProgress.total_bytes > 0
                  ? Math.round((scanProgress.bytes_scanned / scanProgress.total_bytes) * 100)
                  : 0;
                const remaining = scanProgress.total_bytes - scanProgress.bytes_scanned;
                const speedBps = scanProgress.scan_speed_mb * 1024 * 1024;
                const etaSec = speedBps > 0 ? remaining / speedBps : null;
                const fmtTime = (s: number) => {
                  const m = Math.floor(s / 60);
                  const sec = Math.floor(s % 60);
                  return m > 0 ? `${m}m ${sec}s` : `${sec}s`;
                };
                const fmtBytes = (b: number) =>
                  b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(0)} MB` : `${(b / 1e3).toFixed(0)} KB`;

                return (
                  <div className="border border-[#1a1a2e] bg-[#08080f] rounded-lg p-3 space-y-2">
                    {/* Progress bar */}
                    <div className="flex items-center gap-3">
                      <div className="flex-1 h-2 bg-[#1a1a2e] rounded-full overflow-hidden">
                        <div
                          className="h-full rounded-full bg-gradient-to-r from-[#00d4ff]/50 to-[#00d4ff] transition-all duration-500"
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                      <span className="text-[#00d4ff] font-mono text-xs shrink-0 w-9 text-right">{pct}%</span>
                    </div>

                    {/* Stats grid */}
                    <div className="grid grid-cols-4 gap-2 text-[10px]">
                      <div>
                        <div className="text-gray-700">Scanned</div>
                        <div className="text-gray-300 font-mono">{fmtBytes(scanProgress.bytes_scanned)}</div>
                      </div>
                      <div>
                        <div className="text-gray-700">Speed</div>
                        <div className="text-yellow-400 font-mono">{scanProgress.scan_speed_mb.toFixed(1)} MB/s</div>
                      </div>
                      <div>
                        <div className="text-gray-700">ETA</div>
                        <div className="text-green-400 font-mono">{etaSec !== null ? fmtTime(etaSec) : "—"}</div>
                      </div>
                      <div>
                        <div className="text-gray-700">Files</div>
                        <div className="text-[#00d4ff] font-mono">{scanProgress.files_found}</div>
                      </div>
                    </div>

                    {/* Elapsed + offset */}
                    <div className="flex justify-between text-[9px] text-gray-700 font-mono">
                      <span>Elapsed: {fmtTime(scanProgress.elapsed_seconds)}</span>
                      <span>@ {scanProgress.current_offset_hex}</span>
                    </div>
                  </div>
                );
              })()}

              {/* Terminal */}
              <HexTerminal
                logs={terminalLogs.filter(Boolean) as any}
                progress={scanProgress}
                isScanning={scanState === "scanning"}
              />

              {/* Results table */}
              <RecoveryTable
                files={recoveredFiles}
                onRecover={recoverFile}
                onPreview={() => {}}
                loading={scanState === "scanning" && recoveredFiles.length === 0}
              />
            </div>
          )}

          {/* ── Oblivion View ── */}
          {view === "oblivion" && (
            <div className="p-6 max-w-xl space-y-4">
              <div>
                <h2 className="text-lg font-semibold text-white">
                  The Oblivion — Military-Grade Shredder
                </h2>
                <p className="text-xs text-gray-600 mt-0.5 leading-relaxed">
                  Overwrites a file or entire drive with cryptographically random data using
                  DoD 5220.22-M, Gutmann 35-pass, or NVMe hardware sanitize.
                  Enter a file path or a device path (e.g.{" "}
                  <code className="font-mono text-gray-500">\\.\C:</code>
                  {selectedDisk && (
                    <span className="text-[#00d4ff]/60"> — selected: <code className="font-mono">{selectedDisk}</code></span>
                  )}
                  ).
                </p>
              </div>

              <ShredPanel
                progress={shredProgress}
                state={shredState}
                error={shredError}
                selectedDiskPath={selectedDisk}
                onStart={startShred}
                onCancel={cancelShred}
                onReset={resetShred}
              />
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
