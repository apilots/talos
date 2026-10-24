import type { Config } from "tailwindcss";
import typography from "@tailwindcss/typography";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        overlay: "rgb(var(--color-overlay) / var(--overlay-opacity))",
        background: "rgb(var(--color-background) / <alpha-value>)",
        sidebar: "rgb(var(--color-sidebar) / <alpha-value>)",
        card: "rgb(var(--color-card) / <alpha-value>)",
        border: "rgb(var(--color-border) / <alpha-value>)",
        divider: "rgb(var(--color-divider) / <alpha-value>)",
        headerBorder: "rgb(var(--color-header-border) / <alpha-value>)",
        surface: "rgb(var(--color-surface) / <alpha-value>)",
        hover: "rgb(var(--color-hover) / var(--hover-opacity))",
        selectedBorder: "rgb(var(--color-selected-border) / <alpha-value>)",
        actionHover: "rgb(var(--color-action-hover) / <alpha-value>)",
        selected: "rgb(var(--color-selected) / var(--selected-opacity))",
        ink: "rgb(var(--color-ink) / <alpha-value>)",
        muted: "rgb(var(--color-muted) / <alpha-value>)",
        brand: "rgb(var(--color-brand) / <alpha-value>)",
        brandDim: "rgb(var(--color-brand-dim) / <alpha-value>)",
        warning: "rgb(var(--color-warning) / <alpha-value>)",
        danger: "rgb(var(--color-danger) / <alpha-value>)",
        success: "rgb(var(--color-success) / <alpha-value>)",
        info: "rgb(var(--color-info) / <alpha-value>)",
      },
      boxShadow: {
        panel: "var(--shadow-panel)",
      },
      fontFamily: {
        display: ["\"Familjen Grotesk\"", "\"IBM Plex Sans\"", "sans-serif"],
        body: ["\"IBM Plex Sans\"", "-apple-system", "sans-serif"],
        mono: ["\"IBM Plex Mono\"", "\"JetBrains Mono\"", "monospace"],
      },
    },
  },
  plugins: [typography],
} satisfies Config;
