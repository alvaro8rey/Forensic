import React, { useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/tauri";
import { save, open as openDialog } from "@tauri-apps/api/dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/api/fs";
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
  FolderOpen,
  FileDown,
  PackageOpen,
  Upload,
  FolderTree,
  CheckCircle2,
  Image,
  BookOpen,
  Video,
  Music,
  Archive,
  File,
} from "lucide-react";

import { DiskSelector } from "./components/DiskSelector";
import { HexTerminal, buildLogEntry } from "./components/HexTerminal";
import { RecoveryTable } from "./components/RecoveryTable";
import { ShredPanel } from "./components/ShredPanel";
import { useTauriEvents } from "./hooks/useTauriEvents";
import { SUPPORTED_LANGUAGES, LangCode } from "./i18n";
import {
  AppView,
  BatchRecoverResult,
  DiskInfo,
  RecoveredFile,
  RecoverResult,
  ScanProgress,
  ScanState,
  ShredProgress,
  ShredState,
  WipeState,
} from "./types";

interface OrganizedRecoverySummary {
  total: number;
  ok: number;
  failed: number;
  by_folder: Record<string, number>;
}

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
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [terminalLogs, setTerminalLogs] = useState<ReturnType<typeof buildLogEntry>[]>([]);
  const prevFilesRef = useRef(0);
  const mainScrollRef = useRef<HTMLDivElement>(null);

  // Synchronous scroll reset — called inside every onClick that changes view
  // so it runs BEFORE React re-renders (useEffect fires too late: the browser
  // may scroll to a focused element between the render and the effect).
  function scrollToTop() {
    if (mainScrollRef.current) mainScrollRef.current.scrollTop = 0;
  }

  // Guards against scan-complete / scan-error events arriving after the user
  // already cancelled — those stale updates cause inconsistent state and a
  // black screen because there is no React Error Boundary to catch the crash.
  const scanCancelledRef = useRef(false);

  // Scan profile
  type ScanProfile = "fast" | "full" | "custom";
  const [scanProfile, setScanProfile] = useState<ScanProfile>("full");
  const [customTypes, setCustomTypes] = useState<Set<string>>(new Set());

  const ALL_FILE_TYPES: { type: string; group: string }[] = [
    { type: "JPEG", group: "img" }, { type: "PNG", group: "img" }, { type: "GIF", group: "img" },
    { type: "TIFF", group: "img" }, { type: "BMP", group: "img" },
    { type: "PDF", group: "doc" }, { type: "DOCX", group: "doc" }, { type: "XLSX", group: "doc" },
    { type: "PPTX", group: "doc" }, { type: "DOC", group: "doc" }, { type: "TXT", group: "doc" },
    { type: "ZIP", group: "arc" }, { type: "RAR", group: "arc" }, { type: "SevenZ", group: "arc" },
    { type: "EXE", group: "exe" },
    { type: "MP4", group: "vid" }, { type: "AVI", group: "vid" }, { type: "MKV", group: "vid" },
    { type: "MP3", group: "aud" }, { type: "WAV", group: "aud" }, { type: "FLAC", group: "aud" },
    { type: "SQLite", group: "db" },
  ];
  const GROUP_COLORS: Record<string, string> = {
    img: "text-pink-400", doc: "text-orange-400", arc: "text-yellow-400",
    exe: "text-red-400", vid: "text-purple-400", aud: "text-green-400", db: "text-blue-400",
  };

  function toggleCustomType(type: string) {
    setCustomTypes((prev) => {
      const next = new Set(prev);
      if (next.has(type)) next.delete(type); else next.add(type);
      return next;
    });
  }

  // Shred state
  const [shredState, setShredState] = useState<ShredState>("idle");
  const [shredProgress, setShredProgress] = useState<ShredProgress | null>(null);
  const [shredError, setShredError] = useState<string | null>(null);

  // Free space wipe state
  const [wipeState, setWipeState] = useState<WipeState>("idle");
  const [wipeProgress, setWipeProgress] = useState<ShredProgress | null>(null);
  const [wipeError, setWipeError] = useState<string | null>(null);

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
      // Ignore stale events that arrive after the user cancelled the scan
      if (scanCancelledRef.current) return;
      const safeFiles = Array.isArray(files) ? files : [];
      setRecoveredFiles(safeFiles);
      setScanState("complete");
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: t("hunter.scanComplete", { count: safeFiles.length }),
          type: "found" as const,
        },
      ]);
    }, [t]),

    onScanError: useCallback((err: string) => {
      // Ignore stale error events after user-initiated cancel
      if (scanCancelledRef.current) return;
      setScanState("idle");   // treat as idle, not a hard error state
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

    onWipeProgress: useCallback((p: ShredProgress) => {
      setWipeProgress(p);
    }, []),

    onWipeComplete: useCallback(() => {
      setWipeState("complete");
      setWipeError(null);
    }, []),

    onWipeError: useCallback((err: string) => {
      setWipeState("error");
      setWipeError(err);
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
    scanCancelledRef.current = false;
    setScanState("scanning");
    setRecoveredFiles([]);
    setSelectedIds(new Set());
    setTerminalLogs([]);
    prevFilesRef.current = 0;
    await invoke("start_scan", {
      devicePath: selectedDisk,
      scanProfile,
      customTypes: scanProfile === "custom" ? Array.from(customTypes) : [],
    });
  }

  async function cancelScan() {
    // Mark as cancelled BEFORE invoking so any in-flight events are ignored
    scanCancelledRef.current = true;
    try { await invoke("cancel_scan"); } catch { /* ignore */ }
    setScanState("idle");
    setScanProgress(null);
  }

  function getFileExt(fileType: string): string {
    const map: Record<string, string> = {
      JPEG: "jpg", PNG: "png", GIF: "gif", TIFF: "tif", BMP: "bmp",
      PDF: "pdf", DOCX: "docx", DOC: "doc", ZIP: "zip",
      RAR: "rar", "7Z": "7z", EXE: "exe",
      MP4: "mp4", AVI: "avi", MKV: "mkv",
      MP3: "mp3", WAV: "wav", FLAC: "flac",
      SQLite: "db", TXT: "txt",
    };
    return map[fileType] ?? "bin";
  }

  function addLog(message: string, type: "found" | "error" | "info" = "info") {
    setTerminalLogs((l) => [
      ...l,
      { timestamp: new Date().toTimeString().slice(0, 8), offset: "—", message, type },
    ]);
  }

  async function recoverFile(file: RecoveredFile) {
    const ext = getFileExt(file.file_type);
    // Prefer the original filename if the scanner recovered it from the directory
    const defaultName = file.original_name ?? `recovered_${file.file_type.toLowerCase()}_${file.id}.${ext}`;
    const dest = await save({
      defaultPath: defaultName,
      filters: [
        { name: file.file_type, extensions: [ext] },
        { name: "All files", extensions: ["*"] },
      ],
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
      addLog(msg, "found");
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
      addLog(`✗ Recovery failed: ${msg}`, "error");
    }
  }

  async function previewFile(file: RecoveredFile) {
    try {
      const raw = await invoke<string>("preview_file", { fileId: file.id });
      const colonIdx = raw.indexOf(":");
      if (colonIdx === -1) {
        addLog("✗ Preview failed: unexpected response format", "error");
        return;
      }
      const mime = raw.slice(0, colonIdx);
      const b64 = raw.slice(colonIdx + 1);
      setPreviewData({ mime, b64, type: String(file.file_type) });
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
      addLog(`✗ Preview failed: ${msg}`, "error");
    }
  }

  async function recoverBatch() {
    if (selectedIds.size === 0) return;
    const folder = await openDialog({ directory: true, title: t("recovery.batchFolder") });
    if (!folder || typeof folder !== "string") return;
    try {
      const results = await invoke<BatchRecoverResult[]>("recover_batch", {
        fileIds: Array.from(selectedIds),
        destinationFolder: folder,
      });
      const ok = results.filter((r) => r.success).length;
      const fail = results.length - ok;
      setTerminalLogs((l) => [
        ...l,
        {
          timestamp: new Date().toTimeString().slice(0, 8),
          offset: "—",
          message: `Batch recovery: ${ok} OK, ${fail} failed → ${folder}`,
          type: "found" as const,
        },
      ]);
    } catch (e) {
      console.error(e);
    }
  }

  async function recoverAllOrganized() {
    const ids = selectedIds.size > 0 ? Array.from(selectedIds) : [];
    const folder = await openDialog({
      directory: true,
      title: "Choose destination — files will be sorted into Images/, Documents/, Videos/…",
    });
    if (!folder || typeof folder !== "string") return;
    try {
      const summary = await invoke<OrganizedRecoverySummary>("recover_all_organized", {
        fileIds: ids,
        destinationFolder: folder,
      });
      const breakdown = Object.entries(summary.by_folder)
        .map(([k, v]) => `${v} ${k}`)
        .join(" · ");
      addLog(
        `✓ Recovered ${summary.ok}/${summary.total} files → ${folder}  [${breakdown}]`,
        "found"
      );
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
      addLog(`✗ Organized recovery failed: ${msg}`, "error");
    }
  }

  async function exportCsv() {
    if (recoveredFiles.length === 0) return;
    const dest = await save({ defaultPath: "scan_results.csv", filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!dest) return;
    const header = "id,file_type,original_name,size_bytes,offset_start,recovery_probability,is_fragmented,sector_overwritten\n";
    const rows = recoveredFiles.map((f) =>
      `${f.id},${f.file_type},${f.original_name ?? ""},${f.size_bytes},${f.offset_start},${f.recovery_probability},${f.is_fragmented},${f.sector_overwritten}`
    ).join("\n");
    await writeTextFile(dest, header + rows);
    setTerminalLogs((l) => [...l, { timestamp: new Date().toTimeString().slice(0, 8), offset: "—", message: `CSV exported → ${dest}`, type: "found" as const }]);
  }

  async function exportJson() {
    if (recoveredFiles.length === 0) return;
    const dest = await save({ defaultPath: "scan_results.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!dest) return;
    const payload = { device_path: selectedDisk ?? "", scan_date: new Date().toISOString(), files: recoveredFiles };
    await writeTextFile(dest, JSON.stringify(payload, null, 2));
    addLog(`JSON exported → ${dest}`, "found");
  }

  async function importJson() {
    const src = await openDialog({
      filters: [{ name: "Aeon Scan JSON", extensions: ["json"] }],
      title: "Load scan results",
    });
    if (!src || typeof src !== "string") return;
    try {
      const raw = await readTextFile(src);
      const parsed = JSON.parse(raw);
      // Support both new {device_path, files} envelope and legacy plain array
      const files: RecoveredFile[] = Array.isArray(parsed) ? parsed : (parsed.files ?? []);
      const devicePath: string = parsed.device_path ?? selectedDisk ?? "";
      const count = await invoke<number>("import_scan_results", { devicePath, files });
      setRecoveredFiles(files);
      setScanState("complete");
      addLog(`✓ Loaded ${count} file(s) from ${src}`, "found");
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
      addLog(`✗ Import failed: ${msg}`, "error");
    }
  }

  async function exportZip() {
    const ids = selectedIds.size > 0 ? Array.from(selectedIds) : recoveredFiles.map((f) => f.id);
    if (ids.length === 0) return;
    const dest = await save({ defaultPath: "recovered_files.zip", filters: [{ name: "ZIP Archive", extensions: ["zip"] }] });
    if (!dest) return;
    try {
      const count = await invoke<number>("export_recovered_zip", { fileIds: ids, zipPath: dest });
      setTerminalLogs((l) => [...l, { timestamp: new Date().toTimeString().slice(0, 8), offset: "—", message: `ZIP export: ${count} files → ${dest}`, type: "found" as const }]);
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

  async function startWipeFreeSpace(dirPath: string) {
    setWipeState("wiping");
    setWipeProgress(null);
    setWipeError(null);
    await invoke("start_wipe_free_space", { targetDir: dirPath });
  }

  async function cancelWipeFreeSpace() {
    await invoke("cancel_wipe_free_space");
    setWipeState("idle");
    setWipeError(null);
  }

  function resetWipe() {
    setWipeState("idle");
    setWipeProgress(null);
    setWipeError(null);
  }

  function switchLanguage(code: LangCode) {
    i18n.changeLanguage(code);
    setLangMenuOpen(false);
  }

  // ── Pre-render computed values (keep JSX and logic out of the template) ──
  const TYPE_TO_GROUP: Record<string, string> = {
    JPEG: "images", PNG: "images", GIF: "images", TIFF: "images", BMP: "images",
    PDF: "documents", DOCX: "documents", DOC: "documents", XLSX: "documents", PPTX: "documents", TXT: "documents",
    MP4: "videos", AVI: "videos", MKV: "videos",
    MP3: "audio", WAV: "audio", FLAC: "audio",
    ZIP: "archives", RAR: "archives", SevenZ: "archives",
  };
  const FILE_GROUP_META: Record<string, { label: string; icon: React.ReactNode; color: string }> = {
    images:    { label: t("hunter.groups.images"),    icon: <Image    size={11} />, color: "text-pink-400" },
    documents: { label: t("hunter.groups.documents"), icon: <BookOpen size={11} />, color: "text-orange-400" },
    videos:    { label: t("hunter.groups.videos"),    icon: <Video    size={11} />, color: "text-purple-400" },
    audio:     { label: t("hunter.groups.audio"),     icon: <Music    size={11} />, color: "text-green-400" },
    archives:  { label: t("hunter.groups.archives"),  icon: <Archive  size={11} />, color: "text-yellow-400" },
    other:     { label: t("hunter.groups.other"),     icon: <File     size={11} />, color: "text-gray-500" },
  };
  const fileCounts: Record<string, number> = {};
  recoveredFiles.forEach((f) => {
    const g = TYPE_TO_GROUP[f.file_type] ?? "other";
    fileCounts[g] = (fileCounts[g] ?? 0) + 1;
  });

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
      <header className="relative z-30 flex items-center justify-between px-6 py-3 border-b border-[#1a1a2e] bg-[#0a0a0a]/90 backdrop-blur">
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
              onClick={() => { scrollToTop(); setView(id); }}
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
        <div ref={mainScrollRef} className="flex-1 overflow-y-auto" onClick={() => langMenuOpen && setLangMenuOpen(false)}>

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
                    onClick={() => { scrollToTop(); loadDisks(); setView("hunter"); }}
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
                    onClick={() => { scrollToTop(); setView("oblivion"); }}
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
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {scanState !== "scanning" ? (
                    <button
                      onClick={startScan}
                      disabled={!selectedDisk || (scanProfile === "custom" && customTypes.size === 0)}
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

              {/* ── Scan profile selector ── */}
              {scanState !== "scanning" && (
                <div className="border border-[#1a1a2e] bg-[#08080f] rounded-lg p-3 space-y-3">
                  <span className="text-[10px] text-gray-600 uppercase tracking-wider">
                    {t("hunter.profile.label")}
                  </span>
                  <div className="flex gap-2">
                    {(["fast", "full", "custom"] as const).map((p) => (
                      <button
                        key={p}
                        onClick={() => setScanProfile(p)}
                        className={`flex-1 flex flex-col items-center py-2 px-3 rounded-lg border text-xs transition-all ${
                          scanProfile === p
                            ? "border-[#00d4ff]/50 bg-[#00d4ff]/8 text-[#00d4ff]"
                            : "border-[#1a1a2e] text-gray-500 hover:text-gray-300 hover:border-gray-600"
                        }`}
                      >
                        <span className="font-semibold">{t(`hunter.profile.${p}`)}</span>
                        <span className="text-[9px] mt-0.5 opacity-70">{t(`hunter.profile.${p}Desc`)}</span>
                      </button>
                    ))}
                  </div>

                  {/* Custom type checkboxes */}
                  {scanProfile === "custom" && (
                    <div className="space-y-2 pt-1">
                      <div className="flex items-center justify-between">
                        <span className="text-[10px] text-gray-600">{t("hunter.profile.selectTypes")}</span>
                        <div className="flex gap-2">
                          <button
                            onClick={() => setCustomTypes(new Set(ALL_FILE_TYPES.map((f) => f.type)))}
                            className="text-[9px] text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors"
                          >
                            All
                          </button>
                          <button
                            onClick={() => setCustomTypes(new Set())}
                            className="text-[9px] text-gray-600 hover:text-gray-400 transition-colors"
                          >
                            None
                          </button>
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-1.5">
                        {ALL_FILE_TYPES.map(({ type, group }) => (
                          <button
                            key={type}
                            onClick={() => toggleCustomType(type)}
                            className={`px-2 py-0.5 rounded border text-[10px] font-mono transition-all ${
                              customTypes.has(type)
                                ? `${GROUP_COLORS[group]} border-current bg-current/10`
                                : "text-gray-700 border-[#1a1a2e] hover:text-gray-500"
                            }`}
                          >
                            {type}
                          </button>
                        ))}
                      </div>
                      {customTypes.size === 0 && (
                        <p className="text-[10px] text-yellow-500/70">
                          Select at least one file type.
                        </p>
                      )}
                    </div>
                  )}
                </div>
              )}

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

              {/* ── Scan summary bar (shown after scan completes) ── */}
              {scanState === "complete" && recoveredFiles.length > 0 && (
                <div className="flex items-center gap-3 px-3 py-2 bg-[#08080f] border border-[#00d4ff]/20 rounded-lg flex-wrap">
                  <div className="flex items-center gap-1.5 text-[#00d4ff]">
                    <CheckCircle2 size={13} />
                    <span className="text-xs font-semibold">{t("hunter.filesFound", { count: recoveredFiles.length })}</span>
                  </div>
                  <div className="w-px h-4 bg-[#1a1a2e]" />
                  {Object.entries(FILE_GROUP_META).map(([key, { label, icon, color }]) => {
                    const n = fileCounts[key];
                    if (!n) return null;
                    return (
                      <div key={key} className={`flex items-center gap-1 text-[11px] ${color}`}>
                        {icon}
                        <span className="font-mono font-semibold">{n}</span>
                        <span className="text-[10px] opacity-70">{label}</span>
                      </div>
                    );
                  })}
                </div>
              )}

              {/* Batch toolbar */}
              <div className="flex items-center gap-2 flex-wrap">
                  {/* Load JSON — always visible so you can restore a session without scanning */}
                  <button
                    onClick={importJson}
                    disabled={scanState === "scanning"}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-yellow-800/40 text-yellow-600 hover:text-yellow-400 hover:border-yellow-600/50 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                    title="Load a previously exported scan_results.json — skips re-scanning"
                  >
                    <Upload size={13} />
                    Load JSON
                  </button>
              </div>
              {recoveredFiles.length > 0 && (
                <div className="flex items-center gap-2 flex-wrap">
                  {/* PRIMARY: recover all organized into type subfolders */}
                  <button
                    onClick={recoverAllOrganized}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[#00d4ff]/40 bg-[#00d4ff]/8 text-[#00d4ff] hover:bg-[#00d4ff]/15 transition-all"
                    title={
                      selectedIds.size > 0
                        ? `Recover ${selectedIds.size} selected files, sorted into Images/, Documents/, Videos/… subfolders`
                        : "Recover ALL files, automatically sorted into Images/, Documents/, Videos/… subfolders"
                    }
                  >
                    <FolderTree size={13} />
                    {selectedIds.size > 0
                      ? `Recover Selected (${selectedIds.size}) — Organized`
                      : `Recover All (${recoveredFiles.length}) — Organized`}
                  </button>

                  {/* SECONDARY: batch recover selected to flat folder */}
                  <button
                    onClick={recoverBatch}
                    disabled={selectedIds.size === 0}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[#1a1a2e] text-gray-500 hover:text-[#00d4ff] hover:border-[#00d4ff]/30 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                    title="Save selected files to a single folder (no subfolders)"
                  >
                    <FolderOpen size={13} />
                    Batch Save {selectedIds.size > 0 && `(${selectedIds.size})`}
                  </button>

                  <button
                    onClick={exportZip}
                    disabled={recoveredFiles.length === 0}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[#1a1a2e] text-gray-500 hover:text-[#00d4ff] hover:border-[#00d4ff]/30 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                    title="Pack all (or selected) files into a single ZIP archive"
                  >
                    <PackageOpen size={13} />
                    ZIP {selectedIds.size > 0 ? `(${selectedIds.size})` : `(${recoveredFiles.length})`}
                  </button>

                  <div className="flex-1" />

                  <button
                    onClick={exportCsv}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs border border-[#1a1a2e] text-gray-600 hover:text-gray-300 hover:border-gray-600 transition-all"
                    title="Export file list as CSV spreadsheet"
                  >
                    <FileDown size={12} />
                    CSV
                  </button>
                  <button
                    onClick={exportJson}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs border border-[#1a1a2e] text-gray-600 hover:text-gray-300 hover:border-gray-600 transition-all"
                    title="Export scan results as JSON (can be reloaded later with Load JSON)"
                  >
                    <FileDown size={12} />
                    JSON
                  </button>
                </div>
              )}

              {/* Results table */}
              <RecoveryTable
                files={recoveredFiles}
                onRecover={recoverFile}
                onPreview={previewFile}
                loading={scanState === "scanning" && recoveredFiles.length === 0}
                selectedIds={selectedIds}
                onSelectionChange={setSelectedIds}
                hasDisk={!!selectedDisk}
                scanStarted={scanState !== "idle" || recoveredFiles.length > 0}
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
                wipeProgress={wipeProgress}
                wipeState={wipeState}
                wipeError={wipeError}
                onWipeStart={startWipeFreeSpace}
                onWipeCancel={cancelWipeFreeSpace}
                onWipeReset={resetWipe}
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
