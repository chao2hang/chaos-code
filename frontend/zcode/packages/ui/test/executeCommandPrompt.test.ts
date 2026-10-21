// @vitest-environment jsdom
import { createElement } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
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
  it("renders $ prompt and command when command is provided, and is collapsed by default", () => {
    const screen = renderCommand("git status");
    expect(screen.getByTestId("execute-command-prompt").textContent?.trim()).toBe("$");
    expect(screen.getByTestId("execute-command").textContent).toContain("git status");
    // 默认折叠：正文带有 hidden 类，折叠条上显示预览和复制按钮
    expect(screen.getByTestId("execute-command").className).toContain("hidden");
    expect(screen.getByText("输入命令")).toBeTruthy();
    expect(screen.getByText("$ git status")).toBeTruthy();
    expect(screen.getByTestId("execute-command-copy")).toBeTruthy();
  });

  it("toggles expanded state when clicking the command bar", () => {
    const screen = renderCommand("git log -n 5");
    const toggleHeader = screen.getByTestId("execute-command-header");
    expect(screen.getByTestId("execute-command").className).toContain("hidden");

    // 点击展开
    fireEvent.click(toggleHeader);
    expect(screen.getByTestId("execute-command").className).not.toContain("hidden");

    // 再次点击折叠
    fireEvent.click(toggleHeader);
    expect(screen.getByTestId("execute-command").className).toContain("hidden");
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
