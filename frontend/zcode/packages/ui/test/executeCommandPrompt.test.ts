// @vitest-environment jsdom
import { createElement } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ExecuteCommand } from "@/ToolCallBlocks/renderers/ExecuteCommand.js";
import { ZCodeIntlProvider } from "@/i18n/IntlProvider.js";

beforeEach(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function renderCommand(command: string | undefined) {
  return render(
    createElement(
      ZCodeIntlProvider,
      { initialLocale: "zh-CN" },
      createElement(ExecuteCommand, { command }),
    ),
  );
}

describe("ExecuteCommand prompt and command rendering", () => {
  it("renders $ prompt and command when command is provided", () => {
    const screen = renderCommand("git status");
    expect(screen.getByTestId("execute-command-prompt").textContent?.trim()).toBe("$");
    expect(screen.getByTestId("execute-command").textContent).toContain("git status");
  });

  it("renders nothing when command is undefined or empty whitespace", () => {
    const screenUndefined = renderCommand(undefined);
    expect(screenUndefined.queryByTestId("execute-command-prompt")).toBeNull();
    expect(screenUndefined.queryByTestId("execute-command")).toBeNull();

    const screenEmpty = renderCommand("   ");
    expect(screenEmpty.queryByTestId("execute-command-prompt")).toBeNull();
    expect(screenEmpty.queryByTestId("execute-command")).toBeNull();
  });
});
