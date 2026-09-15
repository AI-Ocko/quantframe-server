import api from "@api/index";
import { TauriTypes } from "$types";
import { SelectSubType } from "@components/Forms/SelectSubType";
import { SelectTradableItem } from "@components/Forms/SelectTradableItem";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Button, Code, Group, Modal, NumberInput, Stack, Table, Text } from "@mantine/core";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const UNRESOLVED_PREFIX = "unresolved: ";

type Row = { key: number; name: string; slug: string; sub_type?: TauriTypes.SubType; quantity: number; price: number };

/** Whole-platinum shares with the remainder on the first row, like the server's equal split. */
export function equalSplit(total: number, count: number): number[] {
  if (count <= 0) return [];
  const base = Math.floor(total / count);
  const shares = Array<number>(count).fill(base);
  shares[0] += total - base * count;
  return shares;
}

const goodsOf = (event: TauriTypes.HelperEvent) =>
  (event.resolution?.direction === "sale" ? event.payload.offered : event.payload.received).filter((item) => item.name !== "Platinum");

function initialRows(event: TauriTypes.HelperEvent): Row[] {
  const resolved = (event.resolution?.items ?? []).map((item) => ({
    name: item.name,
    slug: item.slug,
    sub_type: item.sub_type ?? undefined,
    quantity: item.quantity,
  }));
  const unresolvedNames = event.reason?.startsWith(UNRESOLVED_PREFIX) ? event.reason.slice(UNRESOLVED_PREFIX.length).split(", ") : [];
  const goods = goodsOf(event);
  const unresolved = unresolvedNames.map((name) => ({
    name,
    slug: "",
    sub_type: undefined,
    quantity: goods.find((item) => item.name === name)?.quantity ?? 1,
  }));
  const rows = [...resolved, ...unresolved];
  const prices = equalSplit(event.resolution?.platinum ?? 0, rows.length);
  return rows.map((row, index) => ({ ...row, key: index, price: prices[index] }));
}

export type ReviewTradeModalProps = {
  event: TauriTypes.HelperEvent;
  onClose(): void;
  onApplied(): void;
};

export function ReviewTradeModal({ event, onClose, onApplied }: ReviewTradeModalProps) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trades.review_modal.${key}`, context);
  const tDirection = (direction: string) => useTranslatePages(`live_scraper.trades.direction.${direction}`);
  const [rows, setRows] = useState<Row[]>(() => initialRows(event));
  const [nextKey, setNextKey] = useState(1000);
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const bySlug = useMemo(() => new Map((items ?? []).map((item) => [item.wfmUrl, item])), [items]);
  const platinum = event.resolution?.platinum ?? 0;
  const sum = rows.reduce((total, row) => total + row.price, 0);
  const valid = rows.length > 0 && rows.every((row) => row.slug && row.quantity >= 1 && row.price >= 0);

  const apply = useMutation({
    mutationFn: () =>
      api.helper_link.applyTrade(
        event.event_id,
        rows.map(({ slug, sub_type, quantity, price }) => ({ slug, sub_type, quantity, price })),
      ),
    onSuccess: onApplied,
  });
  const update = (key: number, patch: Partial<Row>) => setRows((current) => current.map((row) => (row.key === key ? { ...row, ...patch } : row)));
  const addRow = () => {
    setRows((current) => [...current, { key: nextKey, name: "", slug: "", quantity: 1, price: 0 }]);
    setNextKey((key) => key + 1);
  };
  const splitEqually = () =>
    setRows((current) => {
      const prices = equalSplit(platinum, current.length);
      return current.map((row, index) => ({ ...row, price: prices[index] }));
    });

  return (
    <Modal opened onClose={onClose} size="xl" title={t("title", { player: event.payload.player_name })}>
      <Stack>
        <Text size="sm">{t("summary", { direction: tDirection(event.resolution?.direction ?? "purchase"), platinum })}</Text>
        <Text size="sm" c="dimmed">
          {t("game_sent")}
        </Text>
        <Code block>
          {goodsOf(event)
            .map((item) => `${item.name} ×${item.quantity}${item.rank != null ? ` (rank ${item.rank})` : ""}`)
            .join("\n")}
        </Code>
        <Table withTableBorder>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t("columns.item")}</Table.Th>
              <Table.Th>{t("columns.sub_type")}</Table.Th>
              <Table.Th>{t("columns.quantity")}</Table.Th>
              <Table.Th>{t("columns.price")}</Table.Th>
              <Table.Th />
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {rows.map((row) => {
              const available = bySlug.get(row.slug)?.subTypes;
              return (
                <Table.Tr key={row.key}>
                  <Table.Td>
                    <Stack gap={2}>
                      <SelectTradableItem hideSubType value={row.slug} onChange={(item) => update(row.key, { slug: item.wfmUrl, sub_type: item.sub_type })} />
                      {row.name && (
                        <Text size="xs" c="dimmed">
                          {t("in_game", { name: row.name })}
                        </Text>
                      )}
                    </Stack>
                  </Table.Td>
                  <Table.Td>
                    <SelectSubType
                      showLabel={false}
                      value={row.sub_type ?? (available ? {} : undefined)}
                      availableSubTypes={available}
                      onChange={(sub_type) => update(row.key, { sub_type })}
                    />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput w={90} min={1} allowDecimal={false} value={row.quantity} onChange={(value) => update(row.key, { quantity: Number(value) || 0 })} />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput w={110} min={0} allowDecimal={false} value={row.price} onChange={(value) => update(row.key, { price: Number(value) || 0 })} />
                  </Table.Td>
                  <Table.Td>
                    <Button size="xs" variant="subtle" color="red" onClick={() => setRows((current) => current.filter((r) => r.key !== row.key))}>
                      {t("remove")}
                    </Button>
                  </Table.Td>
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
        <Group>
          <Button variant="light" onClick={addRow}>
            {t("add_item")}
          </Button>
          <Button variant="light" onClick={splitEqually}>
            {t("split_equally")}
          </Button>
        </Group>
        {sum !== platinum && <Alert color="yellow">{t("sum_mismatch", { sum, platinum })}</Alert>}
        {apply.error && <Alert color="red">{String((apply.error as any)?.message ?? apply.error)}</Alert>}
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button disabled={!valid} loading={apply.isPending} onClick={() => apply.mutate()}>
            {t("apply")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
