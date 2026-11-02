// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom/vitest";
import { installRadixTestEnvironment, selectOption, chooseExample } from "./settingsTestUtils";

import { StrictMode, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { CustomProviderModal } from "./CustomProviderModal";
import { CustomProviderMenu } from "./CustomProviderMenu";
import { SettingsModelModal } from "./SettingsModelModal";
import { emptyModelForm, formToModel, modelToForm, type ModelFormState } from "./modelForm";
import { parseRequestParameters } from "./requestParameters";


installRadixTestEnvironment();

afterEach(cleanup);

describe("custom provider configuration", () => {
  it("keeps provider creation separate from models and restores focus after Escape", async () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return <><CustomProviderMenu providers={{}} selected="openai" onSelect={() => {}} onAdd={() => setOpen(true)} />{open && <CustomProviderModal onSave={() => {}} onClose={() => setOpen(false)} />}</>;
    }
    render(<StrictMode><Harness /></StrictMode>);
    expect(document.activeElement).toBe(document.body);
    await userEvent.click(screen.getByRole("button", { name: "Custom" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Add Provider" }));
    expect(screen.getByLabelText("Display name")).toHaveFocus();
    expect(screen.queryByText("Context window")).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Custom" })).toHaveFocus());
  });

  it("creates an optional-auth connection with an explicit Chat protocol", () => {
    const onSave = vi.fn();
    render(<CustomProviderModal onSave={onSave} onClose={() => {}} />);
    fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "LLM Gateway" } });
    fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "http://localhost:8000/v1" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledWith({ name: "LLM Gateway", protocol: "chat", base_url: "http://localhost:8000/v1", api_key: "", models: [] });
  });

  it.each(["chat", "messages", "responses"] as const)("submits only the selected %s protocol without brand adapters", async (protocol) => {
    const onSave = vi.fn();
    render(<CustomProviderModal onSave={onSave} onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    const format = screen.getByLabelText("API format");
    await selectOption(format, { chat: "Chat Completions", messages: "Messages", responses: "Responses" }[protocol]);
    fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "Local Inference" } });
    fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "http://localhost:8000/v1" } });
    fireEvent.submit(screen.getByRole("button", { name: "Save" }).closest("form")!);
    expect(onSave).toHaveBeenCalledWith({ name: "Local Inference", protocol, base_url: "http://localhost:8000/v1", api_key: "", models: [] });
  });

  it("blocks a protocol edit that would strand a custom Chat reasoning history override", async () => {
    const onSave = vi.fn();
    const model = { ...formToModel({ ...emptyModelForm, id: "model", name: "Local model" }), reasoning_replay: "reasoning_content" as const };
    render(<CustomProviderModal initial={{ name: "LLM Gateway", protocol: "chat", base_url: "http://localhost:8000/v1", api_key: "", models: [model] }} onSave={onSave} onClose={() => {}} />);
    await selectOption(screen.getByLabelText("API format"), "Messages");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/Local model.*Protocol default/);
    expect(onSave).not.toHaveBeenCalled();
    await selectOption(screen.getByLabelText("API format"), "Chat Completions");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ protocol: "chat", models: [model] }));
  });

  it("uses a real modal focus scope and preserves focus moved elsewhere after unmount", async () => {
    const { unmount } = render(<StrictMode><CustomProviderModal onSave={() => {}} onClose={() => {}} /></StrictMode>);
    const close = screen.getByRole("button", { name: "Close" });
    expect(screen.getByLabelText("Display name")).toHaveFocus();
    close.focus();
    await userEvent.tab();
    expect(screen.getByLabelText("Display name")).toHaveFocus();
    const elsewhere = document.createElement("button");
    document.body.append(elsewhere);
    unmount();
    elsewhere.focus();
    await waitFor(() => expect(elsewhere).toHaveFocus());
    elsewhere.remove();
  });

  it("reuses the model form, leaves advanced collapsed, and fills only requested KV examples", async () => {
    const saved = vi.fn();
    function Harness() {
      const [form, setForm] = useState({ ...emptyModelForm, id: "model" });
      return <SettingsModelModal mode="add" form={form} saving={false} error="" onChange={setForm} onSave={() => saved(formToModel(form))} onClose={() => {}} />;
    }
    render(<Harness />);
    const reasoning = screen.getByRole("combobox", { name: "Reasoning effort" });
    expect(reasoning).toBeDisabled();
    await userEvent.click(reasoning);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByText("Advanced").closest("details")).not.toHaveAttribute("open");
    expect(screen.queryByLabelText("Parameter key 1")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("Advanced"));
    await chooseExample("Fill example", "vLLM / SGLang");
    expect(screen.getByLabelText("Parameter key 1")).toHaveValue("chat_template_kwargs");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(saved.mock.calls[0][0].request_parameters).toEqual({ chat_template_kwargs: { enable_thinking: true } });
  });

  it("preserves hidden cost fields and validates JSON types, duplicates and runtime-owned names", () => {
    const original = { ...formToModel({ ...emptyModelForm, id: "model" }), cost: { input: 2, output: 5, cache_read: 1, cache_write: 3 } };
    const form = modelToForm(original);
    form.name = "Edited";
    form.request_parameters = [{ key: "thinking_budget", value: "2048" }, { key: "chat_template_kwargs", value: '{"enable_thinking":false}' }];
    expect(formToModel(form, original)).toMatchObject({ name: "Edited", cost: original.cost, request_parameters: { thinking_budget: 2048, chat_template_kwargs: { enable_thinking: false } } });
    expect(parseRequestParameters([{ key: "messages", value: "[]" }]).error).toBe("reserved");
    expect(parseRequestParameters([{ key: "reasoning_replay", value: '"omit"' }]).error).toBe("reserved");
    expect(parseRequestParameters([{ key: "x", value: "true" }, { key: "x", value: "false" }]).error).toBe("duplicate");
    expect(parseRequestParameters([{ key: "x", value: "not JSON" }]).error).toBe("invalidJson");
  });
});

