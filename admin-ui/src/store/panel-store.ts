import { create } from "zustand";

interface PanelState {
  workspaceSearch: string;
  selectedItemId: string | null;
  setWorkspaceSearch: (value: string) => void;
  selectItem: (value: string | null) => void;
}

export const usePanelStore = create<PanelState>((set) => ({
  workspaceSearch: "",
  selectedItemId: null,
  setWorkspaceSearch: (workspaceSearch) => set({ workspaceSearch }),
  selectItem: (selectedItemId) => set({ selectedItemId }),
}));
