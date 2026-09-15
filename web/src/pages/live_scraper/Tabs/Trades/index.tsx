import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Group, Pagination, SegmentedControl, Stack, Table, Text } from "@mantine/core";
import { modals } from "@mantine/modals";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ReviewTradeModal } from "./ReviewTradeModal";

const LIMIT = 25;
const FILTERS = ["needs_review", "applied", "ignored", "all"] as const;
const STATUS_COLORS: Record<TauriTypes.HelperEventStatus, string> = { applied: "green", needs_review: "yellow", ignored: "gray" };

function itemsSummary(event: TauriTypes.HelperEvent) {
  const resolved = event.resolution?.items ?? [];
  if (resolved.length > 0) return resolved.map((item) => `${item.item_name} ×${item.quantity}`).join(", ");
  return [...event.payload.offered, ...event.payload.received]
    .filter((item) => item.name !== "Platinum")
    .map((item) => `${item.name} ×${item.quantity}`)
    .join(", ");
}

export function TradesPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trades.${key}`, context);
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<string>("needs_review");
  const [page, setPage] = useState(1);
  const [reviewing, setReviewing] = useState<TauriTypes.HelperEvent | null>(null);
  const status = filter === "all" ? null : (filter as TauriTypes.HelperEventStatus);
  const { data } = useQuery({
    queryKey: ["helper_trades", filter, page],
    queryFn: () => api.helper_link.trades(status, page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["helper_trades"] });
  const ignore = useMutation({ mutationFn: (eventId: string) => api.helper_link.ignoreTrade(eventId), onSettled: refresh });
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));

  const confirmIgnore = (event: TauriTypes.HelperEvent) =>
    modals.openConfirmModal({
      title: t("ignore_title"),
      children: <Text size="sm">{t("ignore_message", { player: event.payload.player_name })}</Text>,
      labels: { confirm: t("ignore"), cancel: t("cancel") },
      onConfirm: () => ignore.mutate(event.event_id),
    });

  return (
    <Stack mt="md">
      <Group justify="space-between">
        <SegmentedControl
          value={filter}
          onChange={(value) => {
            setFilter(value);
            setPage(1);
          }}
          data={FILTERS.map((value) => ({ value, label: t(`status.${value}`) }))}
        />
        <Text size="sm" c="dimmed">
          {t("total", { total: data?.total ?? 0 })}
        </Text>
      </Group>
      {ignore.error && <Alert color="red">{String((ignore.error as any)?.message ?? ignore.error)}</Alert>}
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("columns.detected_at")}</Table.Th>
            <Table.Th>{t("columns.player")}</Table.Th>
            <Table.Th>{t("columns.direction")}</Table.Th>
            <Table.Th>{t("columns.platinum")}</Table.Th>
            <Table.Th>{t("columns.items")}</Table.Th>
            <Table.Th>{t("columns.status")}</Table.Th>
            <Table.Th>{t("columns.reason")}</Table.Th>
            <Table.Th />
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(data?.results ?? []).map((event) => {
            const direction = event.resolution?.direction;
            return (
              <Table.Tr key={event.event_id}>
                <Table.Td>{event.detected_at}</Table.Td>
                <Table.Td>{event.payload.player_name}</Table.Td>
                <Table.Td>{direction ? t(`direction.${direction}`) : "—"}</Table.Td>
                <Table.Td>{direction ? event.resolution?.platinum : "—"}</Table.Td>
                <Table.Td>{itemsSummary(event)}</Table.Td>
                <Table.Td>
                  <Badge color={STATUS_COLORS[event.status]}>{t(`status.${event.status}`)}</Badge>
                </Table.Td>
                <Table.Td>{event.reason ?? "—"}</Table.Td>
                <Table.Td>
                  {event.status === "needs_review" && (
                    <Group gap="xs" wrap="nowrap">
                      <Button size="xs" disabled={!direction} onClick={() => setReviewing(event)}>
                        {t("review")}
                      </Button>
                      <Button size="xs" variant="light" color="gray" onClick={() => confirmIgnore(event)}>
                        {t("ignore")}
                      </Button>
                    </Group>
                  )}
                </Table.Td>
              </Table.Tr>
            );
          })}
        </Table.Tbody>
      </Table>
      <Pagination total={pages} value={page} onChange={setPage} />
      {reviewing && (
        <ReviewTradeModal
          event={reviewing}
          onClose={() => setReviewing(null)}
          onApplied={() => {
            setReviewing(null);
            refresh();
          }}
        />
      )}
    </Stack>
  );
}
