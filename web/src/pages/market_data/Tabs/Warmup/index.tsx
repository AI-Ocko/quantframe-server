import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Box, Paper, SimpleGrid, Stack, Text, useMantineTheme } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { Bar } from "react-chartjs-2";
import { TauriTypes } from "$types";

function Histogram({ title, label, buckets, color }: { title: string; label: string; buckets: TauriTypes.MarketHistogramBucket[]; color: string }) {
  return (
    <Paper withBorder p="sm">
      <Text fw={600} mb="xs">
        {title}
      </Text>
      <Box h={220}>
        <Bar
          options={{ responsive: true, maintainAspectRatio: false }}
          data={{ labels: buckets.map((b) => b.bucket), datasets: [{ label, data: buckets.map((b) => b.count), backgroundColor: color }] }}
        />
      </Box>
    </Paper>
  );
}

export function WarmupPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.warmup.${key}`, context);
  const theme = useMantineTheme();
  const { data } = useQuery({ queryKey: ["market_warmup"], queryFn: () => api.market.warmup(), enabled: !!isActive, refetchInterval: 60_000 });
  if (!data) return null;
  return (
    <Stack mt="md">
      <SimpleGrid cols={{ base: 2, md: 4 }}>
        {[
          [t("tracked"), data.tracked],
          [t("warm"), data.warm],
        ].map(([label, value]) => (
          <Paper withBorder p="sm" key={String(label)}>
            <Text size="xs" c="dimmed">
              {label}
            </Text>
            <Text fw={700} size="lg">
              {value}
            </Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Paper withBorder p="sm">
        <Text fw={600} mb="xs">
          {t("projected_title")}
        </Text>
        <Box h={260}>
          <Bar
            options={{ responsive: true, maintainAspectRatio: false }}
            data={{
              labels: data.projected.map((p) => p.date),
              datasets: [{ label: t("warm_count"), data: data.projected.map((p) => p.warm_count), backgroundColor: theme.colors.green[6] }],
            }}
          />
        </Box>
      </Paper>
      <SimpleGrid cols={{ base: 1, md: 2 }}>
        <Histogram title={t("history_title")} label={t("items")} buckets={data.history_days_histogram} color={theme.colors.blue[6]} />
        <Histogram title={t("trades_title")} label={t("items")} buckets={data.trades_histogram} color={theme.colors.violet[6]} />
      </SimpleGrid>
    </Stack>
  );
}
