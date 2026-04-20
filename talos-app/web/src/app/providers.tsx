import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "react-router-dom";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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
