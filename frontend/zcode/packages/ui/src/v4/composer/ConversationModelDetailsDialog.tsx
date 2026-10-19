import { useCallback, useEffect, useState } from "react";
import { Loader2Icon } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.js";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.js";
import { useServices } from "@/hooks/useServices.js";
import { useZCodeIntl } from "@/i18n/IntlProvider.js";
import type { ModelConfigObject } from "@zcode/provider";
import type { ProviderSettingsFormModel } from "@/lib/providerSettingsFormTypes.js";
import { resolveProviderSettingsFormProviders } from "@/lib/providerSettingsFormProjection.js";
import { useProviderModelDraft } from "@/settings/model-provider-section/useProviderModelDraft.js";
import { ProviderModelMetadataDialog } from "@/settings/model-provider-section/ProviderModelMetadataDialog.js";
import { decodeCustomModelValue, encodeCustomModelValue } from "@/lib/zcodeCustomModelValue.js";
import type { ModelSelectGroup } from "@/ModelConfigSelect.js";

interface ConversationModelEditorInnerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  model: ProviderSettingsFormModel;
  providerId: string;
  modelGroups: readonly ModelSelectGroup[];
  viewRevision: number;
  onSelectModel: (providerId: string, modelId: string) => void;
  onSaveSuccess: () => void;
}

