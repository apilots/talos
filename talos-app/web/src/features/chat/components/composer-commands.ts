export const COMPOSER_COMMANDS = ["compact", "clear", "status", "model"] as const;

export type ComposerCommand = (typeof COMPOSER_COMMANDS)[number];

export function filterComposerCommands(input: string): ComposerCommand[] {
  const value = input.trimEnd();
  if (!value.startsWith("/") || /\s/.test(value)) {
    return [];
  }
  const query = value.slice(1).toLowerCase();
  return COMPOSER_COMMANDS.filter((command) => command.startsWith(query));
}
