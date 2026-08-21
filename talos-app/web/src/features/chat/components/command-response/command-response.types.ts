import type { ContextCompactionStage } from "@/shared/types/protocol";
import type { ReasoningLevel, RuntimeMode } from "@/shared/types/ui";

export interface ComposerStatusSnapshot {
  model: string;
  project?: string;
  reasoningLevel?: ReasoningLevel;
  runtimeMode: RuntimeMode;
  sessionId: string;
  state: "ready" | "running";
}

export type ComposerCommandResponseState =
  | { kind: "status" }
  | {
      kind: "compact";
      stage?: ContextCompactionStage;
      state: "running";
    }
  | { kind: "compact"; message?: string; state: "success" | "error" }
  | { kind: "clear"; message?: string; state: "success" | "error" }
  | {
      kind: "model";
      message?: string;
      model: string;
      reasoningLevel?: ReasoningLevel;
      state: "success" | "error";
    };
