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
  Image,
  BookOpen,
  HardDrive,
} from "lucide-react";
import { RecoveredFile } from "../types";

// ── Type grouping ─────────────────────────────────────────────────────────────

const TYPE_GROUPS: Record<string, string> = {
  JPEG: "images", PNG: "images", GIF: "images", TIFF: "images", BMP: "images",
  PDF: "documents", DOCX: "documents", DOC: "documents",
  XLSX: "documents", PPTX: "documents", TXT: "documents",
  MP4: "videos", AVI: "videos", MKV: "videos",
  MP3: "audio", WAV: "audio", FLAC: "audio",
  ZIP: "archives", RAR: "archives", SevenZ: "archives",
};

function getGroup(type: string): string {
  return TYPE_GROUPS[type] ?? "other";
}

interface GroupDef {
  id: string;
  label: string;
  icon: React.ReactNode;
  color: string;
  activeClass: string;
}

const GROUPS: GroupDef[] = [
  {
    id: "all", label: "All", icon: <HardDrive size={12} />,
    color: "text-gray-400", activeClass: "border-[#00d4ff]/50 bg-[#00d4ff]/10 text-[#00d4ff]",
  },
  {
    id: "images", label: "Images", icon: <Image size={12} />,
    color: "text-pink-400", activeClass: "border-pink-500/50 bg-pink-500/10 text-pink-400",
  },
  {
    id: "documents", label: "Documents", icon: <BookOpen size={12} />,
    color: "text-orange-400", activeClass: "border-orange-500/50 bg-orange-500/10 text-orange-400",
  },
  {
    id: "videos", label: "Videos", icon: <Video size={12} />,
    color: "text-purple-400", activeClass: "border-purple-500/50 bg-purple-500/10 text-purple-400",
  },
  {
    id: "audio", label: "Audio", icon: <Music size={12} />,
    color: "text-green-400", activeClass: "border-green-500/50 bg-green-500/10 text-green-400",
  },
  {
    id: "archives", label: "Archives", icon: <Archive size={12} />,
    color: "text-yellow-400", activeClass: "border-yellow-500/50 bg-yellow-500/10 text-yellow-400",
  },
  {
    id: "other", label: "Other", icon: <File size={12} />,
    color: "text-gray-500", activeClass: "border-gray-500/50 bg-gray-500/10 text-gray-400",
  },
];

// ── Sub-components ─────────────────────────────────────────────────────────────

function FileIcon({ type }: { type: string }) {
  const cls = "shrink-0";
  switch (getGroup(type)) {
    case "images":    return <FileImage size={15} className={`${cls} text-pink-400`} />;
    case "documents": return <FileText  size={15} className={`${cls} text-orange-400`} />;
    case "videos":    return <Video     size={15} className={`${cls} text-purple-400`} />;
    case "audio":     return <Music     size={15} className={`${cls} text-green-400`} />;
    case "archives":  return <Archive   size={15} className={`${cls} text-yellow-400`} />;
    default:          return <File      size={15} className={`${cls} text-gray-500`} />;
  }
}

