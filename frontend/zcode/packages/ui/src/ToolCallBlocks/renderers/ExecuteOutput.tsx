import { useLayoutEffect, useMemo, useRef, useState, type ComponentType, type ReactNode } from "react";
import RawAnsi from "ansi-to-react";
import { WrapTextIcon } from "lucide-react";
import { Button } from "@/components/ui/button.js";
import { ScrollFadeViewport } from "@/components/ui/scroll-fade-viewport.js";
import { useZCodeIntl } from "@/i18n/IntlProvider.js";
import { logger } from "@/logger.js";

type AnsiProps = {
  children?: string;
  className?: string;
  linkify?: boolean | "fuzzy";
  useClasses?: boolean;
};

const Ansi =
  (RawAnsi as unknown as { default?: ComponentType<AnsiProps> }).default ||
  (RawAnsi as unknown as ComponentType<AnsiProps>);

// oxlint-disable-next-line no-control-regex
const ANSI_REGEX = /\u001b\[[0-9;]*[a-zA-Z]/;

/**
 * 为无 ANSI 颜色的终端纯文本提供轻量级语法与语义高亮（如 Git status、Diff、警告与错误）。
 */
function renderHighlightedTerminalOutput(content: string): ReactNode {
  if (ANSI_REGEX.test(content)) {
    return <Ansi useClasses={false}>{content}</Ansi>;
  }

  const lines = content.split("\n");
  return lines.map((line, idx) => {
    const isLast = idx === lines.length - 1;

    // 匹配 Git status: "    modified:   path/to/file" 或 "modified: file"
    const gitStatusMatch = line.match(
      /^(\s*)(modified|new file|deleted|renamed|both modified|copied):(\s+)(.+)$/,
    );
    if (gitStatusMatch) {
      const [, indent, status, space, file] = gitStatusMatch;
      let statusColor = "text-amber-500 dark:text-amber-400";
      if (status === "new file") statusColor = "text-emerald-500 dark:text-emerald-400";
      else if (status === "deleted") statusColor = "text-rose-500 dark:text-rose-400";
      else if (status === "renamed") statusColor = "text-cyan-500 dark:text-cyan-400";

      return (
        <span key={idx} className="block">
          {indent}
          <span className={`font-semibold ${statusColor}`}>{status}:</span>
          {space}
          <span className="text-foreground">{file}</span>
          {!isLast && "\n"}
        </span>
      );
    }

    // 匹配 Git branch 信息: "On branch main"
    const onBranchMatch = line.match(/^(On branch\s+)(\S+)(.*)$/);
    if (onBranchMatch) {
      const [, prefix, branch, rest] = onBranchMatch;
      return (
        <span key={idx} className="block">
          <span className="font-medium text-foreground-subtle">{prefix}</span>
          <span className="font-semibold text-cyan-500 dark:text-cyan-400">{branch}</span>
          <span className="text-foreground-subtle">{rest}</span>
          {!isLast && "\n"}
        </span>
      );
    }

    // 匹配 Git 提示括号: '  (use "git add <file>..." to update what will be committed)'
    if (/^\s*\(use ".*?" to .*?\)/.test(line)) {
      return (
        <span key={idx} className="block italic text-foreground-subtlest">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }

    // 匹配 Git 状态分类头部: "Changes not staged for commit:" / "Untracked files:"
    if (
      /^(?:Changes to be committed|Changes not staged for commit|Untracked files):/i.test(
        line.trim(),
      )
    ) {
      return (
        <span key={idx} className="block font-semibold text-foreground-subtle">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }

    // 匹配 Diff 行:
    if (line.startsWith("+") && !line.startsWith("+++")) {
      return (
        <span
          key={idx}
          className="block text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 -mx-3 px-3"
        >
          {line}
          {!isLast && "\n"}
        </span>
      );
    }
    if (line.startsWith("-") && !line.startsWith("---")) {
      return (
        <span
          key={idx}
          className="block text-rose-600 dark:text-rose-400 bg-rose-500/10 -mx-3 px-3"
        >
          {line}
          {!isLast && "\n"}
        </span>
      );
    }
    if (line.startsWith("@@") && line.includes("@@")) {
      return (
        <span key={idx} className="block font-medium text-cyan-600 dark:text-cyan-400">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }
    if (line.startsWith("diff --git ") || line.startsWith("index ")) {
      return (
        <span key={idx} className="block font-semibold text-foreground-subtle">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }

    // 匹配 Error / Warning
    if (/(?:error|FAILED|fatal):/i.test(line)) {
      return (
        <span key={idx} className="block font-medium text-rose-500 dark:text-rose-400">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }
    if (/(?:warning|WARN):/i.test(line)) {
      return (
        <span key={idx} className="block text-amber-500 dark:text-amber-400">
          {line}
          {!isLast && "\n"}
        </span>
      );
    }

    return (
      <span key={idx} className="block">
        {line}
        {!isLast && "\n"}
      </span>
    );
  });
}

export function ExecuteOutput({ text, running }: { text: string; running: boolean }) {
  const { intl } = useZCodeIntl();
  const scroll = useRef<HTMLDivElement>(null);
  const previousTop = useRef(0);
  const hasStreamed = useRef(running);
  const [frozen, setFrozen] = useState<string | null>(null);
  const [wrapped, setWrapped] = useState(false);
  const following = frozen === null;
  const display = frozen ?? text;
  const wrapLabel = intl.formatMessage({
    id: wrapped ? "chat.toolCall.execute.preserveColumns" : "chat.toolCall.execute.wrapOutput",
  });
  const highlightedContent = useMemo(
    () => renderHighlightedTerminalOutput(display),
    [display],
  );

  useLayoutEffect(() => {
    if (running) hasStreamed.current = true;
    if (!hasStreamed.current || !following || !scroll.current) return;
    scroll.current.scrollTop = scroll.current.scrollHeight;
    // 程序吸底后记录浏览器实际位置，避免尾窗变短时把程序滚动误判为上滚。
    previousTop.current = scroll.current.scrollTop;
  }, [display, following, running, wrapped]);

  return (
    <div className="min-w-0 max-w-full flex-none overflow-hidden rounded-lg border border-border bg-background">
      <div className="flex items-center justify-between gap-2 border-b border-border px-2 py-1">
        <span className="text-ui-sm text-foreground-subtle">
          {intl.formatMessage({ id: "chat.toolCall.execute.output" })}
        </span>
        <Button
          data-testid="bash-output-wrap"
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={wrapLabel}
          aria-pressed={wrapped}
          title={wrapLabel}
          onClick={() => setWrapped((current) => !current)}
        >
          <WrapTextIcon className="size-3.5" aria-hidden="true" />
        </Button>
      </div>
      <ScrollFadeViewport
        ref={scroll}
        data-testid="bash-output-scroll"
        data-following={following}
        data-wrapped={wrapped}
        className="min-w-0 max-w-full max-h-[8lh] sm:max-h-[12lh] flex-none overflow-auto leading-5"
        tabIndex={0}
        onScroll={(event) => {
          const el = event.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= 8;
          if (hasStreamed.current && following && el.scrollTop < previousTop.current && !atBottom) {
            setFrozen(display);
            logger.debug("Bash output following changed", { following: false });
          } else if (!following && atBottom) {
            setFrozen(null);
            logger.debug("Bash output following changed", { following: true });
          }
          previousTop.current = el.scrollTop;
        }}
      >
        <pre
          data-testid={running ? "bash-output-preview-full" : "bash-result-output"}
          className={
            wrapped
              ? "w-full min-w-0 whitespace-pre-wrap break-words px-3 py-2 font-mono text-ui-base leading-5 text-foreground"
              : "w-max min-w-full whitespace-pre px-3 py-2 font-mono text-ui-base leading-5 text-foreground"
          }
        >
          {highlightedContent}
        </pre>
      </ScrollFadeViewport>
    </div>
  );
}
