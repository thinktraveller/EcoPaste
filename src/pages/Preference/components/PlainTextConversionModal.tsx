import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useMount, useUnmount } from "ahooks";
import {
  Alert,
  Button,
  Card,
  Collapse,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Switch,
} from "antd";
import type { ChangeEvent, FC } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import {
  previewPlainTextConversion,
  savePlainTextConversionSettings,
  setWindowDirty,
} from "@/commands";
import {
  createDefaultPlainTextConversion,
  MAX_CUSTOM_RULES,
} from "@/constants/plainTextConversion";
import type {
  CustomReplacementRule,
  PlainTextConversionEditor,
  PlainTextConversionSettings,
  PlainTextPreview,
} from "@/types/settings";
import { getMessageApi } from "@/utils/feedback";

interface PlainTextConversionModalProps {
  editor: PlainTextConversionEditor;
  onCancel: () => void;
  onSaved: () => void;
}

const CONVERSION_DIRTY_OWNER = "plain-text-conversion";

/**
 * 编辑去格式规则的独立草稿，预览与保存均由 Rust 校验，取消不影响已生效配置。
 */
const PlainTextConversionModal: FC<PlainTextConversionModalProps> = (props) => {
  const { editor, onCancel, onSaved } = props;
  const { t } = useTranslation(["preferences", "common"]);
  const [draft, setDraft] = useState<PlainTextConversionSettings>(() => {
    return structuredClone(editor.config);
  });
  const [input, setInput] = useState("");
  const [inputFormat, setInputFormat] = useState<"text" | "html">("text");
  const [preview, setPreview] = useState<PlainTextPreview | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [scrollRuleId, setScrollRuleId] = useState<string | null>(null);
  const [dirtyOwner] = useState(() => {
    return `${CONVERSION_DIRTY_OWNER}:${crypto.randomUUID()}`;
  });
  const ruleListRef = useRef<VirtuosoHandle>(null);
  const requestRef = useRef(0);
  const disposedRef = useRef(false);
  const windowLabelRef = useRef(getCurrentWebviewWindow().label);
  const busy = saving || previewing;

  useMount(() => {
    void protectDraft();
  });

  useUnmount(() => {
    disposedRef.current = true;
    requestRef.current += 1;
    void releaseDraft();
  });

  useEffect(() => {
    if (!scrollRuleId) return;

    const index = draft.rules.findIndex((rule) => {
      return rule.id === scrollRuleId;
    });
    if (index >= 0)
      ruleListRef.current?.scrollToIndex({ align: "start", index });
    setScrollRuleId(null);
  }, [draft.rules, scrollRuleId]);

  /**
   * 弹窗存在期间阻止窗口空闲销毁；挂载请求延迟返回时仍配对释放。
   */
  async function protectDraft() {
    try {
      await setWindowDirty(windowLabelRef.current, dirtyOwner, true);
      if (disposedRef.current) await releaseDraft();
    } catch {
      return;
    }
  }

  async function releaseDraft() {
    try {
      await setWindowDirty(windowLabelRef.current, dirtyOwner, false);
    } catch {
      return;
    }
  }

  /**
   * 编辑后丢弃旧输出及旧请求，避免将过期结果显示成当前草稿效果。
   */
  function invalidatePreview() {
    requestRef.current += 1;
    setPreview(null);
    setError("");
  }

  function updateBuiltin(patch: Partial<PlainTextConversionSettings>) {
    invalidatePreview();
    setDraft((current) => {
      return { ...current, ...patch };
    });
  }

  function changeLinkStyle(value: PlainTextConversionSettings["linkStyle"]) {
    updateBuiltin({ linkStyle: value });
  }

  function changeImageStyle(value: PlainTextConversionSettings["imageStyle"]) {
    updateBuiltin({ imageStyle: value });
  }

  function changeListStyle(value: PlainTextConversionSettings["listStyle"]) {
    updateBuiltin({ listStyle: value });
  }

  function changeTaskStyle(value: PlainTextConversionSettings["taskStyle"]) {
    updateBuiltin({ taskStyle: value });
  }

  function changeTableSeparator(
    value: PlainTextConversionSettings["tableSeparator"],
  ) {
    updateBuiltin({ tableSeparator: value });
  }

  function changeBlankLines(checked: boolean) {
    updateBuiltin({ collapseBlankLines: checked });
  }

  function addRule() {
    invalidatePreview();
    const rule: CustomReplacementRule = {
      enabled: true,
      id: crypto.randomUUID(),
      mode: "literal",
      name: t("plainTextConversion.newRuleName", {
        index: draft.rules.length + 1,
      }),
      pattern: "",
      replacement: "",
    };
    setDraft((current) => {
      return { ...current, rules: [...current.rules, rule] };
    });
    setScrollRuleId(rule.id);
  }

  function changeRule(id: string, patch: Partial<CustomReplacementRule>) {
    invalidatePreview();
    setDraft((current) => {
      return {
        ...current,
        rules: current.rules.map((rule) => {
          return rule.id === id ? { ...rule, ...patch } : rule;
        }),
      };
    });
  }

  function deleteRule(id: string) {
    invalidatePreview();
    setDraft((current) => {
      return {
        ...current,
        rules: current.rules.filter((rule) => {
          return rule.id !== id;
        }),
      };
    });
  }

  function moveRule(id: string, direction: -1 | 1) {
    invalidatePreview();
    setDraft((current) => {
      const index = current.rules.findIndex((rule) => {
        return rule.id === id;
      });
      const target = index + direction;
      if (index < 0 || target < 0 || target >= current.rules.length)
        return current;

      const rules = [...current.rules];
      [rules[index], rules[target]] = [rules[target], rules[index]];
      return { ...current, rules };
    });
    setScrollRuleId(id);
  }

  function restoreDefaults() {
    invalidatePreview();
    setDraft(createDefaultPlainTextConversion());
  }

  function changeInput(event: ChangeEvent<HTMLTextAreaElement>) {
    invalidatePreview();
    setInput(event.target.value);
  }

  function changeInputFormat(value: "text" | "html") {
    invalidatePreview();
    setInputFormat(value);
  }

  /**
   * 仅将用户输入样例交给 Rust，关闭弹窗后不回写异步结果。
   */
  async function runPreview() {
    if (busy) return;
    const request = ++requestRef.current;
    setError("");
    setPreview(null);
    setPreviewing(true);
    try {
      const result = await previewPlainTextConversion(
        input,
        inputFormat,
        draft,
      );
      if (!disposedRef.current && request === requestRef.current)
        setPreview(result);
    } catch (cause) {
      if (!disposedRef.current && request === requestRef.current) {
        setError(errorMessage(cause, t("plainTextConversion.errorFallback")));
      }
    } finally {
      if (!disposedRef.current) setPreviewing(false);
    }
  }

  /**
   * 用打开时的版本标记整体保存，冲突或失败保留草稿供用户修正。
   */
  async function saveRules() {
    if (busy) return;
    setError("");
    setSaving(true);
    try {
      await savePlainTextConversionSettings(draft, editor.revision);
      if (disposedRef.current) return;

      getMessageApi().success(t("plainTextConversion.saved"));
      onSaved();
    } catch (cause) {
      if (!disposedRef.current)
        setError(errorMessage(cause, t("plainTextConversion.errorFallback")));
    } finally {
      if (!disposedRef.current) setSaving(false);
    }
  }

  function cancel() {
    if (!busy) onCancel();
  }

  function options(values: string[]) {
    return values.map((value) => {
      return { label: t(`plainTextConversion.options.${value}`), value };
    });
  }

  function renderFooter() {
    return (
      <div className="flex flex-wrap justify-between gap-2">
        <Button disabled={busy} onClick={restoreDefaults}>
          {t("plainTextConversion.restoreDefaults")}
        </Button>
        <Space>
          <Button disabled={busy} onClick={cancel}>
            {t("common:actions.cancel")}
          </Button>
          <Button
            disabled={previewing}
            loading={saving}
            onClick={saveRules}
            type="primary"
          >
            {t("common:actions.save")}
          </Button>
        </Space>
      </div>
    );
  }

  function ruleKey(_index: number, rule: CustomReplacementRule) {
    return rule.id;
  }

  function renderRule(index: number, rule: CustomReplacementRule) {
    return (
      <div className="pb-3">
        <ReplacementRuleCard
          count={draft.rules.length}
          disabled={busy}
          index={index}
          onChange={changeRule}
          onDelete={deleteRule}
          onMove={moveRule}
          rule={rule}
        />
      </div>
    );
  }

  return (
    <Modal
      closable={!busy}
      footer={renderFooter()}
      keyboard={!busy}
      onCancel={cancel}
      open
      title={t("plainTextConversion.title")}
      width="min(48rem, calc(100vw - 2rem))"
    >
      <div className="max-h-[60vh] space-y-4 overflow-auto pe-2">
        <p className="text-ant-secondary text-sm">
          {t("plainTextConversion.description")}
        </p>
        {editor.diagnostics.length > 0 ? (
          <Alert
            description={
              <div className="space-y-2">
                <p>{editor.diagnostics.join("\n")}</p>
                <p>{t("plainTextConversion.recoveryHint")}</p>
                <Collapse
                  items={[
                    {
                      children: (
                        <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-xs">
                          {JSON.stringify(editor.recovery, null, 2)}
                        </pre>
                      ),
                      key: "raw",
                      label: t("plainTextConversion.recoveryRaw"),
                    },
                  ]}
                  size="small"
                />
              </div>
            }
            showIcon
            title={t("plainTextConversion.recoveryTitle")}
            type="warning"
          />
        ) : null}
        <Card size="small" title={t("plainTextConversion.builtinTitle")}>
          <Form disabled={busy} layout="vertical">
            <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
              <Form.Item label={t("plainTextConversion.linkStyle")}>
                <Select
                  aria-label={t("plainTextConversion.linkStyle")}
                  onChange={changeLinkStyle}
                  options={options(["title", "titleAndUrl", "url"])}
                  value={draft.linkStyle}
                />
              </Form.Item>
              <Form.Item label={t("plainTextConversion.imageStyle")}>
                <Select
                  aria-label={t("plainTextConversion.imageStyle")}
                  onChange={changeImageStyle}
                  options={options(["alt", "omit"])}
                  value={draft.imageStyle}
                />
              </Form.Item>
              <Form.Item label={t("plainTextConversion.listStyle")}>
                <Select
                  aria-label={t("plainTextConversion.listStyle")}
                  onChange={changeListStyle}
                  options={options(["textOnly", "readableMarkers"])}
                  value={draft.listStyle}
                />
              </Form.Item>
              <Form.Item label={t("plainTextConversion.taskStyle")}>
                <Select
                  aria-label={t("plainTextConversion.taskStyle")}
                  onChange={changeTaskStyle}
                  options={options(["status", "textOnly"])}
                  value={draft.taskStyle}
                />
              </Form.Item>
              <Form.Item label={t("plainTextConversion.tableSeparator")}>
                <Select
                  aria-label={t("plainTextConversion.tableSeparator")}
                  onChange={changeTableSeparator}
                  options={options(["tab", "space"])}
                  value={draft.tableSeparator}
                />
              </Form.Item>
              <Form.Item
                extra={t("plainTextConversion.collapseHint")}
                label={t("plainTextConversion.collapseBlankLines")}
              >
                <Switch
                  aria-label={t("plainTextConversion.collapseBlankLines")}
                  checked={draft.collapseBlankLines}
                  onChange={changeBlankLines}
                />
              </Form.Item>
            </div>
          </Form>
        </Card>
        <div className="space-y-3">
          <div className="flex items-center justify-between gap-2">
            <h3 className="font-medium text-base">
              {t("plainTextConversion.rulesTitle")}
            </h3>
            <Button
              disabled={busy || draft.rules.length >= MAX_CUSTOM_RULES}
              onClick={addRule}
            >
              {t("plainTextConversion.addRule")}
            </Button>
          </div>
          <p className="text-ant-secondary text-sm">
            {t("plainTextConversion.rulesHint")}
          </p>
          {draft.rules.length === 0 ? (
            <p className="text-ant-secondary text-sm">
              {t("plainTextConversion.emptyRules")}
            </p>
          ) : null}
          {draft.rules.length > 0 ? (
            <div className="h-104">
              <Virtuoso
                computeItemKey={ruleKey}
                data={draft.rules}
                itemContent={renderRule}
                ref={ruleListRef}
              />
            </div>
          ) : null}
          <p className="text-ant-secondary text-xs">
            {t("plainTextConversion.regexHint")}
          </p>
        </div>
        <Card size="small" title={t("plainTextConversion.previewTitle")}>
          <div className="space-y-3">
            <p className="text-ant-secondary text-sm">
              {t("plainTextConversion.previewHint")}
            </p>
            <Select
              aria-label={t("plainTextConversion.inputFormat")}
              disabled={busy}
              onChange={changeInputFormat}
              options={[
                { label: t("plainTextConversion.textInput"), value: "text" },
                { label: t("plainTextConversion.htmlInput"), value: "html" },
              ]}
              value={inputFormat}
            />
            <Input.TextArea
              aria-label={t("plainTextConversion.previewInput")}
              disabled={busy}
              onChange={changeInput}
              placeholder={t("plainTextConversion.previewInput")}
              rows={4}
              value={input}
            />
            <Button disabled={saving} loading={previewing} onClick={runPreview}>
              {t("plainTextConversion.previewAction")}
            </Button>
            {preview ? (
              <div className="space-y-2">
                <p className="text-sm">
                  {t("plainTextConversion.previewOutput")}
                  {!preview.changed
                    ? ` · ${t("plainTextConversion.previewUnchanged")}`
                    : ""}
                </p>
                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-2 border border-ant-border bg-ant-container p-3 text-sm">
                  {preview.output || t("plainTextConversion.previewEmpty")}
                </pre>
              </div>
            ) : null}
          </div>
        </Card>
        {error ? <Alert showIcon title={error} type="error" /> : null}
      </div>
    </Modal>
  );
};

