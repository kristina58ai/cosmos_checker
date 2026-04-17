import { create } from "zustand";

export interface ChainInfo {
  chainId: string;
  bech32Prefix: string;
  slip44: number;
  prettyName?: string;
}

interface ChainsState {
  chains: ChainInfo[];
  selected: Set<string>;
  setChains: (c: ChainInfo[]) => void;
  toggle: (chainId: string) => void;
  clear: () => void;
}

export const useChainsStore = create<ChainsState>((set) => ({
  chains: [],
  selected: new Set(),
  setChains: (chains) => set({ chains }),
  toggle: (chainId) =>
    set((state) => {
      const next = new Set(state.selected);
      if (next.has(chainId)) next.delete(chainId);
      else next.add(chainId);
      return { selected: next };
    }),
  clear: () => set({ selected: new Set() }),
}));