function ConversationModelEditorInner({
  open,
  onOpenChange,
  model,
  providerId,
  modelGroups,
  viewRevision,
  onSelectModel,
  onSaveSuccess,
}: ConversationModelEditorInnerProps) {
  const { providerSettingsService } = useServices();
  const { intl } = useZCodeIntl();
  const [saving, setSaving] = useState(false);
  const [commitError, setCommitError] = useState<string | null>(null);
  const [draftErrorField, setDraftErrorField] = useState<
    | "id"
    | "contextWindow"
    | "maxOutputTokens"
    | "inputFormat"
    | "reasoningLevelValues"
    | "reasoningLevelMap"
    | null
  >(null);

  const resolveModelConfig = useCallback(
    (mId: string, personalConfig: ModelConfigObject) =>
      providerSettingsService.resolveModelConfig({
        providerId,
        originalModelId: model.modelId,
        modelId: mId,
        personalConfig: structuredClone(personalConfig),
      }),
    [providerId, model.modelId, providerSettingsService],
  );

  const editor = useProviderModelDraft({
    model,
    open,
    scopeKey: `${providerId}/${model.modelId}`,
    resolve: resolveModelConfig,
  });

  const commit = async (): Promise<boolean> => {
    setSaving(true);
    setCommitError(null);
    try {
      const result = await editor.commit();
      if (result.status === "invalid") {
        setDraftErrorField(result.field);
        return false;
      }
      setDraftErrorField(null);
      const nextModel = result.model;
      await providerSettingsService.savePersonalModelDraft({
        providerId,
        originalModelId: model.modelId,
        nextModelId: nextModel.modelId.trim(),
        personalConfig: structuredClone(nextModel.personalConfig),
        ...(nextModel.useRecommendedConfig === undefined
          ? {}
          : { useRecommendedConfig: nextModel.useRecommendedConfig }),
        basedOnRevision: viewRevision,
      });
      // ProviderRegistry 的变更会触发 ModelSelection 的 onDidChange，读视图会随事件刷新。
      await providerSettingsService.refresh("conversation-model-details-dialog");
      onSaveSuccess();
      onOpenChange(false);
      return true;
    } catch (err) {
      setCommitError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const modelSwitcher =
    modelGroups.length > 0 ? (
      <div className="mb-4">
        <label className="mb-1 block text-ui-base text-foreground-subtle">
          {intl.formatMessage({ id: "settings.modelProvider.switchModel" })}
        </label>
        <Select
          value={encodeCustomModelValue(providerId, model.modelId)}
          onValueChange={(nextVal) => {
            const decoded = decodeCustomModelValue(nextVal);
            if (decoded && decoded.modelName) {
              onSelectModel(decoded.providerId, decoded.modelName);
            }
          }}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="max-h-60 overflow-y-auto">
            {modelGroups.map((group) => (
              <SelectGroup key={group.key}>
                <SelectLabel>{group.label}</SelectLabel>
                {group.items.map((item) => (
                  <SelectItem key={item.key} value={item.value}>
                    {item.name}
                  </SelectItem>
                ))}
              </SelectGroup>
            ))}
          </SelectContent>
        </Select>
      </div>
    ) : null;

  return (
    <ProviderModelMetadataDialog
      mode="edit"
      open={open}
      hideTrigger
      modelSwitcher={modelSwitcher}
      draft={editor.draft}
      draftErrorMessage={
        commitError ??
        (draftErrorField
          ? intl.formatMessage({
              id: `settings.modelProvider.modelMetadata.invalid.${draftErrorField}`,
            })
          : null)
      }
      draftErrorField={draftErrorField}
      overrideFields={editor.overrides}
      inheritedConfig={editor.inheritedConfig}
      onOpenChange={onOpenChange}
      onDraftChange={editor.change}
      onRestore={() => {
        setDraftErrorField(null);
        setCommitError(null);
        void editor
          .restore()
          .catch((error) =>
            setCommitError(error instanceof Error ? error.message : String(error)),
          );
      }}
      onCommit={commit}
      saving={saving}
      modelConfigResolutionPending={editor.pending}
      modelDefaultsLoaded={editor.defaultsLoaded}
      onModelIdBlur={() => {
        void editor.flush().catch(() => undefined);
      }}
      modelIdReadOnly={model.builtin}
    />
  );
}

export function ConversationModelDetailsDialog({
  open,
  onOpenChange,
  providerId,
  modelId,
  modelGroups,
  onSaveSuccess,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  providerId: string;
  modelId: string;
  modelGroups: readonly ModelSelectGroup[];
  onSaveSuccess?: () => void;
}) {
  const { providerSettingsService } = useServices();
  const { intl } = useZCodeIntl();
  const [selectedProviderId, setSelectedProviderId] = useState(providerId);
  const [selectedModelId, setSelectedModelId] = useState(modelId);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [formModel, setFormModel] = useState<ProviderSettingsFormModel | null>(null);
  const [viewRevision, setViewRevision] = useState(0);

  useEffect(() => {
    setSelectedProviderId(providerId);
    setSelectedModelId(modelId);
  }, [providerId, modelId]);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setLoadError(null);
    void providerSettingsService
      .getView()
      .then((view) => {
        if (!active) return;
        setViewRevision(view.revision);
        const formProviders = resolveProviderSettingsFormProviders({ view });
        const provider = formProviders.find((p) => p.providerId === selectedProviderId);
        const model = provider?.models.find((m) => m.modelId === selectedModelId);
        if (model) {
          setFormModel(model);
        } else {
          setFormModel({
            kind: "candidate",
            modelId: selectedModelId,
            builtin: false,
            personalConfig: {},
            config: { properties: { supportsToolCall: true } },
            hasPersonalConfig: false,
            executable: true,
            selectable: true,
          });
        }
        setLoading(false);
      })
      .catch((err) => {
        if (!active) return;
        setLoadError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [open, selectedProviderId, selectedModelId, providerSettingsService]);

  if (!open) return null;

  if (loading || !formModel) {
    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {intl.formatMessage({ id: "settings.modelProvider.editModel" })}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {intl.formatMessage({ id: "settings.modelProvider.editModelDescription" })}
            </DialogDescription>
          </DialogHeader>
          <div className="flex h-36 items-center justify-center gap-2 text-foreground-subtle">
            {loadError ? (
              <span className="text-destructive">{loadError}</span>
            ) : (
              <>
                <Loader2Icon className="size-4 animate-spin" />
                <span className="text-ui-base">
                  {intl.formatMessage({ id: "common.loading" })}
                </span>
              </>
            )}
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <ConversationModelEditorInner
      key={`${selectedProviderId}/${selectedModelId}`}
      open={open}
      onOpenChange={onOpenChange}
      model={formModel}
      providerId={selectedProviderId}
      modelGroups={modelGroups}
      viewRevision={viewRevision}
      onSelectModel={(pId, mId) => {
        setSelectedProviderId(pId);
        setSelectedModelId(mId);
      }}
      onSaveSuccess={() => onSaveSuccess?.()}
    />
  );
}
