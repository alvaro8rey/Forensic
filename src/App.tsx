import React, { useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
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
  Globe,
} from "lucide-react";

import { DiskSelector } from "./components/DiskSelector";
import { HexTerminal, buildLogEntry } from "./components/HexTerminal";
import { RecoveryTable } from "./components/RecoveryTable";
import { ShredPanel } from "./components/ShredPanel";
import { useTauriEvents } from "./hooks/useTauriEvents";
import { SUPPORTED_LANGUAGES, LangCode } from "./i18n";
import {
  AppView,
  DiskInfo,
  RecoveredFile,
  RecoverResult,
  ScanProgress,
  ScanState,
  ShredProgress,
  ShredState,
} from "./types";

export default function App() {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<AppView>("dashboard");
  const [langMenuOpen, setLangMenuOpen] = useState(false);

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

  // Preview modal
  const [previewData, setPreviewData] = useState<{ mime: string; b64: string; type: string } | null>(null);

  // ── Tauri event listeners ────────────────────────────────────────────────
  useTauriEvents({
    onScanProgress: useCallback((p: ScanProgress) => {
      setScanProgress(p);
      const entry = buildLogEntry(p, prevFilesRef.current, t);
      if (entry) {
        setTerminalLogs((l) => [...l.slice(-200), entry]);
        prevFilesRef.current = p.files_found;
      }
    }, [t]),

    onScanComplete: useCallback((files: RecoveredFile[]) => {
      setRecoveredFiles(files);
      setScanState("complete");
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: t("hunter.scanComplete", { count: files.length }),
          type: "found" as const,
        },
      ]);
    }, [t]),

    onScanError: useCallback((err: string) => {
      setScanState("error");
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: t("hunter.scanError", { error: err }),
          type: "error" as const,
        },
      ]);
    }, [t]),

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
      title: t("recovery.recoverFile"),
    });
    if (!dest) return;
    try {
      const result = await invoke<RecoverResult>("recover_file", {
        fileId: file.id,
        destinationPath: dest,
      });
      const msg = t(result.key, {
        fileType: result.file_type,
        kb: result.kb,
        path: result.path,
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

  async function previewFile(file: RecoveredFile) {
    try {
      const raw = await invoke<string>("preview_file", { fileId: file.id });
      const colonIdx = raw.indexOf(":");
      if (colonIdx === -1) return;
      const mime = raw.slice(0, colonIdx);
      const b64 = raw.slice(colonIdx + 1);
      setPreviewData({ mime, b64, type: String(file.file_type) });
    } catch (e) {
      console.error("Preview failed:", e);
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

  function switchLanguage(code: LangCode) {
    i18n.changeLanguage(code);
    setLangMenuOpen(false);
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
              {t("app.title")}
            </h1>
            <p className="text-[10px] text-[#00d4ff]/50 tracking-widest uppercase">
              {t("app.subtitle")}
            </p>
          </div>
        </div>

        <nav className="flex items-center gap-1">
          {(
            [
              { id: "dashboard", labelKey: "nav.dashboard", Icon: LayoutDashboard },
              { id: "hunter",    labelKey: "nav.hunter",    Icon: Search },
              { id: "oblivion",  labelKey: "nav.oblivion",  Icon: ShieldOff },
            ] as const
          ).map(({ id, labelKey, Icon }) => (
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
              {t(labelKey)}
            </button>
          ))}
        </nav>

        <div className="flex items-center gap-3">
          {/* ── Language selector ── */}
          <div className="relative">
            <button
              onClick={() => setLangMenuOpen((o) => !o)}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs border border-[#1a1a2e] text-gray-500 hover:text-[#00d4ff] hover:border-[#00d4ff]/30 transition-all"
              title={t("settings.language")}
            >
              <Globe size={12} />
              <span className="font-mono uppercase">
                {i18n.language?.slice(0, 2) ?? "en"}
              </span>
            </button>
            {langMenuOpen && (
              <div className="absolute right-0 top-full mt-1 w-36 bg-[#0d0d1a] border border-[#1a1a2e] rounded-lg shadow-xl z-50 overflow-hidden">
                <div className="px-3 py-1.5 text-[9px] text-gray-600 uppercase tracking-wider border-b border-[#1a1a2e]">
                  {t("settings.language")}
                </div>
                {SUPPORTED_LANGUAGES.map((lang) => (
                  <button
                    key={lang.code}
                    onClick={() => switchLanguage(lang.code)}
                    className={`w-full text-left px-3 py-2 text-xs transition-colors flex items-center justify-between ${
                      (i18n.language?.slice(0, 2) ?? "en") === lang.code
                        ? "text-[#00d4ff] bg-[#00d4ff]/5"
                        : "text-gray-400 hover:text-white hover:bg-[#1a1a2e]"
                    }`}
                  >
                    <span>{lang.nativeLabel}</span>
                    {(i18n.language?.slice(0, 2) ?? "en") === lang.code && (
                      <span className="w-1.5 h-1.5 rounded-full bg-[#00d4ff]" />
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="flex items-center gap-2 text-[10px] text-gray-700 font-mono">
            <Activity size={11} className="text-green-500" />
            {t("app.version")}
          </div>
        </div>
      </header>

      {/* ── Body ── */}
      <main className="relative z-10 flex h-[calc(100vh-49px)]">
        {/* ── Sidebar ── */}
        <aside className="w-60 shrink-0 border-r border-[#1a1a2e] flex flex-col">
          {/* Header — fixed */}
          <div className="px-4 pt-4 pb-2 shrink-0 flex items-center justify-between">
            <div>
              <span className="text-[10px] text-gray-600 uppercase tracking-wider">
                {t("sidebar.title")}
              </span>
              <p className="text-[9px] text-gray-700 mt-0.5">
                {t("sidebar.hint")}
              </p>
            </div>
            <button
              onClick={loadDisks}
              className="p-1 rounded hover:bg-[#1a1a2e] text-gray-600 hover:text-[#00d4ff] transition-colors"
              title={t("sidebar.refresh")}
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
                {t("sidebar.enumerate")}
              </button>
            )}
          </div>

          {/* Session Stats — sticky at bottom */}
          <div className="shrink-0 border-t border-[#1a1a2e] px-4 py-3 space-y-1.5">
            <span className="text-[9px] text-gray-700 uppercase tracking-wider">
              {t("sidebar.stats.title")}
            </span>
            <div className="space-y-1">
              {[
                {
                  label: t("sidebar.stats.filesFound"),
                  value: recoveredFiles.length > 0 ? String(recoveredFiles.length) : "—",
                  color: recoveredFiles.length > 0 ? "text-[#00d4ff]" : "text-gray-700",
                },
                {
                  label: t("sidebar.stats.progress"),
                  value: scanProgress
                    ? `${Math.round((scanProgress.bytes_scanned / Math.max(scanProgress.total_bytes, 1)) * 100)}%`
                    : "—",
                  color: "text-green-400",
                },
                {
                  label: t("sidebar.stats.speed"),
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
        <div className="flex-1 overflow-y-auto" onClick={() => langMenuOpen && setLangMenuOpen(false)}>

          {/* ── Dashboard ── */}
          {view === "dashboard" && (
            <div className="p-6 space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-white">{t("dashboard.title")}</h2>
                <p className="text-xs text-gray-600 mt-1 leading-relaxed max-w-2xl">
                  <strong className="text-gray-500">{t("app.title")}</strong>{" "}
                  {t("dashboard.title") === "System Overview"
                    ? "is a forensic recovery and secure data-destruction tool. Use "
                    : "es una herramienta de recuperación forense y destrucción segura de datos. Usa "}
                  <span className="text-[#00d4ff]/70">{t("nav.hunter")}</span>
                  {t("dashboard.title") === "System Overview"
                    ? " to scan a storage device for deleted or hidden files, and "
                    : " para escanear un dispositivo en busca de archivos eliminados u ocultos, y "}
                  <span className="text-red-400/70">{t("nav.oblivion")}</span>
                  {t("dashboard.title") === "System Overview"
                    ? " to permanently destroy sensitive data beyond recovery. Start by selecting a storage device from the left sidebar."
                    : " para destruir permanentemente datos confidenciales. Empieza seleccionando un dispositivo en la barra lateral."}
                </p>
              </div>
              <div className="grid grid-cols-3 gap-4">
                {[
                  {
                    label: t("dashboard.totalDevices"),
                    value: disks.length,
                    icon: Database,
                    color: "#00d4ff",
                  },
                  {
                    label: t("dashboard.recoveredFiles"),
                    value: recoveredFiles.length,
                    icon: Search,
                    color: "#22c55e",
                  },
                  {
                    label: t("dashboard.scanStatus"),
                    value:
                      scanState === "scanning"
                        ? t("dashboard.status.active")
                        : scanState === "complete"
                        ? t("dashboard.status.done")
                        : t("dashboard.status.idle"),
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
                    <div className="text-3xl font-bold font-mono" style={{ color }}>
                      {value}
                    </div>
                  </div>
                ))}
              </div>

              <div className="bg-[#0d0d1a] border border-[#1a1a2e] rounded-xl p-5">
                <h3 className="text-sm text-gray-400 mb-4">{t("dashboard.quickActions")}</h3>
                <div className="grid grid-cols-2 gap-3">
                  <button
                    onClick={() => { loadDisks(); setView("hunter"); }}
                    className="flex items-start gap-2 p-3 rounded-lg border border-[#1a1a2e] hover:border-[#00d4ff]/40 hover:bg-[#00d4ff]/5 text-gray-400 hover:text-[#00d4ff] text-sm transition-all text-left"
                  >
                    <Search size={16} className="mt-0.5 shrink-0" />
                    <div>
                      <div>{t("dashboard.startScan")}</div>
                      <div className="text-[10px] text-gray-600 mt-0.5 font-normal">
                        {t("dashboard.startScanDesc")}
                      </div>
                    </div>
                  </button>
                  <button
                    onClick={() => setView("oblivion")}
                    className="flex items-start gap-2 p-3 rounded-lg border border-[#1a1a2e] hover:border-red-700/40 hover:bg-red-900/10 text-gray-400 hover:text-red-400 text-sm transition-all text-left"
                  >
                    <ShieldOff size={16} className="mt-0.5 shrink-0" />
                    <div>
                      <div>{t("dashboard.secureShred")}</div>
                      <div className="text-[10px] text-gray-600 mt-0.5 font-normal">
                        {t("dashboard.secureShredDesc")}
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
                    {t("hunter.title")}
                  </h2>
                  <p className="text-xs text-gray-600 mt-0.5 max-w-lg">
                    {t("hunter.description")}
                    {!selectedDisk && (
                      <span className="text-yellow-500/80 ml-1">
                        {t("hunter.selectFirst")}
                      </span>
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
                      {scanState === "complete" ? t("hunter.rescan") : t("hunter.startScan")}
                    </button>
                  ) : (
                    <button
                      onClick={cancelScan}
                      className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#1a1a2e] border border-[#1a1a2e] text-gray-400 text-sm font-medium hover:border-red-700/50 hover:text-red-400 transition-all"
                    >
                      <Square size={14} />
                      {t("hunter.cancel")}
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
                    <div className="flex items-center gap-3">
                      <div className="flex-1 h-2 bg-[#1a1a2e] rounded-full overflow-hidden">
                        <div
                          className="h-full rounded-full bg-gradient-to-r from-[#00d4ff]/50 to-[#00d4ff] transition-all duration-500"
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                      <span className="text-[#00d4ff] font-mono text-xs shrink-0 w-9 text-right">{pct}%</span>
                    </div>
                    <div className="grid grid-cols-4 gap-2 text-[10px]">
                      <div>
                        <div className="text-gray-700">{t("hunter.progress.scanned")}</div>
                        <div className="text-gray-300 font-mono">{fmtBytes(scanProgress.bytes_scanned)}</div>
                      </div>
                      <div>
                        <div className="text-gray-700">{t("hunter.progress.speed")}</div>
                        <div className="text-yellow-400 font-mono">{scanProgress.scan_speed_mb.toFixed(1)} MB/s</div>
                      </div>
                      <div>
                        <div className="text-gray-700">{t("hunter.progress.eta")}</div>
                        <div className="text-green-400 font-mono">{etaSec !== null ? fmtTime(etaSec) : "—"}</div>
                      </div>
                      <div>
                        <div className="text-gray-700">{t("hunter.progress.files")}</div>
                        <div className="text-[#00d4ff] font-mono">{scanProgress.files_found}</div>
                      </div>
                    </div>
                    <div className="flex justify-between text-[9px] text-gray-700 font-mono">
                      <span>{t("hunter.progress.elapsed")}: {fmtTime(scanProgress.elapsed_seconds)}</span>
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
                onPreview={previewFile}
                loading={scanState === "scanning" && recoveredFiles.length === 0}
              />
            </div>
          )}

          {/* ── Oblivion View ── */}
          {view === "oblivion" && (
            <div className="p-6 max-w-2xl space-y-4">
              <div>
                <h2 className="text-lg font-semibold text-white">
                  {t("oblivion.title")}
                </h2>
                <p className="text-xs text-gray-600 mt-0.5 leading-relaxed">
                  {t("oblivion.description", { path: "\\\\.\\ C:" })}
                  {selectedDisk && (
                    <span className="text-[#00d4ff]/60">
                      {" "}{t("oblivion.selected")}{" "}
                      <code className="font-mono">{selectedDisk}</code>
                    </span>
                  )}
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

      {/* ── Preview Modal ── */}
      {previewData && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm"
          onClick={() => setPreviewData(null)}
        >
          <div
            className="relative max-w-3xl max-h-[85vh] bg-[#0a0a0a] border border-[#1a1a2e] rounded-xl overflow-hidden shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between px-4 py-2 border-b border-[#1a1a2e]">
              <span className="text-xs text-gray-400 font-mono">
                {t("preview.title", { type: previewData.type })}
              </span>
              <button
                onClick={() => setPreviewData(null)}
                className="text-gray-600 hover:text-white text-lg leading-none px-1"
              >
                {t("preview.close")}
              </button>
            </div>
            <div className="p-4 overflow-auto max-h-[calc(85vh-42px)]">
              {previewData.mime.startsWith("image/") ? (
                <img
                  src={`data:${previewData.mime};base64,${previewData.b64}`}
                  alt="Preview"
                  className="max-w-full max-h-[70vh] object-contain rounded"
                />
              ) : (
                <pre className="text-xs text-gray-400 font-mono whitespace-pre-wrap break-all">
                  {atob(previewData.b64).slice(0, 4096)}
                </pre>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