function ProbabilityBadge({ prob }: { prob: number }) {
  const pct = Math.round(prob * 100);
  const color =
    pct >= 75 ? "text-[#00d4ff] border-[#00d4ff]/30 bg-[#00d4ff]/5" :
    pct >= 40 ? "text-yellow-400 border-yellow-400/30 bg-yellow-400/5" :
                "text-red-400 border-red-400/30 bg-red-400/5";
  const label = pct >= 75 ? "High" : pct >= 40 ? "Med" : "Low";
  return (
    <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded border ${color}`}
      title={`${pct}% recovery confidence`}>
      {pct}% {label}
    </span>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

// ── Props ─────────────────────────────────────────────────────────────────────

interface Props {
  files: RecoveredFile[];
  onRecover: (file: RecoveredFile) => void;
  onPreview: (file: RecoveredFile) => void;
  loading: boolean;
  selectedIds: Set<number>;
  onSelectionChange: (ids: Set<number>) => void;
}

type SortKey = "file_type" | "size_bytes" | "recovery_probability";

// ── Main component ─────────────────────────────────────────────────────────────

export function RecoveryTable({ files, onRecover, onPreview, loading, selectedIds, onSelectionChange }: Props) {
  const { t } = useTranslation();
  const [sortKey, setSortKey] = useState<SortKey>("recovery_probability");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("desc");
  const [textFilter, setTextFilter] = useState("");
  const [groupFilter, setGroupFilter] = useState("all");

  // Count per group
  const groupCounts = files.reduce<Record<string, number>>((acc, f) => {
    const g = getGroup(f.file_type);
    acc[g] = (acc[g] ?? 0) + 1;
    return acc;
  }, {});

  function toggleOne(id: number) {
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id); else next.add(id);
    onSelectionChange(next);
  }

  function toggleAll() {
    if (selectedIds.size === sorted.length) {
      onSelectionChange(new Set());
    } else {
      onSelectionChange(new Set(sorted.map((f) => Number(f.id))));
    }
  }

  /** Select all files in the current group filter */
  function selectCurrentGroup() {
    const ids = new Set(sorted.map((f) => Number(f.id)));
    onSelectionChange(ids);
  }

  const handleSort = (key: SortKey) => {
    if (sortKey === key) setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    else { setSortKey(key); setSortDir("desc"); }
  };

  const filtered = files.filter((f) => {
    const matchGroup = groupFilter === "all" || getGroup(f.file_type) === groupFilter;
    const matchText =
      textFilter === "" ||
      f.file_type.toLowerCase().includes(textFilter.toLowerCase()) ||
      (f.original_name ?? "").toLowerCase().includes(textFilter.toLowerCase());
    return matchGroup && matchText;
  });

  const sorted = [...filtered].sort((a, b) => {
    let cmp = 0;
    if (sortKey === "file_type") cmp = a.file_type.localeCompare(b.file_type);
    else if (sortKey === "size_bytes") cmp = a.size_bytes - b.size_bytes;
    else cmp = a.recovery_probability - b.recovery_probability;
    return sortDir === "asc" ? cmp : -cmp;
  });

  const colHeader = (label: string, key: SortKey) => (
    <th
      className="px-3 py-2 text-left text-[10px] font-semibold text-gray-600 uppercase tracking-wider cursor-pointer select-none hover:text-[#00d4ff] transition-colors"
      onClick={() => handleSort(key)}
    >
      {label}
      {sortKey === key && (
        <span className="ml-1 text-[#00d4ff]">{sortDir === "asc" ? "↑" : "↓"}</span>
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
      <div className="flex flex-col items-center justify-center py-16 text-gray-700 gap-2">
        <File size={32} className="opacity-20" />
        <span className="text-sm">{t("recovery.empty")}</span>
        <span className="text-[11px] text-gray-700 text-center max-w-sm leading-relaxed">
          Select a device from the sidebar, choose a scan profile and hit{" "}
          <span className="text-[#00d4ff]/60">Start Scan</span>. Results appear here in real time.
        </span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">

      {/* ── Type filter chips ─────────────────────────────────────────── */}
      <div className="flex flex-wrap gap-1.5 items-center">
        {GROUPS.map((g) => {
          const count = g.id === "all" ? files.length : (groupCounts[g.id] ?? 0);
          if (count === 0 && g.id !== "all") return null;
          const active = groupFilter === g.id;
          return (
            <button
              key={g.id}
              onClick={() => setGroupFilter(g.id)}
              title={`Show only ${g.label}`}
              className={`flex items-center gap-1 px-2.5 py-1 rounded-full border text-[11px] font-medium transition-all ${
                active ? g.activeClass : `border-[#1a1a2e] ${g.color}/60 hover:${g.color} hover:border-current/30`
              }`}
            >
              {g.icon}
              {g.label}
              <span className={`font-mono text-[10px] ${active ? "opacity-80" : "opacity-50"}`}>
                {count}
              </span>
            </button>
          );
        })}
        {groupFilter !== "all" && (
          <button
            onClick={selectCurrentGroup}
            className="ml-auto text-[10px] text-[#00d4ff]/50 hover:text-[#00d4ff] transition-colors"
            title="Select all visible files"
          >
            Select all {sorted.length}
          </button>
        )}
      </div>

      {/* ── Text search + count ───────────────────────────────────────── */}
      <div className="flex items-center gap-2">
        <input
          type="text"
          placeholder={t("recovery.filterPlaceholder")}
          value={textFilter}
          onChange={(e) => setTextFilter(e.target.value)}
          className="flex-1 bg-[#0d0d1a] border border-[#1a1a2e] rounded px-3 py-1.5 text-xs text-gray-300 placeholder-gray-700 focus:outline-none focus:border-[#00d4ff]/50"
        />
        <span className="text-xs text-gray-600 shrink-0">
          {t("recovery.count", { shown: sorted.length, total: files.length })}
        </span>
      </div>

      {/* ── Table ────────────────────────────────────────────────────── */}
      <div className="border border-[#1a1a2e] rounded-lg overflow-hidden">
        <table className="w-full text-xs">
          <thead className="bg-[#08080f] border-b border-[#1a1a2e]">
            <tr>
              <th className="px-2 py-2 w-7">
                <input
                  type="checkbox"
                  className="accent-[#00d4ff] cursor-pointer"
                  checked={sorted.length > 0 && selectedIds.size === sorted.length}
                  onChange={toggleAll}
                  title="Select / deselect all visible"
                />
              </th>
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
                className={`hover:bg-[#00d4ff]/3 transition-colors group cursor-pointer ${
                  selectedIds.has(Number(file.id)) ? "bg-[#00d4ff]/5" : ""
                }`}
                onClick={() => toggleOne(Number(file.id))}
              >
                <td className="px-2 py-2">
                  <input
                    type="checkbox"
                    className="accent-[#00d4ff] cursor-pointer"
                    checked={selectedIds.has(Number(file.id))}
                    onChange={() => toggleOne(Number(file.id))}
                    onClick={(e) => e.stopPropagation()}
                  />
                </td>
                <td className="px-3 py-2 text-gray-700 font-mono">{file.id}</td>
                <td className="px-3 py-2">
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-1.5">
                      <FileIcon type={file.file_type} />
                      <span className="text-white font-medium">{file.file_type}</span>
                    </div>
                    {file.original_name && (
                      <span
                        className="text-[10px] text-[#00d4ff]/60 font-mono truncate max-w-[150px]"
                        title={file.original_name}
                      >
                        {file.original_name}
                      </span>
                    )}
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
                        title={`Fragmented file (${file.fragment_count} fragments) — may have gaps`}
                        className="text-yellow-500/80"
                      >
                        <Layers size={12} />
                      </span>
                    )}
                    {file.sector_overwritten ? (
                      <span title="Sector may be partially overwritten — recovery might be incomplete" className="text-red-500/80">
                        <AlertCircle size={12} />
                      </span>
                    ) : (
                      <span title="Sector intact — high chance of full recovery" className="text-[#00d4ff]/50">
                        <CheckCircle size={12} />
                      </span>
                    )}
                  </div>
                </td>
                <td className="px-3 py-2" onClick={(e) => e.stopPropagation()}>
                  <div className="flex justify-end gap-1">
                    {file.preview_available && (
                      <button
                        onClick={() => onPreview(file)}
                        className="p-1 rounded hover:bg-[#00d4ff]/10 text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors"
                        title="Preview file contents"
                      >
                        <Eye size={13} />
                      </button>
                    )}
                    <button
                      onClick={() => onRecover(file)}
                      className="p-1 rounded hover:bg-[#00d4ff]/10 text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors"
                      title="Save file to disk"
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