it("preserves HTTP deadlines when editing a connection and explicit efforts when editing a model", () => {
  const onSave = vi.fn();
  const http_timeouts = { connect_secs: 5, response_secs: 120, stream_idle_secs: 60, stream_total_secs: 600 };
  render(<CustomProviderModal initial={{ name: "Local", protocol: "chat", base_url: "http://localhost:8000/v1", models: [], http_timeouts }} onSave={onSave} onClose={() => {}} />);
  fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "Edited" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ name: "Edited", http_timeouts }));
  const original = { ...formToModel({ ...emptyModelForm, id: "opaque", reasoning: true }), reasoning_efforts: ["light", "medium", "xhigh"] as const };
  const model = { ...original, reasoning_efforts: [...original.reasoning_efforts] };
  expect(formToModel(modelToForm(model), model).reasoning_efforts).toEqual(["light", "medium", "xhigh"]);
});

it("edits accepted effort levels independently of the thinking parameter", async () => {
  const saved = vi.fn();
  function Harness() {
    const [form, setForm] = useState({ ...emptyModelForm, id: "opaque", reasoning: true, request_parameters: [{ key: "enable_thinking", value: "true" }] });
    return <SettingsModelModal mode="edit" form={form} saving={false} error="" onChange={setForm} onSave={() => saved(formToModel(form))} onClose={() => {}} />;
  }
  render(<Harness />);
  fireEvent.click(screen.getByText("Advanced"));
  await userEvent.click(screen.getByRole("checkbox", { name: "Restrict accepted effort levels" }));
  await userEvent.click(screen.getByRole("checkbox", { name: /^High$/ }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(saved).toHaveBeenCalledWith(expect.objectContaining({ reasoning_efforts: ["light", "medium", "xhigh"], request_parameters: { enable_thinking: true } }));
});

it("selects a default from declared effort choices", async () => {
  const saved = vi.fn();
  function Harness() {
    const [form, setForm] = useState<ModelFormState>({ ...emptyModelForm, id: "opaque", reasoning: true, reasoning_efforts: ["light", "medium", "xhigh"] });
    return <SettingsModelModal mode="edit" form={form} saving={false} error="" onChange={setForm} onSave={() => saved(formToModel(form))} onClose={() => {}} />;
  }
  render(<Harness />);
  await userEvent.click(screen.getByRole("combobox", { name: "Reasoning effort" }));
  expect(screen.getAllByRole("option")).toHaveLength(3);
  await userEvent.click(screen.getByRole("option", { name: /^Light$/ }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(saved).toHaveBeenCalledWith(expect.objectContaining({ reasoning_effort: "light", reasoning_efforts: ["light", "medium", "xhigh"] }));
});
