import { describe, expect, it } from "vitest";
import { formatUnits } from "./format";

describe("credit formatting", () => {
  it("keeps 64-bit decimal values exact", () => {
    expect(formatUnits("9223372036854775807")).toBe(
      BigInt("9223372036854775807").toLocaleString("pt-BR"),
    );
  });
});
