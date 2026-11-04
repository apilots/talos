import type { ChatSlice, ChatSliceSetter } from "./store.types";

type ChatTransaction = { state: ChatSlice; patch: Partial<ChatSlice>; commit?: ChatSliceSetter };
let transaction: ChatTransaction | undefined;

/** Apply ordered Chat mutations with one subscriber/persistence commit. */
export function batchChatUpdates(state: ChatSlice, apply: () => void) {
  if (transaction) {
    apply();
    return;
  }
  const current: ChatTransaction = { state, patch: {} };
  transaction = current;
  try {
    apply();
  } finally {
    transaction = undefined;
    current.commit?.(() => current.patch);
  }
}

export function chatBatchState(): ChatSlice | undefined {
  return transaction?.state;
}

export function createBatchedChatSetter(commit: ChatSliceSetter): ChatSliceSetter {
  return (updater) => {
    if (!transaction) {
      commit(updater);
      return;
    }
    const patch = updater(transaction.state);
    transaction.state = { ...transaction.state, ...patch };
    Object.assign(transaction.patch, patch);
    transaction.commit = commit;
  };
}
