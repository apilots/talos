import { useState, type ComponentProps, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { openResourceLink } from "@/shared/lib/open-resource";

export function ResourceLink({ href, children, title, className }: ComponentProps<"a">) {
  const { t } = useTranslation("webPreview");
  const [failed, setFailed] = useState(false);
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.type === "auxclick" && event.button !== 1) return;
    // Markdown owns URL sanitization; leave its other safe protocols to the browser.
    if (href?.startsWith("#") || (href && /^[a-z][a-z\d+.-]*:/i.test(href) && !/^(https?|talos-file):/i.test(href))) return;
    event.preventDefault(); setFailed(false);
    void openResourceLink(href ?? "").catch(() => setFailed(true));
  };
  return <><a href={href} title={title} className={className} onClick={open} onAuxClick={open}>{children}</a>{failed ? <span role="alert" className="ml-2 text-xs text-danger">{t("linkFailed")}</span> : null}</>;
}
