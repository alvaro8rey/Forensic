import React from "react";
import {
  HardDrive,
  Zap,
  Thermometer,
  Activity,
  CheckCircle,
  AlertTriangle,
  XCircle,
} from "lucide-react";
import { DiskInfo } from "../types";

interface Props {
  disks: DiskInfo[];
  selected: string | null;
  onSelect: (path: string) => void;
  loading: boolean;
}

function HealthIcon({ status }: { status: string }) {
  switch (status) {
    case "Healthy":
      return <CheckCircle size={14} className="text-[#00d4ff]" />;
    case "Warning":
      return <AlertTriangle size={14} className="text-yellow-400" />;
    case "Critical":
      return <XCircle size={14} className="text-red-500" />;
    default:
      return <Activity size={14} className="text-gray-500" />;
  }
}

function HealthBar({ score }: { score: number }) {
  const color =
    score >= 80
      ? "#00d4ff"
      : score >= 50
      ? "#facc15"
      : "#ef4444";

  return (
    <div className="w-full h-1.5 bg-[#1a1a2e] rounded-full overflow-hidden mt-1">
      <div
        className="h-full rounded-full transition-all duration-500"
        style={{ width: `${score}%`, backgroundColor: color }}
      />
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${bytes} B`;
}

export function DiskSelector({ disks, selected, onSelect, loading }: Props) {
  if (loading) {
    return (
      <div className="flex items-center gap-2 text-[#00d4ff]/60 text-sm py-4">
        <div className="w-4 h-4 border border-[#00d4ff]/40 border-t-[#00d4ff] rounded-full animate-spin" />
        Enumerating devices...
      </div>
    );
  }

  if (disks.length === 0) {
    return (
      <div className="text-gray-600 text-xs py-4 leading-relaxed">
        No devices detected.
        <br />
        Click the refresh button above to enumerate storage devices.
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <p className="text-[10px] text-gray-700 leading-relaxed">
        Select a device to use it as target for scanning or shredding.
      </p>
      {disks.map((disk) => {
        const isSelected = selected === disk.device_path;
        const usedPct =
          disk.total_bytes > 0
            ? Math.round((disk.used_bytes / disk.total_bytes) * 100)
            : 0;
        const isPhysical = disk.file_system === "RAW";

        return (
          <button
            key={disk.device_path}
            onClick={() => onSelect(disk.device_path)}
            title={`Device path: ${disk.device_path}\nClick to select for scanning or shredding`}
            className={`w-full text-left p-3 rounded-lg border transition-all duration-200 ${
              isSelected
                ? "border-[#00d4ff] bg-[#00d4ff]/5 shadow-[0_0_12px_rgba(0,212,255,0.15)]"
                : "border-[#1a1a2e] bg-[#0d0d1a] hover:border-[#00d4ff]/40"
            }`}
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                {disk.is_ssd ? (
                  <Zap size={16} className="text-[#00d4ff]" title="Solid-State Drive (SSD)" />
                ) : (
                  <HardDrive size={16} className="text-gray-400" title="Hard Disk Drive (HDD)" />
                )}
                <span className="text-white text-sm font-medium truncate max-w-[120px]">
                  {disk.display_name}
                </span>
              </div>
              <div className="flex items-center gap-1">
                <HealthIcon status={disk.smart_health.overall_health} />
                <span
                  className="text-xs text-gray-500"
                  title={`S.M.A.R.T. health score: ${disk.smart_health.health_score}/100\n(based on available space and disk diagnostics)`}
                >
                  {disk.smart_health.health_score}%
                </span>
              </div>
            </div>

            {/* Device path badge */}
            <div className="mt-1.5 mb-1">
              <span className="font-mono text-[9px] text-gray-700 bg-[#111] px-1.5 py-0.5 rounded border border-[#1a1a2e]">
                {disk.device_path}
              </span>
              {isPhysical && (
                <span className="ml-1 text-[9px] text-yellow-600 border border-yellow-900/40 px-1 py-0.5 rounded">
                  RAW DISK
                </span>
              )}
            </div>

            <div className="mt-1 grid grid-cols-3 gap-2 text-xs text-gray-500">
              <span title="Total storage capacity">{formatBytes(disk.total_bytes)}</span>
              <span className="text-center" title="File system format">{disk.file_system}</span>
              {disk.smart_health.temperature_celsius != null ? (
                <span className="flex items-center justify-end gap-0.5" title="Drive temperature">
                  <Thermometer size={10} />
                  {disk.smart_health.temperature_celsius}°C
                </span>
              ) : (
                <span className="text-right">—</span>
              )}
            </div>

            <HealthBar score={disk.smart_health.health_score} />

            <div className="flex justify-between mt-1 text-[10px] text-gray-600">
              <span title="Self-Monitoring, Analysis and Reporting Technology — disk reliability indicator">
                S.M.A.R.T. Health
              </span>
              <span title={`${formatBytes(disk.used_bytes)} used out of ${formatBytes(disk.total_bytes)}`}>
                Used: {usedPct}%
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}
