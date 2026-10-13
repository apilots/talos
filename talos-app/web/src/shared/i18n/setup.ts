import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import { defaultLocale, resources } from "./resources";

void i18n.use(initReactI18next).init({
  resources,
  lng: defaultLocale,
  fallbackLng: defaultLocale,
  interpolation: {
    escapeValue: false,
  },
  defaultNS: "common",
  ns: [
    "common",
    "computers",
    "errors",
    "sidebar",
    "chat",
    "memory",
    "files",
    "settings",
    "auth",
    "workspaceDocs",
    "plugins",
  ],
});

export { i18n };
