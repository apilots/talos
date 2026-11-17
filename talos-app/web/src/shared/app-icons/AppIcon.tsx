import { useAppStore } from "@/shared/store/app-store";

import { appIconAssetUrl, type AppIconId } from "./catalog";

export function AppIcon({
  alt,
  className,
  id,
}: {
  alt: string;
  className?: string;
  id?: AppIconId;
}) {
  const iconId = useAppStore((state) => id ?? state.appIconId);

  return <img alt={alt} className={className} src={appIconAssetUrl(iconId, "icon")} />;
}
