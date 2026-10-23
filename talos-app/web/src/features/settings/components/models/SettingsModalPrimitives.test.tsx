// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { StrictMode, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { SettingsModelModal } from "./SettingsModelModal";
import { emptyModelForm, formToModel, modelToForm } from "./modelForm";
import { installRadixTestEnvironment } from "./settingsTestUtils";

installRadixTestEnvironment();
afterEach(cleanup);

it("closes the nested Select before the model dialog and restores each user trigger", async () => {
  const user = userEvent.setup();
  function Harness() {
    const [open, setOpen] = useState(false);
    const [form, setForm] = useState({ ...emptyModelForm, id: "model", reasoning: true });
    return <><button onClick={() => setOpen(true)}>Edit model</button>{open && <SettingsModelModal allowReasoningReplay mode="edit" form={form} saving={false} error="" onChange={setForm} onClose={() => setOpen(false)} onSave={vi.fn()} />}</>;
  }
  render(<StrictMode><Harness /></StrictMode>);
  const trigger = screen.getByRole("button", { name: "Edit model" });
  await user.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "Edit Model" });
  const first = within(dialog).getAllByRole("textbox").find((element) => !element.hasAttribute("disabled"));
  expect(first).toHaveFocus();
  await user.click(screen.getByText("Advanced"));
  const history = screen.getByRole("combobox", { name: "Reasoning history" });
  await user.click(history);
  expect(await screen.findByRole("listbox")).toBeInTheDocument();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(dialog).toBeInTheDocument();
  expect(history).toHaveFocus();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  await waitFor(() => expect(trigger).toHaveFocus());
});

it("keeps an existing example's full JSON and sibling values until the user explicitly edits or removes it", async () => {
  const user = userEvent.setup();
  const object = { enable_thinking: false, sibling: { tags: ["保留", "quoted \"value\""], nullable: null, nested: { mode: "precise" } } };
  const raw = JSON.stringify(object, null, 2);
  const original = { ...formToModel({ ...emptyModelForm, id: "model" }), cost: { input: 3, output: 5, cache_read: 1, cache_write: 2 }, request_parameters: { chat_template_kwargs: object } };
  const saved = vi.fn();
  const changed = vi.fn();
  function Harness() {
    const [form, setForm] = useState({ ...modelToForm(original), request_parameters: [{ key: " chat_template_kwargs ", value: raw }] });
    return <SettingsModelModal allowReasoningReplay mode="edit" form={form} saving={false} error="" onChange={(next) => { changed(next); setForm(next); }} onSave={() => saved(formToModel(form, original))} onClose={vi.fn()} />;
  }
  render(<Harness />);
  await user.click(screen.getByText("Advanced"));
  const value = screen.getByRole("textbox", { name: "Parameter value 1" });
  expect(value.tagName).toBe("TEXTAREA");
  expect(value).toHaveValue(raw);
  expect(value).toHaveAttribute("wrap", "soft");
  await user.click(screen.getByRole("button", { name: "Fill example" }));
  const example = await screen.findByRole("menuitem", { name: "vLLM / SGLang" });
  expect(example).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByText("Already set — edit or remove the current row.")).toBeInTheDocument();
  await user.click(example);
  expect(changed).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(value).toHaveValue(raw);
  const edited = `${raw}\n`;
  fireEvent.change(value, { target: { value: edited } });
  expect(value).toHaveValue(edited);
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(saved).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ cost: original.cost, request_parameters: { chat_template_kwargs: object } }));
});
