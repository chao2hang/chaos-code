import type { ChatViewSummaryPanelVariant } from "@/v4/legacyChatViewTypes.js";

export const CONVERSATION_DRAFT_CONTENT_WIDTH_CLASS_NAME = "max-w-2xl";
// The status panel floats above the timeline instead of reserving a second content column.
// Keep both public class names aligned so opening the panel does not shift the conversation.
export const CONVERSATION_CONTENT_WITH_STATUS_PANEL_WIDTH_CLASS_NAME =
  "w-full @min-[864px]/conversation:max-w-4xl";
export const CONVERSATION_CONTENT_WITHOUT_STATUS_PANEL_WIDTH_CLASS_NAME =
  CONVERSATION_CONTENT_WITH_STATUS_PANEL_WIDTH_CLASS_NAME;
export const CONVERSATION_STATUS_PANEL_INLINE_OFFSET_CLASS_NAME = "-translate-x-42";
export const CONVERSATION_STATUS_PANEL_INLINE_OFFSET_PX = 168;
export const CONVERSATION_STATUS_PANEL_WIDE_OFFSET_CLASS_NAME =
  "@min-[1280px]/conversation:-translate-x-42";

type ConversationStatusPanelResolvedVariant = ChatViewSummaryPanelVariant | "auto";

export function getConversationContentWidthClassName(params: {
  centeredEmptyLayout: boolean;
  statusPanelLayout: "none" | "auto" | "inline";
}): string {
  if (params.centeredEmptyLayout) return CONVERSATION_DRAFT_CONTENT_WIDTH_CLASS_NAME;
  return CONVERSATION_CONTENT_WITH_STATUS_PANEL_WIDTH_CLASS_NAME;
}

export function resolveConversationStatusPanelVariant(params: {
  variantOverride: ConversationStatusPanelResolvedVariant | null;
}): ConversationStatusPanelResolvedVariant {
  // 自动模式必须保留到 DOM，由 conversation container query 裁决实际形态；
  // React 不再通过 ResizeObserver 把容器宽度翻译成业务状态。
  return params.variantOverride ?? "auto";
}

export function shouldUseConversationStatusPanelInlineLayout(params: {
  hasContent: boolean;
  variant: ConversationStatusPanelResolvedVariant;
}): boolean {
  return params.hasContent && params.variant !== "mini";
}

export function getConversationStatusPanelOffsetClassName(
  _layout: "none" | "auto" | "inline",
): string | undefined {
  // Floating status panel: keep the conversation column centered and stable.
  return undefined;
}
