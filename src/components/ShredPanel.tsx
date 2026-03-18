import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/api/dialog";
import { ShieldOff, AlertTriangle, CheckCircle, FolderOpen, Square, Wind } from "lucide-react";
import { ShredProgress, ShredState, WipeState } from "../types";

interface Props {
  progress: ShredProgress | null;
  state: ShredState;
  error?: string | null;
  selectedDiskPath?: string | null;
  onStart: (path: string, algorithm: string, verify: boolean) => void;
  onCancel: () => void;
  onReset: () => void;
  // Free space wiper
  wipeProgress: ShredProgress | null;
  wipeState: WipeState;
  wipeError?: string | null;
  onWipeStart: (dirPath: string) => void;
  onWipeCancel: () => void;
  onWipeReset: () => void;
}

/** Non-translatable metadata per algorithm */
const ALGO_META: Record<
  string,
  { icon: string; passes: number; safe: boolean; recommendationColor: string }
> = {
  DoD5220:     { icon: "🛡️", passes: 3,  safe: true,  recommendationColor: "text-green-400" },
  Gutmann35:   { icon: "☢️", passes: 35, safe: true,  recommendationColor: "text-yellow-400" },
  RandomSingle:{ icon: "⚡", passes: 1,  safe: false, recommendationColor: "text-[#00d4ff]" },
  NvmeSanitize:{ icon: "💾", passes: 1,  safe: true,  recommendationColor: "text-purple-400" },
  NvmeFormat:  { icon: "🔥", passes: 1,  safe: true,  recommendationColor: "text-orange-400" },
};

