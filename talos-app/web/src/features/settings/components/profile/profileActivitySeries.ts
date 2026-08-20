export interface ProfileActivityPoint {
  date: string;
  tokens: number;
}

export interface DatedValue {
  date: string;
  value: number;
}

export interface ProfileActivitySeries {
  cumulative: DatedValue[];
  daily: DatedValue[];
  months: string[];
  weekly: DatedValue[];
}

export function createActivitySeries(
  activity: ProfileActivityPoint[],
  asOfDate: string,
): ProfileActivitySeries {
  const end = parseDate(asOfDate);
  const start = new Date(end);
  start.setDate(end.getDate() - (52 * 7 - 1));
  const tokensByDay = new Map(activity.map((point) => [point.date, point.tokens]));
  const daily = Array.from({ length: 52 * 7 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const dateKey = formatDate(date);
    return { date: dateKey, value: tokensByDay.get(dateKey) ?? 0 };
  });
  const weekly = Array.from({ length: 52 }, (_, weekIndex) => ({
    date: daily[weekIndex * 7]?.date ?? asOfDate,
    value: daily
      .slice(weekIndex * 7, weekIndex * 7 + 7)
      .reduce((total, point) => total + point.value, 0),
  }));
  let runningTotal = 0;
  const cumulative = daily.map((point) => {
    runningTotal += point.value;
    return { date: point.date, value: runningTotal };
  });
  const months = Array.from({ length: 12 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + Math.floor(index * ((52 * 7) / 12)));
    return new Intl.DateTimeFormat("en-US", { month: "short" }).format(date);
  });
  return { cumulative, daily, months, weekly };
}

function parseDate(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function formatDate(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
