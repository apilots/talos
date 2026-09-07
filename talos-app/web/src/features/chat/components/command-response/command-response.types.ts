import type {
  ContextCompactionOutcome,
  ContextCompactionStage,
} from "@/shared/types/protocol";
import type { ReasoningLevel } from "@/shared/types/ui";
import type { AppProtocol } from "@talos/sdk-browser";

export interface ComposerStatusSnapshot {
  model: string;
  project?: string;
  reasoningLevel?: ReasoningLevel;
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
  | {
      kind: "compact";
      error?: AppProtocol.ApiError;
      outcome?: ContextCompactionOutcome;
      state: "success" | "error" | "cancelled";
    }
  | { kind: "clear"; error?: AppProtocol.ApiError; state: "success" | "error" }
  | {
      kind: "model";
      error?: AppProtocol.ApiError;
      model: string;
      reasoningLevel?: ReasoningLevel;
      state: "success" | "error";
    };
