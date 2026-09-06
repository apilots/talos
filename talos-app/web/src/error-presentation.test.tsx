// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { TalosApiError } from "@talos/sdk-browser";
import { i18n } from "@/shared/i18n/setup";
import { SidebarSessionDeleteDialog } from "@/widgets/sidebar/SidebarSessionDeleteDialog";
import { ProjectDirectoryDialog } from "@/features/projects/components/ProjectDirectoryDialog";
import { ComposerAttachmentTray } from "@/features/chat/components/ComposerAttachmentTray";
import { ComposerCommandResponse } from "@/features/chat/components/command-response/ComposerCommandResponse";

const failure = new TalosApiError({
  code: "providerUnavailable", message: "The selected model is absent from the provider configuration.",
  localization: { type: "modelNotConfigured", provider: "local", model: "alpha" },
});
const reject = async () => { throw failure; };
const noop = () => undefined;

afterEach(async () => { cleanup(); await i18n.changeLanguage("en"); });

it.each([
  ["sidebar operation", async () => {
    render(<SidebarSessionDeleteDialog deleting={false} sessionTitle="example" onClose={noop} onDelete={reject} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  }],
  ["directory request", async () => {
    render(<ProjectDirectoryDialog mode="existing" onClose={noop} onCreateProject={reject}
      readDirectory={reject} createDirectory={reject} />);
  }],
  ["failed attachment", async () => {
    render(<ComposerAttachmentTray disabled={false} error={null} onRemove={noop}
      items={[{ id: "upload", status: "failed", file: new File([], "upload.txt"), error: failure.apiError }]} />);
  }],
  ["model command", async () => {
    render(<ComposerCommandResponse response={{ kind: "model", model: "alpha", state: "error", error: failure.apiError }} />);
  }],
] as const)("%s renders stored failures in the current locale", async (_name, show) => {
  await show();
  await waitFor(() => expect(document.body.textContent).toContain('Model "alpha" is not configured for provider "local".'));
  await act(() => i18n.changeLanguage("zh-CN"));
  expect(document.body.textContent).toContain('提供方“local”未配置模型“alpha”。');
});