const ALGO_IDS = Object.keys(ALGO_META);

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function ShredPanel({ progress, state, error, selectedDiskPath, onStart, onCancel, onReset, wipeProgress, wipeState, wipeError, onWipeStart, onWipeCancel, onWipeReset }: Props) {
  const { t } = useTranslation();
  const [selectedAlgo, setSelectedAlgo] = useState("DoD5220");
  const [targetPath, setTargetPath] = useState("");
  const [verify, setVerify] = useState(true);
  const [confirmed, setConfirmed] = useState(false);

  const isActive = state === "shredding";
  const isComplete = state === "complete";
  const isError = state === "error";

  const [wipeDirPath, setWipeDirPath] = useState("");
  const [wipeConfirmed, setWipeConfirmed] = useState(false);
  const isWiping = wipeState === "wiping";
  const isWipeComplete = wipeState === "complete";
  const isWipeError = wipeState === "error";

  const wipePct =
    wipeProgress && wipeProgress.total_bytes > 0
      ? Math.round((wipeProgress.bytes_written / wipeProgress.total_bytes) * 100)
      : null;

  async function handleWipeBrowse() {
    const selected = await open({ directory: true, multiple: false, title: t("wipe.browseTitle") });
    if (typeof selected === "string") {
      setWipeDirPath(selected);
      setWipeConfirmed(false);
    }
  }

  async function handleBrowse() {
    const selected = await open({
      multiple: false,
      title: t("shred.browseTitle"),
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
          {t("shred.algorithmLabel")}
        </label>
        <div className="space-y-1">
          {ALGO_IDS.map((id) => {
            const meta = ALGO_META[id];
            const isSelected = selectedAlgo === id;
            return (
              <button
                key={id}
                disabled={isActive}
                onClick={() => { setSelectedAlgo(id); setConfirmed(false); }}
                className={`w-full text-left rounded-lg border transition-all duration-150 ${
                  isSelected
                    ? "border-[#00d4ff]/40 bg-[#00d4ff]/5"
                    : "border-[#1a1a2e] bg-[#0d0d1a] hover:border-[#2a2a3e]"
                } disabled:opacity-40`}
              >
                {/* Compact always-visible row */}
                <div className="flex items-center gap-2 px-2.5 py-2">
                  <span className="text-sm leading-none shrink-0">{meta.icon}</span>
                  <span className={`text-xs font-medium shrink-0 ${isSelected ? "text-white" : "text-gray-400"}`}>
                    {t(`shred.algorithms.${id}.name`)}
                  </span>
                  <span className={`text-[9px] font-medium truncate ${meta.recommendationColor}`}>
                    {t(`shred.algorithms.${id}.recommendation`)}
                  </span>
                  <span
                    className={`ml-auto shrink-0 text-[9px] px-1.5 py-0.5 rounded border ${
                      meta.safe
                        ? "text-[#00d4ff]/50 border-[#00d4ff]/20"
                        : "text-yellow-500/50 border-yellow-500/20"
                    }`}
                  >
                    {meta.passes}P
                  </span>
                </div>

                {/* Expanded detail — only for selected card */}
                {isSelected && (
                  <div className="px-2.5 pb-2.5 border-t border-[#00d4ff]/10">
                    <p className={`text-[9px] font-semibold mt-2 mb-1 ${meta.recommendationColor} opacity-80`}>
                      {t(`shred.algorithms.${id}.forWhom`)}
                    </p>
                    <p className="text-[10px] text-gray-500 leading-relaxed">
                      {t(`shred.algorithms.${id}.desc`)}
                    </p>
                  </div>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Target path */}
      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="text-xs text-gray-600 uppercase tracking-wider">
            {t("shred.targetLabel")}
          </label>
          {selectedDiskPath && !isActive && (
            <button
              onClick={() => { setTargetPath(selectedDiskPath); setConfirmed(false); }}
              className="text-[10px] text-[#00d4ff]/60 hover:text-[#00d4ff] transition-colors font-mono"
              title={t("shred.useDiskTitle", { path: selectedDiskPath })}
            >
              {t("shred.useDisk", { path: selectedDiskPath })}
            </button>
          )}
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={targetPath}
            onChange={(e) => { setTargetPath(e.target.value); setConfirmed(false); }}
            placeholder={t("shred.placeholder")}
            disabled={isActive}
            className="flex-1 bg-[#0d0d1a] border border-[#1a1a2e] rounded px-3 py-2 text-xs text-gray-300 placeholder-gray-700 focus:outline-none focus:border-[#00d4ff]/50 font-mono disabled:opacity-40"
          />
          <button
            onClick={handleBrowse}
            disabled={isActive}
            className="px-2.5 py-2 bg-[#0d0d1a] border border-[#1a1a2e] rounded hover:border-[#00d4ff]/40 text-gray-500 hover:text-[#00d4ff] transition-colors disabled:opacity-40"
            title={t("shred.browseTitle")}
          >
            <FolderOpen size={14} />
          </button>
        </div>
        <p className="text-[10px] text-gray-700 mt-1 leading-relaxed">
          {/* File help */}
          <span dangerouslySetInnerHTML={{
            __html: t("shred.fileHelp")
              .replace(/<1>(.*?)<\/1>/g, '<strong class="text-gray-600">$1</strong>')
              .replace(/<2>(.*?)<\/2>/g, '<code class="font-mono">$1</code>')
          }} />
          <br />
          {/* Drive help */}
          <span dangerouslySetInnerHTML={{
            __html: t("shred.driveHelp")
              .replace(/<1>(.*?)<\/1>/g, '<strong class="text-gray-600">$1</strong>')
              .replace(/<2>(.*?)<\/2>/g, '<code class="font-mono">$1</code>')
          }} />
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
        <span className="text-xs text-gray-500">{t("shred.verify")}</span>
      </div>

      {/* Confirmation warning */}
      {targetPath && !isActive && !isComplete && (
        <div className="border border-red-900/40 bg-red-900/10 rounded-lg p-3">
          <div className="flex items-start gap-2">
            <AlertTriangle size={14} className="text-red-400 mt-0.5 shrink-0" />
            <div className="text-xs text-red-400/80">
              <strong className="text-red-400 block mb-1">{t("shred.warning.title")}</strong>
              <span dangerouslySetInnerHTML={{
                __html: t("shred.warning.body", {
                  path: targetPath,
                  algo: t(`shred.algorithms.${selectedAlgo}.name`),
                })
                  .replace(/<1>(.*?)<\/1>/g, '<code class="font-mono text-red-300">$1</code>')
                  .replace(/<2>(.*?)<\/2>/g, '<strong>$1</strong>')
              }} />
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
              {t("shred.warning.confirm")}
            </span>
          </div>
        </div>
      )}

      {/* Progress */}
      {isActive && progress && (
        <div className="space-y-2">
          <div className="flex justify-between text-xs">
            <span className="text-gray-500">
              {t("shred.progress.pass", {
                current: progress.current_pass,
                total: progress.total_passes,
                algo: progress.algorithm,
              })}
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
            <span>{formatBytes(progress.bytes_written)} {t("shred.written")}</span>
            <span>{formatBytes(progress.total_bytes)} {t("shred.total")}</span>
          </div>
        </div>
      )}

      {/* Complete */}
      {isComplete && (
        <div className="border border-green-900/30 bg-green-900/10 rounded-lg px-3 py-2 space-y-1">
          <div className="flex items-center gap-2 text-green-400 text-sm">
            <CheckCircle size={14} />
            {t("shred.complete.title")}
            {progress?.verification_passed === true && (
              <span className="text-[10px] text-green-400/60 ml-auto">
                {t("shred.complete.verified")}
              </span>
            )}
          </div>
          <p className="text-[10px] text-green-400/50">{t("shred.complete.body")}</p>
        </div>
      )}

      {/* Error */}
      {isError && (
        <div className="border border-red-900/40 bg-red-900/10 rounded-lg px-3 py-2 space-y-1">
          <div className="flex items-center gap-2 text-red-400 text-sm">
            <AlertTriangle size={14} />
            {t("shred.error.title")}
          </div>
          {error && (
            <p className="text-[10px] text-red-400/70 font-mono break-all">{error}</p>
          )}
          <p className="text-[10px] text-gray-600">{t("shred.error.hint")}</p>
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
            {t("shred.cancel")}
          </button>
        ) : (isComplete || isError) ? (
          <button
            onClick={onReset}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-[#0d0d1a] border border-[#1a1a2e] text-gray-400 text-sm font-medium hover:border-[#00d4ff]/40 hover:text-[#00d4ff] transition-all"
          >
            {t("shred.newOperation")}
          </button>
        ) : (
          <button
            onClick={handleStart}
            disabled={!targetPath || !confirmed}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-gradient-to-r from-red-900/40 to-red-800/40 border border-red-700/50 text-red-400 text-sm font-medium hover:from-red-800/50 hover:to-red-700/50 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <ShieldOff size={15} />
            {t("shred.execute")}
          </button>
        )}
      </div>

      {/* ── Free Space Wiper ─────────────────────────────────────────────────── */}
      <div className="border-t border-[#1a1a2e] pt-4 space-y-3">
        <div>
          <div className="flex items-center gap-2 mb-0.5">
            <Wind size={13} className="text-[#00d4ff]/50" />
            <span className="text-xs font-semibold text-gray-400 uppercase tracking-wider">
              {t("wipe.title")}
            </span>
          </div>
          <p className="text-[10px] text-gray-600 leading-relaxed">{t("wipe.description")}</p>
        </div>

        {/* Directory input */}
        {!isWiping && !isWipeComplete && !isWipeError && (
          <>
            <div className="flex gap-2">
              <input
                type="text"
                value={wipeDirPath}
                onChange={(e) => { setWipeDirPath(e.target.value); setWipeConfirmed(false); }}
                placeholder={t("wipe.placeholder")}
                className="flex-1 bg-[#0d0d1a] border border-[#1a1a2e] rounded px-3 py-2 text-xs text-gray-300 placeholder-gray-700 focus:outline-none focus:border-[#00d4ff]/50 font-mono"
              />
              <button
                onClick={handleWipeBrowse}
                className="px-2.5 py-2 bg-[#0d0d1a] border border-[#1a1a2e] rounded hover:border-[#00d4ff]/40 text-gray-500 hover:text-[#00d4ff] transition-colors"
                title={t("wipe.browseTitle")}
              >
                <FolderOpen size={14} />
              </button>
            </div>

            {/* Wipe confirmation */}
            {wipeDirPath && (
              <div className="border border-orange-900/40 bg-orange-900/10 rounded-lg p-3">
                <div className="flex items-start gap-2">
                  <AlertTriangle size={13} className="text-orange-400 mt-0.5 shrink-0" />
                  <p className="text-[10px] text-orange-400/80 leading-relaxed">
                    {t("wipe.warning", { path: wipeDirPath })}
                  </p>
                </div>
                <div className="flex items-center gap-2 mt-2">
                  <button
                    onClick={() => setWipeConfirmed((c) => !c)}
                    className={`w-4 h-4 rounded border flex items-center justify-center transition-colors ${
                      wipeConfirmed
                        ? "bg-orange-500/30 border-orange-500/60"
                        : "bg-transparent border-orange-900/40"
                    }`}
                  >
                    {wipeConfirmed && <CheckCircle size={10} className="text-orange-400" />}
                  </button>
                  <span className="text-[10px] text-orange-400/60">{t("wipe.confirm")}</span>
                </div>
              </div>
            )}

            <button
              onClick={() => onWipeStart(wipeDirPath)}
              disabled={!wipeDirPath || !wipeConfirmed}
              className="w-full flex items-center justify-center gap-2 py-2 rounded-lg bg-[#0d0d1a] border border-[#00d4ff]/20 text-[#00d4ff]/60 text-xs font-medium hover:border-[#00d4ff]/50 hover:text-[#00d4ff] transition-all disabled:opacity-30 disabled:cursor-not-allowed"
            >
              <Wind size={13} />
              {t("wipe.execute")}
            </button>
          </>
        )}

        {/* Wipe progress */}
        {isWiping && (
          <div className="space-y-2">
            <div className="flex justify-between text-xs">
              <span className="text-gray-500">{t("wipe.progress")}</span>
              {wipePct !== null && (
                <span className="text-gray-400 font-mono">{wipePct}%</span>
              )}
            </div>
            <div className="h-1.5 bg-[#1a1a2e] rounded-full overflow-hidden">
              <div
                className={`h-full rounded-full bg-gradient-to-r from-[#00d4ff]/40 to-[#00d4ff]/80 transition-all duration-300 ${
                  wipePct === null ? "animate-pulse w-full" : ""
                }`}
                style={wipePct !== null ? { width: `${wipePct}%` } : {}}
              />
            </div>
            {wipeProgress && (
              <div className="flex justify-between text-[10px] text-gray-600 font-mono">
                <span>{formatBytes(wipeProgress.bytes_written)} {t("shred.written")}</span>
                {wipeProgress.total_bytes > 0 && (
                  <span>{formatBytes(wipeProgress.total_bytes)} {t("shred.total")}</span>
                )}
              </div>
            )}
            <button
              onClick={onWipeCancel}
              className="w-full flex items-center justify-center gap-2 py-1.5 rounded-lg bg-[#0d0d1a] border border-[#1a1a2e] text-gray-500 text-xs hover:border-red-700/40 hover:text-red-400 transition-all"
            >
              <Square size={12} />
              {t("shred.cancel")}
            </button>
          </div>
        )}

        {/* Wipe complete */}
        {isWipeComplete && (
          <div className="border border-green-900/30 bg-green-900/10 rounded-lg px-3 py-2 space-y-1">
            <div className="flex items-center gap-2 text-green-400 text-xs">
              <CheckCircle size={13} />
              {t("wipe.complete")}
            </div>
            {wipeProgress && (
              <p className="text-[10px] text-green-400/50 font-mono">
                {formatBytes(wipeProgress.bytes_written)} {t("wipe.completeBytes")}
              </p>
            )}
            <button
              onClick={onWipeReset}
              className="text-[10px] text-gray-600 hover:text-gray-400 transition-colors"
            >
              {t("wipe.reset")}
            </button>
          </div>
        )}

        {/* Wipe error */}
        {isWipeError && (
          <div className="border border-red-900/40 bg-red-900/10 rounded-lg px-3 py-2 space-y-1">
            <div className="flex items-center gap-2 text-red-400 text-xs">
              <AlertTriangle size={13} />
              {t("wipe.error")}
            </div>
            {wipeError && (
              <p className="text-[10px] text-red-400/70 font-mono break-all">{wipeError}</p>
            )}
            <button
              onClick={onWipeReset}
              className="text-[10px] text-gray-600 hover:text-gray-400 transition-colors"
            >
              {t("wipe.reset")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
