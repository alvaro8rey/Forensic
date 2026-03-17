import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { ScanProgress, ShredProgress, RecoveredFile, DeletedMftEntry } from "../types";

interface EventHandlers {
  onScanProgress?: (p: ScanProgress) => void;
  onScanComplete?: (files: RecoveredFile[]) => void;
  onScanError?: (err: string) => void;
  onShredProgress?: (p: ShredProgress) => void;
  onShredComplete?: (path: string) => void;
  onShredError?: (err: string) => void;
  onMftComplete?: (entries: DeletedMftEntry[]) => void;
  onMftError?: (err: string) => void;
}

export function useTauriEvents(handlers: EventHandlers) {
  useEffect(() => {
    const unlisten: Array<() => void> = [];

    (async () => {
      if (handlers.onScanProgress) {
        const u = await listen<ScanProgress>("scan-progress", (e) =>
          handlers.onScanProgress!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onScanComplete) {
        const u = await listen<RecoveredFile[]>("scan-complete", (e) =>
          handlers.onScanComplete!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onScanError) {
        const u = await listen<string>("scan-error", (e) =>
          handlers.onScanError!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onShredProgress) {
        const u = await listen<ShredProgress>("shred-progress", (e) =>
          handlers.onShredProgress!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onShredComplete) {
        const u = await listen<string>("shred-complete", (e) =>
          handlers.onShredComplete!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onShredError) {
        const u = await listen<string>("shred-error", (e) =>
          handlers.onShredError!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onMftComplete) {
        const u = await listen<DeletedMftEntry[]>("mft-complete", (e) =>
          handlers.onMftComplete!(e.payload)
        );
        unlisten.push(u);
      }
      if (handlers.onMftError) {
        const u = await listen<string>("mft-error", (e) =>
          handlers.onMftError!(e.payload)
        );
        unlisten.push(u);
      }
    })();

    return () => unlisten.forEach((u) => u());
  }, []);
}
