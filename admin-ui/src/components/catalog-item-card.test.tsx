// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { expect, it, vi } from "vitest";
import type { ItemResponse } from "@/api/catalog-api";
import { CatalogItemCard } from "./catalog-item-card";

const item: ItemResponse = {
  item_id: "item-1",
  product_id: "product-1",
  parent_item_id: null,
  name: "Tarefa",
  unit_name: "tarefa",
  quantity_scale: "1",
  status: "ACTIVE",
  version: 1,
  created_at: "2026-09-27T00:00:00Z",
  updated_at: "2026-09-27T00:00:00Z",
};

it("saves the scale and closes the item editor after success", async () => {
  const save = vi.fn().mockResolvedValue({});
  render(
    <MemoryRouter>
      <CatalogItemCard
        item={item}
        items={[item]}
        prices={[]}
        usageModel="CREDIT_METERED"
        saving={false}
        publishingPriceId={null}
        onSave={save}
        onPublishPrice={() => undefined}
      />
    </MemoryRouter>,
  );

  fireEvent.click(screen.getByRole("button", { name: "Editar item" }));
  fireEvent.change(screen.getByLabelText("Escala de quantidade"), {
    target: { value: "10" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Salvar item" }));

  await waitFor(() => {
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ quantity_scale: "10", expected_version: 1 }),
    );
  });
  await waitFor(() => {
    expect(screen.queryByLabelText("Escala de quantidade")).toBeNull();
  });
});
