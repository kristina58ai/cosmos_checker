import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock IPC core перед импортом App.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "ping") return "pong";
    throw new Error(`unknown command: ${cmd}`);
  }),
}));

import App from "../App";

describe("App (Stage 1 smoke)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders title and pings backend", async () => {
    render(<App />);
    expect(screen.getByText("Cosmos Checker")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId("ipc-status")).toHaveTextContent("pong");
    });
  });
});
