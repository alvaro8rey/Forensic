import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FileImage,
  FileText,
  File,
  Archive,
  Music,
  Video,
  Download,
  Eye,
  AlertCircle,
  CheckCircle,
  Layers,
} from "lucide-react";
import { RecoveredFile } from "../types";

interface Props {
  files: RecoveredFile[];
  onRecover: (file: RecoveredFile) => void;
  onPreview: (file: RecoveredFile) => void;
  loading: boolean;
}

function FileIcon({ type }: { type: string }) {
  const cls = "shrink-0";
  switch (type) {
    case "JPEG":
    case "PNG":
    case "GIF":
    case "TIFF":
    case "BMP":
      return <FileImage size={15} className={`${cls} text-pink-400`} />;
    case "PDF":
      return <FileText size={15} className={`${cls} text-orange-400`} />;
    case "TXT":
      return <FileText size={15} className={`${cls} text-gray-300`} />;
    case "MP4":
    case "AVI":
    case "MKV":
      return <Video size={15} className={`${cls} text-purple-400`} />;
    case "MP3":
    case "WAV":
    case "FLAC":
      return <Music size={15} className={`${cls} text-green-400`} />;
    case "ZIP":
    case "DOCX":
    case "DOC":
    case "RAR":
    case "7Z":
      return <Archive size={15} className={`${cls} text-yellow-400`} />;
    case "SQLite":
      return <FileText size={15} className={`${cls} text-blue-400`} />;
    default:
      return <File size={15} className={`${cls} text-gray-500`} />;
  }
}

function ProbabilityBadge({ prob }: { prob: number }) {
  const pct = Math.round(prob * 100);
  const color =
    pct >= 75
      ? "text-[#00d4ff] border-[#00d4ff]/30 bg-[#00d4ff]/5"
      : pct >= 40
      ? "text-yellow-400 border-yellow-400/30 bg-yellow-400/5"
      : "text-red-400 border-red-400/30 bg-red-400/5";

  return (
    <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded border ${color}`}>
      {pct}%
    </span>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

type SortKey = "file_type" | "size_bytes" | "recovery_probability";

export function RecoveryTable({ files, onRecover, onPreview, loading }: Props) {
  const { t } = useTranslation();
  const [sortKey, setSortKey] = useState<SortKey>("recovery_probability");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("desc");
  const [filter, setFilter] = useState<string>("");

  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("desc");
    }
  };

  const filtered = files.filter(
    (f) =>
      filter === "" ||
      f.file_type.toLowerCase().includes(filter.toLowerCase())
  );

  const sorted = [...filtered].sort((a, b) => {
    let cmp = 0;
    if (sortKey === "file_type") {
      cmp = a.file_type.localeCompare(b.file_type);
    } else if (sortKey === "size_bytes") {
      cmp = a.size_bytes - b.size_bytes;
    } else {
      cmp = a.recovery_probability - b.recovery_probability;
    }
    return sortDir === "asc" ? cmp : -cmp;
  });

  const colHeader = (label: string, key: SortKey) => (
    <th
      className="px-3 py-2 text-left text-[10px] font-semibold text-gray-600 uppercase tracking-wider cursor-pointer select-none hover:text-[#00d4ff] transition-colors"
      onClick={() => handleSort(key)}
    >
      {label}
      {sortKey === key && (
        <span className="ml-1 text-[#00d4ff]">
          {sortDir === "asc" ? "↑" : "↓"}
        </span>
      )}
    </th>
  );

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-700">
        <div className="w-8 h-8 border-2 border-[#00d4ff]/30 border-t-[#00d4ff] rounded-full animate-spin mb-3" />
        <span className="text-sm">{t("recovery.loading")}</span>
      </div>
    );
  }

  if (files.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-700">
        <File size={32} className="mb-3 opacity-20" />
        <span className="text-sm">{t("recovery.empty")}</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {/* Filter bar */}
      <div className="flex items-center gap-2">
        <input
          type="text"
          placeholder={t("recovery.filterPlaceholder")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="flex-1 bg-[#0d0d1a] border border-[#1a1a2e] rounded px-3 py-1.5 text-xs text-gray-300 placeholder-gray-700 focus:outline-none focus:border-[#00d4ff]/50"
        />
        <span className="text-xs text-gray-600 shrink-0">
          {t("recovery.count", { shown: sorted.length, total: files.length })}
        </span>
      </div>

      {/* Table */}
      <div className="border border-[#1a1a2e] rounded-lg overflow-hidden">
        <table className="w-full text-xs">
          <thead className="bg-[#08080f] border-b border-[#1a1a2e]">
            <tr>
              <th className="px-3 py-2 text-left text-[10px] font-semibold text-gray-600 uppercase tracking-wider w-8">
                {t("recovery.columns.id")}
              </th>
              {colHeader(t("recovery.columns.type"), "file_type")}
              {colHeader(t("recovery.columns.size"), "size_bytes")}
              <th className="px-3 py-2 text-left text-[10px] font-semibold text-gray-600 uppercase tracking-wider">
                {t("recovery.columns.offset")}
              </th>
              {colHeader(t("recovery.columns.recovery"), "recovery_probability")}
              <th className="px-3 py-2 text-left text-[10px] font-semibold text-gray-600 uppercase tracking-wider">
                {t("recovery.columns.flags")}
              </th>
              <th className="px-3 py-2 text-right text-[10px] font-semibold text-gray-600 uppercase tracking-wider">
                {t("recovery.columns.actions")}
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[#1a1a2e]">
            {sorted.map((file) => (
              <tr
                key={file.id}
                className="hover:bg-[#00d4ff]/3 transition-colors group"
              >
                <td className="px-3 py-2 text-gray-700 font-mono">{file.id}</td>
                <td className="px-3 py-2">
                  <div className="flex items-center gap-1.5">
                    <FileIcon type={file.file_type} />
                    <span className="text-white font-medium">{file.file_type}</span>
                  </div>
                </td>
                <td className="px-3 py-2 text-gray-400 font-mono">
                  {formatBytes(file.size_bytes)}
                </td>
                <td className="px-3 py-2 font-mono text-[#00d4ff]/60 text-[10px]">
                  0x{file.offset_start.toString(16).toUpperCase().padStart(12, "0")}
                </td>
                <td className="px-3 py-2">
                  <ProbabilityBadge prob={file.recovery_probability} />
                </td>
                <td className="px-3 py-2">
                  <div className="flex gap-1">
                    {file.is_fragmented && (
                      <span
                        title={t("recovery.fragments", { count: file.fragment_count })}
                        className="text-yellow-500/80"
                      >
                        <Layers size={12} />
                      </span>
                    )}
                    {file.sector_overwritten ? (
                      <span title={t("recovery.overwritten")} className="text-red-500/80">
                        <AlertCircle size={12} />
                      </span>
                    ) : (
                      <span title={t("recovery.intact")} className="text-[#00d4ff]/50">
                        <CheckCircle size={12} />
                      </span>
                    )}
                  </div>
                </td>
                <td className="px-3 py-2">
                  <div className="flex justify-end gap-1">
                    {file.preview_available && (
                      <button
                        onClick={() => onPreview(file)}
                        className="p-1 rounded hover:bg-[#00d4ff]/10 text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors"
                        title={t("recovery.preview")}
                      >
                        <Eye size={13} />
                      </button>
                    )}
                    <button
                      onClick={() => onRecover(file)}
                      className="p-1 rounded hover:bg-[#00d4ff]/10 text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors"
                      title={t("recovery.recoverFile")}
                    >
                      <Download size={13} />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
