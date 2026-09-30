import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, NumberInput, Switch, Text } from "@mantine/core";
import { useDebouncedValue } from "@mantine/hooks";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { num, sortRows } from "@utils/sortRows";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { writeSelection } from "../../selection";

const PAGE = 50;
type Row = TauriTypes.MarketHoldRow;
const DEFAULT_SORT: DataTableSortStatus<Row> = { columnAccessor: "profit_low_pct", direction: "desc" };

export function HoldsPanel({ isActive, onOpenItem }: { isActive?: boolean; onOpenItem: () => void }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.holds.${key}`, context);
  // Raw input values: NumberInput hands back strings like "0." mid-typing, so they convert only at query time.
  const [inputs, setInputs] = useState<Record<"horizonWeeks" | "minVolume" | "minMarginPct" | "minSteadiness", number | string>>({
    horizonWeeks: 4,
    minVolume: 10,
    minMarginPct: 10,
    minSteadiness: 0.85,
  });
  const [debounced] = useDebouncedValue(inputs, 300);
  const args = {
    horizonWeeks: Number(debounced.horizonWeeks) || 0,
    minVolume: Number(debounced.minVolume) || 0,
    minMarginPct: Number(debounced.minMarginPct) || 0,
    minSteadiness: Number(debounced.minSteadiness) || 0,
  };
  const [showAll, setShowAll] = useState(false);
  const [page, setPage] = useState(1);
  // null keeps the served order (qualified first, then profit_low_pct desc) until a column is sorted.
  const [sort, setSort] = useState<DataTableSortStatus<Row> | null>(null);
  const { data, isFetching } = useQuery({
    queryKey: ["market_holds", args.horizonWeeks, args.minVolume, args.minMarginPct, args.minSteadiness],
    queryFn: () => api.market.holds(args),
    enabled: !!isActive,
    placeholderData: keepPreviousData,
  });
  const rows = useMemo(() => {
    const filtered = (data?.rows ?? []).filter((r) => showAll || r.qualified);
    return sort ? sortRows(filtered, sort) : filtered;
  }, [data, showAll, sort]);
  const set = (key: keyof typeof inputs) => (value: number | string) => {
    setInputs((s) => ({ ...s, [key]: value }));
    setPage(1);
  };
  const signed = (value: number, text: string) => <Text size="sm" c={value < 0 ? "red" : "green"}>{text}</Text>;

  return (
    <>
      <Group mt="md" align="end">
        <NumberInput label={t("horizon")} value={inputs.horizonWeeks} onChange={set("horizonWeeks")} min={1} max={12} allowDecimal={false} w={140} />
        <NumberInput label={t("min_volume")} value={inputs.minVolume} onChange={set("minVolume")} min={0} w={160} />
        <NumberInput label={t("min_margin")} value={inputs.minMarginPct} onChange={set("minMarginPct")} min={0} w={140} />
        <NumberInput
          label={t("min_steadiness")}
          value={inputs.minSteadiness}
          onChange={set("minSteadiness")}
          min={0}
          max={1}
          step={0.05}
          decimalScale={2}
          w={170}
        />
        <Switch
          label={t("show_all")}
          checked={showAll}
          onChange={(e) => {
            setShowAll(e.currentTarget.checked);
            setPage(1);
          }}
        />
      </Group>
      {data && (
        <Text size="sm" mt="md">
          {t("header", { qualified: data.qualified, scored: data.scored, latest_day: data.latest_day || "—" })}
        </Text>
      )}
      <DataTable
        mt="sm"
        striped
        highlightOnHover
        minHeight={150}
        fetching={isFetching}
        records={rows.slice((page - 1) * PAGE, page * PAGE)}
        idAccessor={(r) => `${r.item_id}|${r.sub_type}`}
        totalRecords={rows.length}
        recordsPerPage={PAGE}
        page={page}
        onPageChange={setPage}
        sortStatus={sort ?? DEFAULT_SORT}
        onSortStatusChange={(s) => {
          setSort(s);
          setPage(1);
        }}
        noRecordsText={t("empty")}
        onRowClick={({ record }) => {
          writeSelection({ slug: record.slug, sub_type: record.sub_type });
          onOpenItem();
        }}
        columns={[
          {
            accessor: "name",
            title: t("columns.item"),
            sortable: true,
            render: (r) => (
              <Group gap={6} wrap="nowrap">
                <Text size="sm">{r.name}</Text>
                {r.sub_type && (
                  <Text size="sm" c="dimmed">
                    {r.sub_type}
                  </Text>
                )}
                {showAll && r.qualified && (
                  <Badge color="green" size="sm">
                    {t("qualified")}
                  </Badge>
                )}
              </Group>
            ),
          },
          { accessor: "category", title: t("columns.category"), sortable: true },
          { accessor: "ask_now", title: t("columns.buy_at"), sortable: true, render: (r) => num(r.ask_now) },
          { accessor: "exit_low", title: t("columns.exit"), sortable: true, render: (r) => `${num(r.exit_low)} → ${num(r.exit_mid)}` },
          {
            accessor: "profit_low_pct",
            title: t("columns.profit_low"),
            sortable: true,
            render: (r) => signed(r.profit_low, `${num(r.profit_low)} p (${num(r.profit_low_pct)} %)`),
          },
          { accessor: "trend_pct_month", title: t("columns.trend"), sortable: true, render: (r) => signed(r.trend_pct_month, `${num(r.trend_pct_month)} %`) },
          { accessor: "steadiness", title: t("columns.steadiness"), sortable: true, render: (r) => num(r.steadiness, 2) },
          { accessor: "volume", title: t("columns.volume"), sortable: true, render: (r) => num(r.volume, 1) },
          { accessor: "weeks", title: t("columns.weeks"), sortable: true },
          { accessor: "trend_intact", title: t("columns.trend_intact"), sortable: true, render: (r) => (r.trend_intact ? "✓" : "✗") },
          {
            accessor: "quote_source",
            title: t("columns.quote"),
            sortable: true,
            render: (r) => <Badge color={r.quote_source === "sweep" ? "blue" : "gray"}>{r.quote_source}</Badge>,
          },
        ]}
      />
      <Text size="xs" c="dimmed" mt="sm">
        {t("note")}
      </Text>
    </>
  );
}
