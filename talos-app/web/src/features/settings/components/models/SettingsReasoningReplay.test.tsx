// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { installRadixTestEnvironment, selectOption } from "./settingsTestUtils";

import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { LlmProtocol, ProviderConfig } from "@talos/sdk-browser";
import "@/shared/i18n/setup";
import { SettingsProviderModels } from "./SettingsProviderModels";
import { emptyModelForm, formToModel } from "./modelForm";

installRadixTestEnvironment();

afterEach(cleanup);

it.each([{ custom: false, protocol: "chat" }, { custom: true, protocol: "messages" }, { custom: true, protocol: "responses" }] as const)("hides replay overrides for custom=$custom / $protocol and blocks carrying a hidden nondefault override", ({ custom, protocol }) => {
  const onChange = vi.fn();
  const config = { protocol, models: [{ ...formToModel({ ...emptyModelForm, id: "model" }), reasoning_replay: "omit" as const }] };
  render(<SettingsProviderModels config={config} custom={custom} capabilities={{ default_protocol: protocol, protocols: [protocol] }} keyConfigured={false} onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit Model" }));
  fireEvent.click(screen.getByText("Advanced"));
  expect(screen.queryByRole("combobox", { name: "Reasoning history" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(screen.getByRole("alert")).toHaveTextContent(/only for a custom Chat provider/);
  expect(onChange).not.toHaveBeenCalled();
});

it("saves a custom Chat replay override as a typed model field and keeps cost and KV intact", async () => {
  const original = { ...formToModel({ ...emptyModelForm, id: "model" }), cost: { input: 2, output: 4, cache_read: 1, cache_write: 3 }, request_parameters: { chat_template_kwargs: { enable_thinking: false } } };
  const changed = vi.fn();
  function Harness() {
    const [config, setConfig] = useState<ProviderConfig>({ protocol: "chat", models: [original] });
    return <SettingsProviderModels config={config} custom capabilities={{ default_protocol: "chat", protocols: ["chat", "messages", "responses"] as LlmProtocol[] }} keyConfigured={false} onChange={(patch) => { changed(patch); setConfig({ ...config, ...patch }); }} />;
  }
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "Edit Model" }));
  expect(screen.getByText("Advanced").closest("details")).not.toHaveAttribute("open");
  fireEvent.click(screen.getByText("Advanced"));
  await selectOption(screen.getByRole("combobox", { name: "Reasoning history" }), "Replay as reasoning");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(changed).toHaveBeenCalledExactlyOnceWith({ models: [{ ...original, reasoning_replay: "reasoning" }] });
});
