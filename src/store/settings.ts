import { create } from "zustand";

export interface AppSettings {
  maxConcurrency: number;
  requestTimeoutMs: number;
  proxyEnabled: boolean;
}

const DEFAULTS: AppSettings = {
  maxConcurrency: 100,
  requestTimeoutMs: 5000,
  proxyEnabled: false,
};

interface SettingsState extends AppSettings {
  update: (patch: Partial<AppSettings>) => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  ...DEFAULTS,
  update: (patch) => set((state) => ({ ...state, ...patch })),
}));
