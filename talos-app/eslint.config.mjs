import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: ["**/dist/**", "**/node_modules/**"],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
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
    // shared/store is intentionally excluded here because app-store is the composition point
    // that wires feature-owned Zustand slices into a single persisted store.
    files: [
      "web/src/shared/api/**/*.{ts,tsx}",
      "web/src/shared/hooks/**/*.{ts,tsx}",
      "web/src/shared/lib/**/*.{ts,tsx}",
      "web/src/shared/sse/**/*.{ts,tsx}",
      "web/src/shared/types/**/*.{ts,tsx}",
      "web/src/shared/ui/**/*.{ts,tsx}",
    ],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@/features/*"],
              message: "Shared layer must not import feature modules.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["web/src/widgets/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@/features/*"],
              message: "Widgets must receive feature data via props or containers.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["web/src/app/**/*.{ts,tsx}", "web/src/pages/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@/features/*/*"],
              message: "Import feature APIs from the feature barrel only.",
            },
          ],
        },
      ],
    },
  },
);
