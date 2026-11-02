export function milliseconds(
  value: number | null | undefined,
  locale: string,
): string {
  if (value === null || value === undefined) return "—";
  return (
    new Intl.NumberFormat(locale, {
      maximumFractionDigits: value < 1000 ? 0 : 2,
    }).format(value < 1000 ? value : value / 1000) +
    (value < 1000 ? " ms" : " s")
  );
}
export const number = (value: number | null | undefined, locale: string) =>
  value == null ? "—" : new Intl.NumberFormat(locale).format(value);
