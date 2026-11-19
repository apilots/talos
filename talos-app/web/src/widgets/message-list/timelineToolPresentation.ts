import type { TFunction } from "i18next";
import { FilePenLine, Globe, Search, BookOpen, Terminal, Wrench } from "lucide-react";

import { buildToolDisplayTitle, toolActivity, toolDisplayTarget } from "./toolDisplay";

const icons = {
  commands: Terminal,
  edit: FilePenLine,
  lookup: Search,
  read: BookOpen,
  search: Search,
  web: Globe,
};

export function timelineToolIcon(toolName: string) {
  const activity = toolActivity(toolName);
  return activity ? icons[activity] : Wrench;
}

export function timelineToolTitle(toolName: string, input: unknown, t: TFunction<"chat">) {
  const activity = toolActivity(toolName);
  const label = toolName === "WebSearch" ? "search" : activity;
  if (label === "read" || label === "edit" || label === "search" || label === "web") {
    const target = toolDisplayTarget(toolName, input, label === "read" || label === "edit");
    if (target) return t(`timelineToolTarget.${label}`, { target });
    return t(`timelineActivity.${activity}`);
  }
  return buildToolDisplayTitle(toolName, input);
}
