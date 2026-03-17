import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { open } from "@tauri-apps/api/dialog";
import { ShieldOff, Zap, AlertTriangle, CheckCircle, FolderOpen, Square } from "lucide-react";
import { ShredProgress, ShredState } from "../types";

interface Props {
  progress: ShredProgress | null;
  state: ShredState;
  error?: string | null;
  selectedDiskPath?: string | null;
  onStart: (path: string, algorithm: string, verify: boolean) => void;
  onCancel: () => void;
  onReset: () => void;
}

const ALGORITHMS = [
  {
    id: "DoD5220",
    name: "DoD 5220.22-M",
    desc: "3-pass US DoD standard",
    passes: 3,
    icon: "🛡️",
    safe: true,
  },
  {
    id: "Gutmann35",
    name: "Gutmann 35-Pass",
    desc: "Maximum security overwrite",
    passes: 35,
    icon: "☢️",
    safe: true,
  },
  {
    id: "RandomSingle",
    name: "Random (1-Pass)",
    desc: "Fast single-pass random",
    passes: 1,
    icon: "⚡",
    safe: false,
  },
  {
    id: "NvmeSanitize",
    name: "NVMe Sanitize",
    desc: "Hardware NAND erase (SSD/M.2 only)",
    passes: 1,
    icon: "💾",
    safe: true,
  },
  {
    id: "NvmeFormat",
    name: "NVMe Format NVM",
    desc: "Controller-level format (SSD/M.2 only)",
    passes: 1,
    icon: "🔥",
    safe: true,
  },
];

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function ShredPanel({ progress, state, error, selectedDiskPath, onStart, onCancel, onReset }: Props) {
  const [selectedAlgo, setSelectedAlgo] = useState("DoD5220");
  const [targetPath, setTargetPath] = useState("");
  const [verify, setVerify] = useState(true);
  const [confirmed, setConfirmed] = useState(false);

  const isActive = state === "shredding";
  const isComplete = state === "complete";
  const isError = state === "error";

  async function handleBrowse() {
    const selected = await open({
      multiple: false,
      title: "Select file or drive to shred",
    });
    if (typeof selected === "string") {
      setTargetPath(selected);
      setConfirmed(false);
    }
  }

  function handleStart() {
    if (!targetPath || !confirmed) return;
    onStart(targetPath, selectedAlgo, verify);
  }

  const pct =
    progress && progress.total_bytes > 0
      ? Math.round((progress.bytes_written / progress.total_bytes) * 100)
      : 0;

  return (
    <div className="space-y-4">
      {/* Algorithm selector */}
      <div>
        <label className="text-xs text-gray-600 uppercase tracking-wider block mb-2">
          Destruction Algorithm
        </label>
        <div className="space-y-1.5">
          {ALGORITHMS.map((algo) => (
            <button
              key={algo.id}
              disabled={isActive}
              onClick={() => {
                setSelectedAlgo(algo.id);
                setConfirmed(false);
              }}
              className={`w-full text-left p-2.5 rounded-lg border transition-all ${
                selectedAlgo === algo.id
                  ? "border-[#00d4ff]/50 bg-[#00d4ff]/5"
                  : "border-[#1a1a2e] hover:border-[#1a1a2e]/80 bg-[#0d0d1a]"
              } disabled:opacity-40`}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="text-base leading-none">{algo.icon}</span>
                  <div>
                    <div className="text-white text-xs font-medium">{algo.name}</div>
                    <div className="text-gray-600 text-[10px]">{algo.desc}</div>
                  </div>
                </div>
                <div className="text-right">
                  <span
                    className={`text-[10px] px-1.5 py-0.5 rounded border ${
                      algo.safe
                        ? "text-[#00d4ff]/60 border-[#00d4ff]/20"
                        : "text-yellow-500/60 border-yellow-500/20"
                    }`}
                  >
                    {algo.passes}P
                  </span>
                </div>
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Target path */}
      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="text-xs text-gray-600 uppercase tracking-wider">
            Target — file path or device path
          </label>
          {selectedDiskPath && !isActive && (
            <button
              onClick={() => { setTargetPath(selectedDiskPath); setConfirmed(false); }}
              className="text-[10px] text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors font-mono"
              title={`Fill with the currently selected disk: ${selectedDiskPath}`}
            >
              Use {selectedDiskPath}
            </button>
          )}
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={targetPath}
            onChange={(e) => { setTargetPath(e.target.value); setConfirmed(false); }}
            placeholder="\\.\C:  or  C:\path\to\file.docx"
            disabled={isActive}
            className="flex-1 bg-[#0d0d1a] border border-[#1a1a2e] rounded px-3 py-2 text-xs text-gray-300 placeholder-gray-700 focus:outline-none focus:border-[#00d4ff]/50 font-mono disabled:opacity-40"
          />
          <button
            onClick={handleBrowse}
            disabled={isActive}
            className="px-2.5 py-2 bg-[#0d0d1a] border border-[#1a1a2e] rounded hover:border-[#00d4ff]/40 text-gray-500 hover:text-[#00d4ff] transition-colors disabled:opacity-40"
            title="Browse for a file"
          >
            <FolderOpen size={14} />
          </button>
        </div>
        <p className="text-[10px] text-gray-700 mt-1">
          Enter a file path to shred a single file, or a device path like{" "}
          <code className="font-mono">\\.\C:</code> or{" "}
          <code className="font-mono">\\.\PhysicalDrive0</code> to wipe an entire drive.
          Requires administrator privileges for device paths.
        </p>
      </div>

      {/* Options */}
      <div className="flex items-center gap-2">
        <button
          onClick={() => setVerify((v) => !v)}
          disabled={isActive}
          className={`w-4 h-4 rounded border flex items-center justify-center transition-colors ${
            verify
              ? "bg-[#00d4ff]/20 border-[#00d4ff]/50"
              : "bg-transparent border-[#1a1a2e]"
          } disabled:opacity-40`}
        >
          {verify && <CheckCircle size={10} className="text-[#00d4ff]" />}
        </button>
        <span className="text-xs text-gray-500">Verify last pass (bit-level read-back)</span>
      </div>

      {/* Confirmation warning */}
      {targetPath && !isActive && !isComplete && (
        <div className="border border-red-900/40 bg-red-900/10 rounded-lg p-3">
          <div className="flex items-start gap-2">
            <AlertTriangle size={14} className="text-red-400 mt-0.5 shrink-0" />
            <div className="text-xs text-red-400/80">
              <strong className="text-red-400 block mb-1">IRREVERSIBLE ACTION</strong>
              All data at <code className="font-mono text-red-300">{targetPath}</code> will
              be permanently destroyed using{" "}
              <strong>{ALGORITHMS.find((a) => a.id === selectedAlgo)?.name}</strong>.
              This cannot be undone.
            </div>
          </div>
          <div className="flex items-center gap-2 mt-2.5">
            <button
              onClick={() => setConfirmed((c) => !c)}
              className={`w-4 h-4 rounded border flex items-center justify-center transition-colors ${
                confirmed
                  ? "bg-red-500/30 border-red-500/60"
                  : "bg-transparent border-red-900/40"
              }`}
            >
              {confirmed && <CheckCircle size={10} className="text-red-400" />}
            </button>
            <span className="text-xs text-red-400/60">
              I confirm this data will be permanently destroyed
            </span>
          </div>
        </div>
      )}

      {/* Progress */}
      {isActive && progress && (
        <div className="space-y-2">
          <div className="flex justify-between text-xs">
            <span className="text-gray-500">
              Pass {progress.current_pass}/{progress.total_passes} —{" "}
              <span className="text-[#00d4ff]">{progress.algorithm}</span>
            </span>
            <span className="text-gray-400 font-mono">{pct}%</span>
          </div>
          <div className="h-2 bg-[#1a1a2e] rounded-full overflow-hidden">
            <div
              className="h-full rounded-full bg-gradient-to-r from-[#00d4ff]/60 to-[#00d4ff] transition-all duration-300"
              style={{ width: `${pct}%` }}
            />
          </div>
          <div className="flex justify-between text-[10px] text-gray-600 font-mono">
            <span>{formatBytes(progress.bytes_written)} written</span>
            <span>{formatBytes(progress.total_bytes)} total</span>
          </div>
        </div>
      )}

      {/* Complete */}
      {isComplete && (
        <div className="border border-green-900/30 bg-green-900/10 rounded-lg px-3 py-2 space-y-1.5">
          <div className="flex items-center gap-2 text-green-400 text-sm">
            <CheckCircle size={14} />
            Destruction complete. Data is unrecoverable.
            {progress?.verification_passed === true && (
              <span className="text-[10px] text-green-400/60 ml-auto">✓ Verified</span>
            )}
          </div>
        </div>
      )}

      {/* Error */}
      {isError && (
        <div className="border border-red-900/40 bg-red-900/10 rounded-lg px-3 py-2 space-y-1">
          <div className="flex items-center gap-2 text-red-400 text-sm">
            <AlertTriangle size={14} />
            Shred operation failed
          </div>
          {error && (
            <p className="text-[10px] text-red-400/70 font-mono break-all">{error}</p>
          )}
          <p className="text-[10px] text-gray-600">
            Common causes: path does not exist, insufficient permissions (run as administrator for device paths), or the drive is in use.
          </p>
        </div>
      )}

      {/* Action buttons */}
      <div className="flex gap-2">
        {isActive ? (
          <button
            onClick={onCancel}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-[#0d0d1a] border border-[#1a1a2e] text-gray-400 text-sm font-medium hover:border-red-700/50 hover:text-red-400 transition-all"
          >
            <Square size={15} />
            Cancel
          </button>
        ) : (isComplete || isError) ? (
          <button
            onClick={onReset}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-[#0d0d1a] border border-[#1a1a2e] text-gray-400 text-sm font-medium hover:border-[#00d4ff]/40 hover:text-[#00d4ff] transition-all"
          >
            New Operation
          </button>
        ) : (
          <button
            onClick={handleStart}
            disabled={!targetPath || !confirmed}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-gradient-to-r from-red-900/40 to-red-800/40 border border-red-700/50 text-red-400 text-sm font-medium hover:from-red-800/50 hover:to-red-700/50 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <ShieldOff size={15} />
            Execute Destruction
          </button>
        )}
      </div>
    </div>
  );
}
