import { create } from "zustand";

export interface SessionProgress {
  checked: number;
  total: number;
  speed: number; // wallets / min
  currentAddress?: string;
}

interface SessionState {
  sessionId: string | null;
  progress: SessionProgress | null;
  setSession: (id: string | null) => void;
  setProgress: (p: SessionProgress | null) => void;
}

export const useSessionStore = create<SessionState>((set) => ({
  sessionId: null,
  progress: null,
  setSession: (sessionId) => set({ sessionId }),
  setProgress: (progress) => set({ progress }),
}));
