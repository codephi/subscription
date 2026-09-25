import { create } from "zustand";

interface PanelState {
  workspaceSearch: string;
  selectedItemId: string | null;
  billingWorkspaceFilter: string;
  billingStatusFilter: string;
  setWorkspaceSearch: (value: string) => void;
  selectItem: (value: string | null) => void;
  setBillingWorkspaceFilter: (value: string) => void;
  setBillingStatusFilter: (value: string) => void;
}

export const usePanelStore = create<PanelState>((set) => ({
  workspaceSearch: "",
  selectedItemId: null,
  billingWorkspaceFilter: "",
  billingStatusFilter: "all",
  setWorkspaceSearch: (workspaceSearch) => set({ workspaceSearch }),
  selectItem: (selectedItemId) => set({ selectedItemId }),
  setBillingWorkspaceFilter: (billingWorkspaceFilter) => set({ billingWorkspaceFilter }),
  setBillingStatusFilter: (billingStatusFilter) => set({ billingStatusFilter }),
}));
