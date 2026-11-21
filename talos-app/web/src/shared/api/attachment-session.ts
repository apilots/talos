import { createContext } from "react";

/** The conversation whose management Server owns files shown in this chat. */
export const AttachmentSessionContext = createContext<string | undefined>(undefined);
