import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "react-router-dom";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAuthBootstrap } from "@/features/auth";
import { productQueries as queryClient } from "@/shared/api/product-queries";
import { router } from "@/app/router";
import { useAppStore } from "@/shared/store/app-store";
import { i18n } from "@/shared/i18n/setup";
import { useAppIconEffects } from "@/shared/app-icons/useAppIconEffects";

export function AppProviders() {
  const uiTheme = useAppStore((state) => state.uiTheme);
  const locale = useAppStore((state) => state.locale);
  useTranslation();
  useAuthBootstrap();
  useAppIconEffects();
  useEffect(() => getDesktopBridge()?.onRuntimeState?.((workerRuntimeState) => {
    useAppStore.getState().setConnection({ workerRuntimeState });
  }), []);

  useEffect(() => useAppStore.subscribe((state, previous) => {
    if (connectionScope(state.connection) !== connectionScope(previous.connection)) {
      // Clear the previous actor before React mounts observers for the next actor.
      void queryClient.cancelQueries({ queryKey: ["talos"] });
      queryClient.removeQueries({ queryKey: ["talos"] });
    }
  }), [queryClient]);

  useEffect(() => {
    document.documentElement.dataset.theme = uiTheme;
  }, [uiTheme]);

  useEffect(() => {
    document.documentElement.lang = locale;
    void i18n.changeLanguage(locale);
  }, [locale]);

  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
