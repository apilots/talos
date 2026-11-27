import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import jsxA11y from "eslint-plugin-jsx-a11y";
import query from "@tanstack/eslint-plugin-query";
import vitest from "@vitest/eslint-plugin";
import testingLibrary from "eslint-plugin-testing-library";
import { errorPresentationRule } from "./scripts/error-presentation-rule.mjs";

export default tseslint.config(
  {
    ignores: ["**/dist/**", "**/node_modules/**"],
  },
  js.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    extends: tseslint.configs.recommendedTypeChecked,
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/switch-exhaustiveness-check": "error",
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXText[value=/[\\u4E00-\\u9FFF]/]",
          message: "Use i18n translations instead of hardcoded CJK text in JSX.",
        },
      ],
    },
  },
  {
    files: ["web/src/**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks, "jsx-a11y": jsxA11y, "@tanstack/query": query },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      "jsx-a11y/alt-text": "error",
      "jsx-a11y/aria-props": "error",
      "jsx-a11y/aria-proptypes": "error",
      "jsx-a11y/aria-unsupported-elements": "error",
      "jsx-a11y/role-has-required-aria-props": "error",
      "@tanstack/query/stable-query-client": "error",
      "@tanstack/query/no-unstable-deps": "error",
      "@tanstack/query/exhaustive-deps": "error",
    },
  },
  {
    files: ["**/*.test.{ts,tsx}", "**/__tests__/**/*.{ts,tsx}"],
    plugins: { vitest, "testing-library": testingLibrary },
    rules: {
      "@typescript-eslint/unbound-method": "off",
      "vitest/unbound-method": "error",
      "vitest/no-focused-tests": "error",
      "testing-library/await-async-queries": "error",
      "testing-library/await-async-utils": "error",
      "testing-library/no-wait-for-side-effects": "error",
    },
  },
  {
    files: ["web/src/**/*.{ts,tsx}"],
    ignores: ["**/*.test.{ts,tsx}", "**/__tests__/**", "web/src/shared/i18n/apiErrorMessage.ts"],
    plugins: { talos: { rules: { "error-presentation": errorPresentationRule } } },
    rules: { "talos/error-presentation": "error" },
  },
);
