import { useLocalStorage } from "@mantine/hooks";
import dayjs from "dayjs";

type Range = [string | null, string | null];

/** Shared inclusive date range for the analytics tabs; `to` is sent as the day after the picked end (spec §22 A1). */
export function useAnalyticsRange() {
  const [range, setRange] = useLocalStorage<Range>({
    key: "trading_analytics_range",
    defaultValue: [dayjs().subtract(30, "day").format("YYYY-MM-DD"), dayjs().format("YYYY-MM-DD")],
  });
  const from = range[0] ?? dayjs().subtract(30, "day").format("YYYY-MM-DD");
  const to = dayjs(range[1] ?? dayjs().format("YYYY-MM-DD"))
    .add(1, "day")
    .format("YYYY-MM-DD");
  return { range, setRange, from, to };
}