interface ReplacementRuleCardProps {
  rule: CustomReplacementRule;
  index: number;
  count: number;
  disabled: boolean;
  onChange: (id: string, patch: Partial<CustomReplacementRule>) => void;
  onMove: (id: string, direction: -1 | 1) => void;
  onDelete: (id: string) => void;
}

/**
 * 单条替换规则输入保持真实空白，启停和排序只修改父级草稿。
 */
const ReplacementRuleCard: FC<ReplacementRuleCardProps> = (props) => {
  const { rule, index, count, disabled, onChange, onMove, onDelete } = props;
  const { t } = useTranslation(["preferences", "common"]);

  function changeName(event: ChangeEvent<HTMLInputElement>) {
    onChange(rule.id, { name: event.target.value });
  }
  function changeEnabled(checked: boolean) {
    onChange(rule.id, { enabled: checked });
  }
  function changeMode(mode: CustomReplacementRule["mode"]) {
    onChange(rule.id, { mode });
  }
  function changePattern(event: ChangeEvent<HTMLTextAreaElement>) {
    onChange(rule.id, { pattern: event.target.value });
  }
  function changeReplacement(event: ChangeEvent<HTMLTextAreaElement>) {
    onChange(rule.id, { replacement: event.target.value });
  }
  function moveUp() {
    onMove(rule.id, -1);
  }
  function moveDown() {
    onMove(rule.id, 1);
  }
  function remove() {
    onDelete(rule.id);
  }

  return (
    <Card
      extra={
        <Space wrap>
          <Button
            disabled={disabled || index === 0}
            onClick={moveUp}
            size="small"
          >
            {t("plainTextConversion.moveUp")}
          </Button>
          <Button
            disabled={disabled || index === count - 1}
            onClick={moveDown}
            size="small"
          >
            {t("plainTextConversion.moveDown")}
          </Button>
          <Button danger disabled={disabled} onClick={remove} size="small">
            {t("common:actions.delete")}
          </Button>
        </Space>
      }
      size="small"
      title={t("plainTextConversion.ruleTitle", { index: index + 1 })}
    >
      <Form disabled={disabled} layout="vertical">
        <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
          <Form.Item label={t("plainTextConversion.ruleName")}>
            <Input
              aria-label={t("plainTextConversion.ruleName")}
              onChange={changeName}
              value={rule.name}
            />
          </Form.Item>
          <Form.Item label={t("plainTextConversion.enabled")}>
            <Switch
              aria-label={t("plainTextConversion.enabled")}
              checked={rule.enabled}
              onChange={changeEnabled}
            />
          </Form.Item>
          <Form.Item label={t("plainTextConversion.mode")}>
            <Select
              aria-label={t("plainTextConversion.mode")}
              onChange={changeMode}
              options={[
                { label: t("plainTextConversion.literal"), value: "literal" },
                { label: t("plainTextConversion.regex"), value: "regex" },
              ]}
              value={rule.mode}
            />
          </Form.Item>
        </div>
        <Form.Item
          extra={t("plainTextConversion.patternHint")}
          label={t("plainTextConversion.pattern")}
        >
          <Input.TextArea
            aria-label={t("plainTextConversion.pattern")}
            onChange={changePattern}
            rows={2}
            value={rule.pattern}
          />
        </Form.Item>
        <Form.Item
          extra={t("plainTextConversion.replacementHint")}
          label={t("plainTextConversion.replacement")}
        >
          <Input.TextArea
            aria-label={t("plainTextConversion.replacement")}
            onChange={changeReplacement}
            rows={2}
            value={rule.replacement}
          />
        </Form.Item>
      </Form>
    </Card>
  );
};

/**
 * 命令包装已展示错误 toast，这里仅保留可读根因供编辑草稿时定位。
 */
function errorMessage(error: unknown, fallback: string): string {
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string"
  )
    return error.message;

  return fallback;
}

export default PlainTextConversionModal;
