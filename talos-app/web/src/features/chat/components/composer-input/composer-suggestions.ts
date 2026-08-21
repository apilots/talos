import type { SkillSuggestion } from "@talos/sdk-browser";

export const SESSION_COMMAND_IDS = ["compact", "clear", "status"] as const;
export const COMPOSER_COMMAND_IDS = [...SESSION_COMMAND_IDS, "model"] as const;

export type ComposerCommandId = (typeof COMPOSER_COMMAND_IDS)[number];
export type ComposerSessionCommandId = (typeof SESSION_COMMAND_IDS)[number];

export interface ComposerCommandSuggestion {
  type: "command";
  id: ComposerCommandId;
}

export interface ComposerSkillSuggestion {
  type: "skill";
  skill: SkillSuggestion;
}

export type ComposerSuggestion = ComposerCommandSuggestion | ComposerSkillSuggestion;

export function availableComposerCommands({
  hasModels,
  hasSessionCommands,
}: {
  hasModels: boolean;
  hasSessionCommands: boolean;
}): ComposerCommandId[] {
  return [
    ...(hasSessionCommands ? SESSION_COMMAND_IDS : []),
    ...(hasModels ? (["model"] as const) : []),
  ];
}

export function filterComposerCommands(
  commands: readonly ComposerCommandId[],
  query: string,
): ComposerCommandSuggestion[] {
  const normalizedQuery = query.trim().toLowerCase();
  return commands
    .filter((command) => command.startsWith(normalizedQuery))
    .map((id) => ({ type: "command", id }));
}
