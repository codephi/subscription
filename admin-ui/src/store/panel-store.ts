import { create } from "zustand";

interface PanelState {
  accountSearch: string;
  selectedItemId: string | null;
  billingAccountFilter: string;
  billingStatusFilter: string;
  setAccountSearch: (value: string) => void;
  selectItem: (value: string | null) => void;
  setBillingAccountFilter: (value: string) => void;
  setBillingStatusFilter: (value: string) => void;
}

export const usePanelStore = create<PanelState>((set) => ({
  accountSearch: "",
  selectedItemId: null,
  billingAccountFilter: "",
  billingStatusFilter: "all",
  setAccountSearch: (accountSearch) => set({ accountSearch }),
  selectItem: (selectedItemId) => set({ selectedItemId }),
  setBillingAccountFilter: (billingAccountFilter) => set({ billingAccountFilter }),
  setBillingStatusFilter: (billingStatusFilter) => set({ billingStatusFilter }),
}));
