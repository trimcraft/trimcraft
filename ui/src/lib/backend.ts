// Connection to the Rust side. Inside Tauri this uses IPC; in a plain browser (vite dev) it
// falls back to a small in-page demo so the UI can be developed and checked without the app.

import type {
  AppMeta,
  ButtonPair,
  ControlRef,
  DetectorConfig,
  DeviceId,
  EngineSnapshot,
  FunctionDef,
  OverlayConfig,
} from "./types";
import { createDemo } from "./demo";

export interface Backend {
  meta(): Promise<AppMeta>;
  subscribe(onSnapshot: (s: EngineSnapshot) => void): Promise<() => void>;
  setDeviceLabel(device: DeviceId, label: string | null): Promise<void>;
  addEncoder(name: string, device: DeviceId, decrement: number, increment: number): Promise<string>;
  renameEncoder(id: string, name: string): Promise<void>;
  swapEncoder(id: string): Promise<void>;
  removeEncoder(id: string): Promise<void>;
  dismissSuggestion(pair: ButtonPair): Promise<void>;
  saveFunction(fn: FunctionDef): Promise<string>;
  removeFunction(id: string): Promise<void>;
  setFunctionValue(id: string, value: number): Promise<void>;
  toggleOverlayControl(device: DeviceId, control: ControlRef): Promise<boolean>;
  toggleOverlayFunction(id: string): Promise<boolean>;
  setOverlayLayout(layout: Pick<OverlayConfig, "anchor" | "margin" | "scale">): Promise<void>;
  setOverlayVisible(visible: boolean): Promise<void>;
  setHotkey(hotkey: string): Promise<void>;
  setDetectorConfig(config: DetectorConfig): Promise<void>;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    meta: () => invoke("get_meta"),
    async subscribe(onSnapshot) {
      const unlisten = await listen<EngineSnapshot>("detent://snapshot", (e) => onSnapshot(e.payload));
      onSnapshot(await invoke<EngineSnapshot>("get_snapshot"));
      return unlisten;
    },
    setDeviceLabel: (device, label) => invoke("set_device_label", { device, label }),
    addEncoder: (name, device, decrement, increment) =>
      invoke("add_encoder", { name, device, decrement, increment }),
    renameEncoder: (id, name) => invoke("rename_encoder", { id, name }),
    swapEncoder: (id) => invoke("swap_encoder", { id }),
    removeEncoder: (id) => invoke("remove_encoder", { id }),
    dismissSuggestion: (pair) => invoke("dismiss_suggestion", { pair }),
    saveFunction: (fn) => invoke("save_function", { function: fn }),
    removeFunction: (id) => invoke("remove_function", { id }),
    setFunctionValue: (id, value) => invoke("set_function_value", { id, value }),
    toggleOverlayControl: (device, control) => invoke("toggle_overlay_control", { device, control }),
    toggleOverlayFunction: (id) => invoke("toggle_overlay_function", { id }),
    setOverlayLayout: (layout) => invoke("set_overlay_layout", { layout }),
    setOverlayVisible: (visible) => invoke("set_overlay_visible", { visible }),
    setHotkey: (hotkey) => invoke("set_hotkey", { hotkey }),
    setDetectorConfig: (config) => invoke("set_detector_config", { config }),
  };
}

let instance: Promise<Backend> | null = null;

export function backend(): Promise<Backend> {
  instance ??= isTauri ? tauriBackend() : Promise.resolve(createDemo());
  return instance;
}

/** Error text from a rejected invoke (Rust errors arrive as strings). */
export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
