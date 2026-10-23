// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { installRadixTestEnvironment, selectOption, chooseExample } from "./settingsTestUtils";

import { useState } from "react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { resources } from "@/shared/i18n/resources";
import { CustomProviderModal } from "./CustomProviderModal";
import { RequestParametersEditor } from "./RequestParametersEditor";
import { SettingsModelDeleteDialog } from "./SettingsModelDeleteDialog";
import { SettingsModelModal } from "./SettingsModelModal";
import { emptyModelForm, formToModel } from "./modelForm";

installRadixTestEnvironment();

afterEach(cleanup);

it("keeps model configuration locale keys and interpolation parameters aligned", () => {
  const english = resources.en.settings.modelsPanel;
  const chinese = resources["zh-CN"].settings.modelsPanel;
  expect(Object.keys(chinese).sort()).toEqual(Object.keys(english).sort());
  for (const key of Object.keys(english) as Array<keyof typeof english>) {
    expect(chinese[key].trim()).not.toBe("");
    expect(chinese[key].match(/{{[^}]+}}/g)).toEqual(english[key].match(/{{[^}]+}}/g));
  }
});

it.each(["en", "zh-CN"] as const)("renders custom connection, KV and deletion text and aria in %s", async (locale) => {
  const chinese = locale === "zh-CN";
  const instance = createInstance();
  await instance.init({ resources, lng: locale, fallbackLng: false, interpolation: { escapeValue: false } });
  const modal = render(<I18nextProvider i18n={instance}><CustomProviderModal onClose={vi.fn()} onSave={vi.fn()} /></I18nextProvider>);
  expect(screen.getByRole("dialog", { name: chinese ? "新增 Provider" : "Add Provider" })).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText(chinese ? "显示名称" : "Display name"), { target: { value: "LLM Gateway" } });
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "javascript:invalid" } });
  expect(screen.getByRole("combobox", { name: chinese ? "接口格式" : "API format" })).toHaveTextContent(chinese ? "聊天补全" : "Chat Completions");
  expect(screen.getByLabelText(chinese ? "API Key （可选）" : "API Key (optional)")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: chinese ? "保存" : "Save" }));
  expect(screen.getByRole("alert")).toHaveTextContent(chinese ? "请填写名称及有效的 HTTP(S) Base URL" : "Enter a name and a valid HTTP(S) Base URL");
  const otherLocale = chinese ? "en" : "zh-CN";
  await act(() => instance.changeLanguage(otherLocale).then(() => undefined));
  expect(screen.getByRole("alert")).toHaveTextContent(chinese ? "Enter a name and a valid HTTP(S) Base URL" : "请填写名称及有效的 HTTP(S) Base URL");
  modal.unmount();
  await instance.changeLanguage(locale);

  const parameters = render(<I18nextProvider i18n={instance}><RequestParametersEditor rows={[{ key: "thinking_budget", value: "2048" }]} onChange={vi.fn()} /></I18nextProvider>);
  fireEvent.click(screen.getByText(chinese ? "高级配置" : "Advanced"));
  expect(screen.getByText(chinese ? "参数名" : "Key")).toBeInTheDocument();
  expect(screen.getByText(chinese ? "参数值 · JSON" : "Value · JSON")).toBeInTheDocument();
  expect(screen.getByLabelText(chinese ? "参数名 1" : "Parameter key 1")).toHaveValue("thinking_budget");
  expect(screen.getByLabelText(chinese ? "参数值 1" : "Parameter value 1")).toHaveValue("2048");
  expect(screen.getByRole("button", { name: chinese ? "删除参数 1" : "Remove parameter 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: chinese ? "填入示例" : "Fill example" })).toBeInTheDocument();
  parameters.unmount();

  for (const kind of ["provider", "model"] as const) {
    const deletion = render(<I18nextProvider i18n={instance}><SettingsModelDeleteDialog kind={kind} name="LLM Gateway" fallbackFocusRef={{ current: null }} onClose={vi.fn()} onConfirm={vi.fn()} /></I18nextProvider>);
    const title = kind === "provider"
      ? chinese ? "删除 Provider“LLM Gateway”？" : "Delete Provider “LLM Gateway”?"
      : chinese ? "删除模型“LLM Gateway”？" : "Delete model “LLM Gateway”?";
    const dialog = screen.getByRole("alertdialog", { name: title });
    expect(dialog).toHaveAccessibleDescription(chinese ? /保存更改后生效/ : /Save changes to apply/);
    expect(within(dialog).getByRole("button", { name: chinese ? "关闭" : "Close" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: chinese ? "取消" : "Cancel" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: chinese ? "删除" : "Delete" })).toBeInTheDocument();
    deletion.unmount();
  }
});

it.each(["en", "zh-CN"] as const)("localizes history policy and inserts only the selected thinking parameter shape in %s", async (locale) => {
  const chinese = locale === "zh-CN";
  const instance = createInstance();
  await instance.init({ resources, lng: locale, fallbackLng: false, interpolation: { escapeValue: false } });
  const saved = vi.fn();
  function Harness() {
    const [form, setForm] = useState({ ...emptyModelForm, id: "local-model" });
    return <SettingsModelModal allowReasoningReplay mode="add" form={form} saving={false} error="" onChange={setForm} onSave={() => saved(formToModel(form))} onClose={vi.fn()} />;
  }
  render(<I18nextProvider i18n={instance}><Harness /></I18nextProvider>);
  fireEvent.click(screen.getByText(chinese ? "高级配置" : "Advanced"));
  const history = screen.getByRole("combobox", { name: chinese ? "思考历史" : "Reasoning history" });
  expect(history).toHaveAccessibleDescription(chinese ? /仅适用于自定义 Chat.*不是思考开关或请求参数/ : /Custom Chat only.*not the thinking switch or request parameters/);
  expect(history).toHaveTextContent(chinese ? "遵循协议默认" : "Protocol default");
  await selectOption(history, chinese ? "使用 reasoning_content 回传" : "Replay as reasoning_content");
  await chooseExample(chinese ? "填入示例" : "Fill example", "vLLM / SGLang");
  fireEvent.click(screen.getByRole("button", { name: chinese ? "保存" : "Save" }));
  expect(saved.mock.calls[0][0]).toEqual(expect.objectContaining({ reasoning_replay: "reasoning_content", request_parameters: { chat_template_kwargs: { enable_thinking: true } } }));
  fireEvent.click(screen.getByRole("button", { name: chinese ? "删除参数 1" : "Remove parameter 1" }));
  await chooseExample(chinese ? "填入示例" : "Fill example", chinese ? "百炼" : "Bailian");
  fireEvent.click(screen.getByRole("button", { name: chinese ? "保存" : "Save" }));
  expect(saved.mock.calls[1][0].request_parameters).toEqual({ enable_thinking: true });
  expect(saved.mock.calls[1][0].request_parameters).not.toHaveProperty("reasoning_replay");
});
