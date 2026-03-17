import React from "react";
import { useTranslation } from "react-i18next";
import { HardDrive, Zap, Thermometer } from "lucide-react";
import { DiskInfo } from "../types";

interface Props {
  disks: DiskInfo[];
  selected: string | null;
  onSelect: (path: string) => void;
  loading: boolean;
}

function UsageBar({ pct }: { pct: number }) {
  const color =
    pct >= 80 ? "#ef4444" : pct >= 60 ? "#facc15" : "#00d4ff";
  return (
    <div className="w-full h-1.5 bg-[#1a1a2e] rounded-full overflow-hidden mt-1.5">
      <div
        className="h-full rounded-full transition-all duration-500"
        style={{ width: `${pct}%`, backgroundColor: color }}
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
  const { t } = useTranslation();

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-[#00d4ff]/60 text-sm py-4">
        <div className="w-4 h-4 border border-[#00d4ff]/40 border-t-[#00d4ff] rounded-full animate-spin" />
        {t("disk.enumerating")}
      </div>
    );
  }

  if (disks.length === 0) {
    return (
      <div className="text-gray-600 text-xs py-4 leading-relaxed">
        {t("disk.noDevices")}
        <br />
        {t("disk.noDevicesHint")}
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {disks.map((disk) => {
        const isSelected = selected === disk.device_path;
        const usedPct =
          disk.total_bytes > 0
            ? Math.round((disk.used_bytes / disk.total_bytes) * 100)
            : 0;

        return (
          <button
            key={disk.device_path}
            onClick={() => onSelect(disk.device_path)}
            title={`${disk.device_path}\nClick to select`}
            className={`w-full text-left p-3 rounded-lg border transition-all duration-200 ${
              isSelected
                ? "border-[#00d4ff] bg-[#00d4ff]/5 shadow-[0_0_12px_rgba(0,212,255,0.15)]"
                : "border-[#1a1a2e] bg-[#0d0d1a] hover:border-[#00d4ff]/40"
            }`}
          >
            {/* Row 1: Icon + Name + Temp */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 min-w-0">
                {disk.is_ssd ? (
                  <Zap size={14} className="text-[#00d4ff] shrink-0" />
                ) : (
                  <HardDrive size={14} className="text-gray-400 shrink-0" />
                )}
                <span className="text-white text-sm font-semibold truncate">
                  {disk.display_name}
                </span>
              </div>
              {disk.smart_health.temperature_celsius != null && (
                <span
                  className="flex items-center gap-0.5 text-[10px] text-gray-500 shrink-0 ml-2"
                  title={t("disk.temperature")}
                >
                  <Thermometer size={9} />
                  {disk.smart_health.temperature_celsius}°C
                </span>
              )}
            </div>

            {/* Row 2: Capacity + FS + device path */}
            <div className="flex items-center justify-between mt-1.5 text-[10px] text-gray-600">
              <span>{formatBytes(disk.total_bytes)} · {disk.file_system}</span>
              <span className="font-mono text-gray-700">{disk.device_path}</span>
            </div>

            {/* Usage bar */}
            <UsageBar pct={usedPct} />

            {/* Row 3: Usage label */}
            <div className="flex justify-between mt-1 text-[9px] text-gray-700">
              <span>{t("disk.usage")}</span>
              <span
                className={
                  usedPct >= 80
                    ? "text-red-400"
                    : usedPct >= 60
                    ? "text-yellow-400"
                    : "text-[#00d4ff]/60"
                }
              >
                {t("disk.used", { pct: usedPct })}
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}
