import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Group, Paper, SimpleGrid, Stack, Table, Text, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";

const when = (value?: string | null) => (value ? dayjs(value).format("YYYY-MM-DD HH:mm:ss") : "—");

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <Paper withBorder p="sm">
      <Text size="xs" c="dimmed">
        {label}
      </Text>
      <Text fw={700} size="lg">
        {value}
      </Text>
    </Paper>
  );
}

function BackfillControls({ t }: { t: (key: string, context?: { [key: string]: any }) => string }) {
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: ["market_backfill_status"],
    queryFn: () => api.market.backfillStatus(),
    refetchInterval: (query) => (query.state.data?.state === "running" ? 5_000 : false),
  });
  const start = useMutation({
    mutationFn: () => api.market.backfillStart(),
    onSuccess: (status) => queryClient.setQueryData(["market_backfill_status"], status),
  });
  const running = data?.state === "running" || start.isPending;
  const line =
    !data || data.state === "idle"
      ? t("backfill.idle")
      : data.state === "running"
        ? t("backfill.running", { done: data.items_done, total: data.items_total, days: data.days_inserted })
        : data.state === "done"
          ? t("backfill.done", { at: when(data.finished_at), items: data.items_done, days: data.days_inserted, missing: data.items_missing, failed: data.items_failed })
          : t("backfill.failed", { error: data.last_error ?? "" });
  return (
    <Group>
      <Button onClick={() => start.mutate()} disabled={running} loading={running}>
        {t("backfill.button")}
      </Button>
      <Text size="sm" c={data?.state === "failed" ? "red" : "dimmed"}>
        {line}
      </Text>
    </Group>
  );
}

export function CollectorPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.collector.${key}`, context);
  const { data, error } = api.collector.health();
  if (!isActive) return null;
  if (error)
    return (
      <Alert color="red" mt="md">
        {String((error as any)?.message ?? error)}
      </Alert>
    );
  if (!data) return <Text mt="md">{t("loading")}</Text>;

  const lanes = [
    { id: "hot", health: data.hot, granted: data.limiter.granted_hot },
    { id: "cold", health: data.cold, granted: data.limiter.granted_cold },
  ];

  return (
    <Stack mt="md">
      <Group>
        <Badge color={data.running ? "green" : "gray"}>{data.running ? t("running") : t("stopped")}</Badge>
        <Text size="sm" c="dimmed">
          {t("started_at", { at: when(data.started_at) })}
        </Text>
        {data.limiter.paused_ms > 0 && <Badge color="orange">{t("paused", { seconds: Math.ceil(data.limiter.paused_ms / 1000) })}</Badge>}
      </Group>
      {data.last_error && (
        <Alert color="yellow" title={t("last_error")}>
          {data.last_error}
        </Alert>
      )}
      <SimpleGrid cols={{ base: 2, md: 3, lg: 6 }}>
        <Stat label={t("active_items")} value={data.active_items} />
        <Stat label={t("inactive_items")} value={data.inactive_items} />
        <Stat label={t("hot_items")} value={data.hot_items} />
        <Stat label={t("items_behind")} value={data.items_behind} />
        <Stat
          label={t("last_cold_pass")}
          value={data.last_cold_pass_seconds != null ? t("minutes", { minutes: (data.last_cold_pass_seconds / 60).toFixed(1) }) : "—"}
        />
        <Stat label={t("rate_limited_total")} value={data.limiter.rate_limited_total} />
      </SimpleGrid>
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("lane")}</Table.Th>
            <Table.Th>{t("swept_last_hour")}</Table.Th>
            <Table.Th>{t("errors_last_hour")}</Table.Th>
            <Table.Th>{t("requests_total")}</Table.Th>
            <Table.Th>{t("last_sweep_at")}</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {lanes.map((lane) => (
            <Table.Tr key={lane.id}>
              <Table.Td>{t(`lanes.${lane.id}`)}</Table.Td>
              <Table.Td>{lane.health.swept_last_hour}</Table.Td>
              <Table.Td>{lane.health.errors_last_hour}</Table.Td>
              <Table.Td>{lane.granted}</Table.Td>
              <Table.Td>{when(lane.health.last_sweep_at)}</Table.Td>
            </Table.Tr>
          ))}
          <Table.Tr>
            <Table.Td>{t("lanes.trader")}</Table.Td>
            <Table.Td>—</Table.Td>
            <Table.Td>—</Table.Td>
            <Table.Td>{data.limiter.granted_trader}</Table.Td>
            <Table.Td>—</Table.Td>
          </Table.Tr>
        </Table.Tbody>
      </Table>
      <Text size="sm" c="dimmed">
        {t("footer", { refresh: when(data.last_item_refresh_at), maintenance: when(data.last_maintenance_at) })}
      </Text>
      <Paper withBorder p="sm">
        <Title order={5} mb="xs">
          {t("backfill.title")}
        </Title>
        <BackfillControls t={t} />
      </Paper>
    </Stack>
  );
}
