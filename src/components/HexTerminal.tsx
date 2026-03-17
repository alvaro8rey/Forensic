import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Terminal } from "lucide-react";
import { ScanProgress } from "../types";

interface LogEntry {
  timestamp: string;
  offset: string;
  message: string;
  type: "info" | "found" | "warn" | "error";
}

interface Props {
  logs: LogEntry[];
  progress: ScanProgress | null;
  isScanning: boolean;
}

/**
 * Builds a log entry for a scan-progress tick.
 * Accepts a translation function so messages respect the active language.
 */
export function buildLogEntry(
  progress: ScanProgress,
  prevFiles: number,
  t: (key: string, opts?: Record<string, unknown>) => string
): LogEntry | null {
  const now = new Date().toTimeString().slice(0, 8);
  if (progress.files_found > prevFiles) {
    return {
      timestamp: now,
      offset: progress.current_offset_hex,
      message: t("terminal.signatureMatch", { count: progress.files_found }),
      type: "found",
    };
  }
  return {
    timestamp: now,
    offset: progress.current_offset_hex,
    message: t("terminal.scanning", {
      speed: progress.scan_speed_mb.toFixed(1),
    }),
    type: "info",
  };
}

export function HexTerminal({ logs, progress, isScanning }: Props) {
  const { t } = useTranslation();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  return (
    <div className="bg-[#050508] border border-[#1a1a2e] rounded-lg overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[#1a1a2e] bg-[#08080f]">
        <div className="flex items-center gap-2">
          <Terminal size={13} className="text-[#00d4ff]" />
          <span className="text-xs text-[#00d4ff] font-mono uppercase tracking-widest">
            {t("terminal.title")}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          {isScanning && (
            <>
              <span className="w-1.5 h-1.5 rounded-full bg-[#00d4ff] animate-pulse" />
              <span className="text-[10px] text-[#00d4ff]/60 font-mono">
                {t("terminal.live")}
              </span>
            </>
          )}
        </div>
      </div>

      {/* Terminal body */}
      <div className="h-48 overflow-y-auto p-3 font-mono text-[11px] space-y-0.5 scrollbar-thin scrollbar-track-transparent scrollbar-thumb-[#1a1a2e]">
        {logs.length === 0 ? (
          <div className="text-gray-700 select-none">
            {t("terminal.awaiting")}
          </div>
        ) : (
          logs.map((entry, i) => (
            <div key={i} className="flex gap-3 leading-5">
              <span className="text-gray-700 shrink-0">{entry.timestamp}</span>
              <span className="text-[#00d4ff]/50 shrink-0 w-[130px]">
                {entry.offset}
              </span>
              <span
                className={
                  entry.type === "found"
                    ? "text-[#00d4ff] font-semibold"
                    : entry.type === "warn"
                    ? "text-yellow-400"
                    : entry.type === "error"
                    ? "text-red-400"
                    : "text-gray-500"
                }
              >
                {entry.message}
              </span>
            </div>
          ))
        )}
        {isScanning && (
          <div className="flex gap-3 leading-5">
            <span className="text-gray-700 shrink-0">
              {new Date().toTimeString().slice(0, 8)}
            </span>
            <span className="text-[#00d4ff]/50 shrink-0 w-[130px]">
              {progress?.current_offset_hex ?? "0x0000000000000000"}
            </span>
            <span className="text-gray-600">
              <span className="inline-block w-2 h-3 bg-[#00d4ff]/60 animate-pulse align-middle" />
            </span>
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      {/* Status bar */}
      {progress && (
        <div className="border-t border-[#1a1a2e] px-3 py-1.5 grid grid-cols-4 gap-2 text-[10px] font-mono">
          <div>
            <span className="text-gray-700">{t("terminal.offset")} </span>
            <span className="text-[#00d4ff]/80">{progress.current_offset_hex}</span>
          </div>
          <div>
            <span className="text-gray-700">{t("terminal.speed")} </span>
            <span className="text-green-400">{progress.scan_speed_mb.toFixed(1)} MB/s</span>
          </div>
          <div>
            <span className="text-gray-700">{t("terminal.files")} </span>
            <span className="text-[#00d4ff]">{progress.files_found}</span>
          </div>
          <div>
            <span className="text-gray-700">{t("terminal.time")} </span>
            <span className="text-gray-400">{progress.elapsed_seconds}s</span>
          </div>
        </div>
      )}
    </div>
  );
}
