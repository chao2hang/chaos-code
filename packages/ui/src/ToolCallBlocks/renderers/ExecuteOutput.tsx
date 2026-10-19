import { useLayoutEffect, useRef, useState } from "react";
import { WrapTextIcon } from "lucide-react";
import { Button } from "@/components/ui/button.js";
import { ScrollFadeViewport } from "@/components/ui/scroll-fade-viewport.js";
import { useZCodeIntl } from "@/i18n/IntlProvider.js";
import { logger } from "@/logger.js";

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
          {display}
        </pre>
      </ScrollFadeViewport>
    </div>
  );
}
