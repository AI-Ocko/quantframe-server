import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, Pagination, SegmentedControl, Stack, Table, Text, Title } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const LIMIT = 50;
const DAY_OPTIONS = ["1", "7", "30"];

export function DryRunLogPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.dry_run_log.${key}`, context);
  const [page, setPage] = useState(1);
  const [days, setDays] = useState("7");
  const { data } = useQuery({
    queryKey: ["trader_dry_run_log", page],
    queryFn: () => api.live_scraper.dryRunLog(page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: summary } = useQuery({
    queryKey: ["trader_dry_run_summary", days],
    queryFn: () => api.live_scraper.dryRunSummary(Number(days)),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const names = useMemo(() => new Map((items ?? []).map((item) => [item.wfmId, item.name])), [items]);
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));
  const actionColor = (action: string) => (action === "delete" ? "red" : action === "create" ? "green" : "blue");

  return (
    <Stack mt="md">
      <Group justify="space-between">
        <Title order={5}>{t("summary.title")}</Title>
        <SegmentedControl value={days} onChange={setDays} data={DAY_OPTIONS.map((value) => ({ value, label: t("summary.days", { days: value }) }))} />
      </Group>
      {summary && (
        <Text size="sm" c="dimmed">
          {t("summary.since", { since: summary.since })}
        </Text>
      )}
      {summary && summary.by_action.length === 0 ? (
        <Text size="sm" c="dimmed">
          {t("summary.empty")}
        </Text>
      ) : (
        <Group align="flex-start" grow>
          <Table striped withTableBorder captionSide="top">
            <Table.Caption>{t("summary.by_action")}</Table.Caption>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t("summary.action")}</Table.Th>
                <Table.Th>{t("summary.side")}</Table.Th>
                <Table.Th>{t("summary.forced_by")}</Table.Th>
                <Table.Th>{t("summary.count")}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(summary?.by_action ?? []).map((row) => (
                <Table.Tr key={`${row.action}-${row.side}-${row.forced_by}`}>
                  <Table.Td>
                    <Badge color={actionColor(row.action)}>{row.action}</Badge>
                  </Table.Td>
                  <Table.Td>{row.side}</Table.Td>
                  <Table.Td>{row.forced_by}</Table.Td>
                  <Table.Td>{row.count}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
          <Table striped withTableBorder captionSide="top">
            <Table.Caption>{t("summary.by_item")}</Table.Caption>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t("summary.item")}</Table.Th>
                <Table.Th>{t("summary.sub_type")}</Table.Th>
                <Table.Th>{t("summary.action")}</Table.Th>
                <Table.Th>{t("summary.count")}</Table.Th>
                <Table.Th>{t("summary.min_price")}</Table.Th>
                <Table.Th>{t("summary.max_price")}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(summary?.by_item ?? []).map((row) => (
                <Table.Tr key={`${row.item_id}-${row.sub_type}-${row.action}`}>
                  <Table.Td>{names.get(row.item_id) ?? row.item_id}</Table.Td>
                  <Table.Td>{row.sub_type || "—"}</Table.Td>
                  <Table.Td>
                    <Badge color={actionColor(row.action)}>{row.action}</Badge>
                  </Table.Td>
                  <Table.Td>{row.count}</Table.Td>
                  <Table.Td>{row.min_price ?? "—"}</Table.Td>
                  <Table.Td>{row.max_price ?? "—"}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Group>
      )}
      <Text size="sm" c="dimmed">
        {t("total", { total: data?.total ?? 0 })}
      </Text>
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("at")}</Table.Th>
            <Table.Th>{t("action")}</Table.Th>
            <Table.Th>{t("side")}</Table.Th>
            <Table.Th>{t("item")}</Table.Th>
            <Table.Th>{t("sub_type")}</Table.Th>
            <Table.Th>{t("price")}</Table.Th>
            <Table.Th>{t("quantity")}</Table.Th>
            <Table.Th>{t("forced_by")}</Table.Th>
            <Table.Th>{t("reason")}</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(data?.results ?? []).map((entry) => (
            <Table.Tr key={entry.id}>
              <Table.Td>{entry.at}</Table.Td>
              <Table.Td>
                <Badge color={actionColor(entry.action)}>{entry.action}</Badge>
              </Table.Td>
              <Table.Td>{entry.side}</Table.Td>
              <Table.Td>{names.get(entry.item_id) ?? entry.item_id}</Table.Td>
              <Table.Td>{entry.sub_type || "—"}</Table.Td>
              <Table.Td>{entry.price ?? "—"}</Table.Td>
              <Table.Td>{entry.quantity ?? "—"}</Table.Td>
              <Table.Td>{entry.forced_by}</Table.Td>
              <Table.Td>{entry.reason}</Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Pagination total={pages} value={page} onChange={setPage} />
    </Stack>
  );
}
