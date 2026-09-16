import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, Loader, NumberInput, SegmentedControl, SimpleGrid, Table, Text, Title } from "@mantine/core";
import { useDebouncedValue } from "@mantine/hooks";
import { useQuery } from "@tanstack/react-query";
import { num } from "@utils/sortRows";
import { useState } from "react";
import { TauriTypes } from "$types";
import { writeSelection } from "../../selection";

function MoverTable({
  title,
  rows,
  t,
  onOpen,
}: {
  title: string;
  rows: TauriTypes.MarketMover[];
  t: (key: string) => string;
  onOpen: (mover: TauriTypes.MarketMover) => void;
}) {
  return (
    <div>
      <Title order={5} mb="xs">
        {title}
      </Title>
      {rows.length === 0 ? (
        <Text c="dimmed">{t("empty")}</Text>
      ) : (
        <Table striped withTableBorder highlightOnHover>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t("columns.item")}</Table.Th>
              <Table.Th>{t("columns.sub_type")}</Table.Th>
              <Table.Th>{t("columns.median_then")}</Table.Th>
              <Table.Th>{t("columns.median_now")}</Table.Th>
              <Table.Th>{t("columns.change_pct")}</Table.Th>
              <Table.Th>{t("columns.volume")}</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {rows.map((mover) => (
              <Table.Tr key={`${mover.item_id}|${mover.sub_type}`} style={{ cursor: "pointer" }} onClick={() => onOpen(mover)}>
                <Table.Td>{mover.name}</Table.Td>
                <Table.Td>{mover.sub_type || "—"}</Table.Td>
                <Table.Td>{num(mover.median_then)}</Table.Td>
                <Table.Td>{num(mover.median_now)}</Table.Td>
                <Table.Td>
                  <Text c={mover.change_pct < 0 ? "red" : "green"}>{num(mover.change_pct)}%</Text>
                </Table.Td>
                <Table.Td>{num(mover.volume, 2)}</Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}
    </div>
  );
}

export function MoversPanel({ isActive, onOpenItem }: { isActive?: boolean; onOpenItem: () => void }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.movers.${key}`, context);
  const [period, setPeriod] = useState<"day" | "week">("day");
  const [minVolume, setMinVolume] = useState<number>(3);
  const [debouncedMinVolume] = useDebouncedValue(minVolume, 300);
  const { data, isPending } = useQuery({
    queryKey: ["market_movers", debouncedMinVolume],
    queryFn: () => api.market.movers(debouncedMinVolume),
    enabled: !!isActive,
  });
  const list = data?.[period] ?? { up: [], down: [] };
  const open = (mover: TauriTypes.MarketMover) => {
    writeSelection({ slug: mover.slug, sub_type: mover.sub_type });
    onOpenItem();
  };
  if (isPending) return <Loader size="sm" mt="md" />;

  return (
    <>
      <Group mt="md" align="end">
        <SegmentedControl
          value={period}
          onChange={(value) => setPeriod(value as "day" | "week")}
          data={[
            { value: "day", label: t("period.day") },
            { value: "week", label: t("period.week") },
          ]}
        />
        <NumberInput label={t("min_volume")} value={minVolume} onChange={(value) => setMinVolume(Number(value) || 0)} min={0} step={0.5} w={160} />
      </Group>
      <SimpleGrid cols={{ base: 1, md: 2 }} mt="md">
        <MoverTable title={t("up")} rows={list.up} t={t} onOpen={open} />
        <MoverTable title={t("down")} rows={list.down} t={t} onOpen={open} />
      </SimpleGrid>
    </>
  );
}
