import { createInstance } from "i18next";
import { expect, it } from "vitest";
import { computerResources } from "./computerResources";
import { computerTopologyResources } from "./computerTopologyResources";

it("covers every topology label in both locales, excluding language-specific plural forms", () => {
  const baseKeys = (dictionary: Record<string, string>) =>
    Object.keys(dictionary)
      .filter((key) => !key.endsWith("_one"))
      .sort();
  expect(baseKeys(computerTopologyResources.en)).toEqual(
    baseKeys(computerTopologyResources["zh-CN"]),
  );
  for (const dictionary of Object.values(computerTopologyResources)) {
    expect(Object.values(dictionary).every((value) => value.trim())).toBe(true);
  }
});

it.each(["en", "zh-CN"])(
  "renders singular/plural and interpolated account observations (%s)",
  async (locale) => {
    const i18n = createInstance();
    await i18n.init({
      lng: locale,
      resources: {
        [locale]: {
          computers:
            computerResources[locale as keyof typeof computerResources],
        },
      },
      defaultNS: "computers",
      interpolation: { escapeValue: false },
    });
    expect(i18n.t("unmappedPeers", { count: 1 })).toBe(
      locale === "en"
        ? "1 peer has no Computer mapping."
        : "另有 1 个同步端没有 Computer 映射。",
    );
    expect(i18n.t("unmappedPeers", { count: 2 })).toBe(
      locale === "en"
        ? "2 peers have no Computer mapping."
        : "另有 2 个同步端没有 Computer 映射。",
    );
    expect(i18n.t("inspectComputer", { name: "My Computer" })).toContain(
      "My Computer",
    );
  },
);
