import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Box, Group, Paper, SegmentedControl, SimpleGrid, Text, useMantineTheme } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Chart } from "react-chartjs-2";
import { TauriTypes } from "$types";
import { useAnalyticsRange } from "../../range";

const TOTALS = ["revenue", "expenses", "profit", "sales", "purchases"] as const;

export function TimelinePanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.timeline.${key}`, context);
  const theme = useMantineTheme();
  const { range, setRange, from, to } = useAnalyticsRange();
  const [bucket, setBucket] = useState<TauriTypes.AnalyticsBucket>("day");
  const { data } = useQuery({
    queryKey: ["analytics_timeline", from, to, bucket],
    queryFn: () => api.analytics.timeline(from, to, bucket),
    enabled: !!isActive,
  });
  const rows = data ?? [];
  const sum = (key: (typeof TOTALS)[number]) => rows.reduce((acc, r) => acc + r[key], 0);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" clearable label={t("range")} valueFormat="YYYY MMM DD" w={260} value={range} onChange={setRange} />
        <SegmentedControl
          value={bucket}
          onChange={(v) => setBucket(v as TauriTypes.AnalyticsBucket)}
          data={[
            { value: "day", label: t("bucket.day") },
            { value: "week", label: t("bucket.week") },
          ]}
        />
      </Group>
      <SimpleGrid cols={{ base: 2, md: 5 }} mt="md">
        {TOTALS.map((key) => (
          <Paper withBorder p="sm" key={key}>
            <Text size="xs" c="dimmed">
              {t(`totals.${key}`)}
            </Text>
            <Text fw={700}>{sum(key)}</Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Paper withBorder p="sm" mt="md">
        {rows.length === 0 ? (
          <Text c="dimmed">{t("empty")}</Text>
        ) : (
          <Box h={360}>
            <Chart
              type="bar"
              options={{
                responsive: true,
                maintainAspectRatio: false,
                scales: { y: { position: "left" }, y1: { position: "right", grid: { drawOnChartArea: false } } },
              }}
              data={{
                labels: rows.map((r) => r.bucket_start),
                datasets: [
                  { type: "bar" as const, label: t("profit"), data: rows.map((r) => r.profit), backgroundColor: theme.colors.green[6], yAxisID: "y" },
                  {
                    type: "line" as const,
                    label: t("cumulative"),
                    data: rows.map((r) => r.cumulative_profit),
                    borderColor: theme.colors.blue[6],
                    backgroundColor: theme.colors.blue[6],
                    yAxisID: "y1",
                  },
                ],
              }}
            />
          </Box>
        )}
      </Paper>
    </>
  );
}
