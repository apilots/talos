import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "react-router-dom";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAuthBootstrap } from "@/features/auth";
import { router } from "@/app/router";
import { useAppStore } from "@/shared/store/app-store";
import { i18n } from "@/shared/i18n/setup";

export function AppProviders() {
  const uiTheme = useAppStore((state) => state.uiTheme);
  const locale = useAppStore((state) => state.locale);
  useTranslation();
  useAuthBootstrap();
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            retry: 1,
            refetchOnWindowFocus: false,
          },
        },
      }),
  );

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
