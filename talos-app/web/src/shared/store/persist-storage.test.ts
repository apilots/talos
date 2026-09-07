// @vitest-environment jsdom

import { beforeEach, expect, it } from "vitest";
import { createPersistStorage } from "./persist-storage";

beforeEach(() => window.localStorage.clear());

it("uses browser storage with the complete DOM storage contract", () => {
  const storage = createPersistStorage();
  expect(storage).toBeInstanceOf(window.Storage);
  storage.setItem("persist-fixture", "saved");
  expect([storage.length, storage.key(0), storage.getItem("persist-fixture")])
    .toEqual([1, "persist-fixture", "saved"]);
  storage.removeItem("persist-fixture");
  expect(storage.getItem("persist-fixture")).toBeNull();
});
