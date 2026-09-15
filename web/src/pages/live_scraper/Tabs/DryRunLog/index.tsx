import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Pagination, Stack, Table, Text } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const LIMIT = 50;

export function DryRunLogPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.dry_run_log.${key}`, context);
  const [page, setPage] = useState(1);
  const { data } = useQuery({
    queryKey: ["trader_dry_run_log", page],
    queryFn: () => api.live_scraper.dryRunLog(page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const names = useMemo(() => new Map((items ?? []).map((item) => [item.wfmId, item.name])), [items]);
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));

  return (
    <Stack mt="md">
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
                <Badge color={entry.action === "delete" ? "red" : entry.action === "create" ? "green" : "blue"}>{entry.action}</Badge>
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
