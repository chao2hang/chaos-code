import { useCallback, useId, useState, type MouseEvent } from "react";
import { CheckIcon, ChevronRightIcon, CopyIcon } from "lucide-react";
import { Button } from "@/components/ui/button.js";
import { cn } from "@/components/lib/utils.js";
import { useZCodeIntl } from "@/i18n/IntlProvider.js";
import { logger } from "@/logger.js";

export function ExecuteCommand({ command }: { command: string | undefined }) {
  const { intl } = useZCodeIntl();
  const id = useId();
  // 默认折叠：提供收起的命令条，用户可按需展开以学习与阅读完整参数，并支持一键复制
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(
    (e: MouseEvent<HTMLButtonElement>) => {
      e.stopPropagation();
      if (!command) return;
      void navigator.clipboard.writeText(command).then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      });
    },
    [command],
  );

  // 命令为空时不渲染任何内容，避免孤立展示无意义的 "$" 提示符
  if (!command?.trim()) {
    return null;
  }

  const trimmedCommand = command.trim();

  return (
    <div className="w-full min-w-0 max-w-full flex-none overflow-hidden rounded-lg border border-border bg-background">
      <div
        data-testid="execute-command-header"
        className="flex items-center justify-between gap-2 px-2.5 py-1.5 cursor-pointer select-none hover:bg-muted/40 transition-colors"
        onClick={() => {
          setExpanded((prev) => !prev);
          logger.debug("Execute command toggled", { expanded: !expanded });
        }}
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-controls={id}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setExpanded((prev) => !prev);
          }
        }}
      >
        <div className="flex min-w-0 flex-1 items-center gap-1.5 font-mono text-ui-sm">
          <ChevronRightIcon
            className={cn(
              "size-3.5 shrink-0 text-foreground-subtle transition-transform duration-200",
              expanded && "rotate-90",
            )}
            aria-hidden="true"
          />
          <span className="font-sans font-medium text-foreground-subtle shrink-0">
            {intl.formatMessage({
              id: "chat.toolCall.execute.command",
            })}
          </span>
          {!expanded && (
            <span className="truncate text-ui-xs text-foreground-subtlest ml-1 font-mono">
              $ {trimmedCommand}
            </span>
          )}
        </div>
        <Button
          data-testid="execute-command-copy"
          type="button"
          variant="ghost"
          size="icon-sm"
          className="size-6 text-foreground-subtle hover:text-foreground"
          aria-label={copied ? "已复制" : "复制命令"}
          title={copied ? "已复制" : "复制命令"}
          onClick={handleCopy}
        >
          {copied ? (
            <CheckIcon className="size-3 text-emerald-500" />
          ) : (
            <CopyIcon className="size-3" />
          )}
        </Button>
      </div>

      <div
        id={id}
        data-testid="execute-command"
        className={cn(
          "border-t border-border px-3 py-2 bg-muted/20 font-mono text-ui-base leading-5",
          !expanded && "hidden",
        )}
      >
        <div className="flex items-start gap-2">
          <span
            data-testid="execute-command-prompt"
            className="shrink-0 text-foreground-subtle select-none font-bold"
          >
            $
          </span>
          <pre className="min-w-0 flex-1 whitespace-pre-wrap break-words text-foreground font-mono">
            {command}
          </pre>
        </div>
      </div>
    </div>
  );
}
