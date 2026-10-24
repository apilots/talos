/** Shared light scrims; frame geometry and dismissal remain owned by each dialog. */
export const overlayClasses = {
  tint: "bg-overlay backdrop-blur-[1px]",
  native: "backdrop:bg-overlay backdrop:backdrop-blur-[1px]",
} as const;

/** Document alignment does not constrain full-viewport scrim coverage. */
export const documentOverlayClasses = {
  frame: "fixed inset-y-0 left-0 right-0 z-[80] flex items-center justify-center px-6 py-8 lg:left-[280px]",
  backdrop: `fixed inset-0 cursor-default ${overlayClasses.tint}`,
} as const;
