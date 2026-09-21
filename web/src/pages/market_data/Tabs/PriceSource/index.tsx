import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, Loader, Paper, SimpleGrid, Switch, Text, TextInput } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { num, sortRows } from "@utils/sortRows";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";

const PAGE = 50;
type Row = TauriTypes.MarketPriceSourceRow;

/**
 * Candidate membership or the warm flags differ, an inferred candidate has no closed values at all
 * (it silently stays on the inferred basis in closed mode), or the two moving averages are more than
 * 10 % apart (spec §25 P8, P12).
 */
function differs(r: Row) {
  if (r.candidate_inferred !== r.candidate_closed) return true;
  if (r.warm_inferred !== r.warm_closed) return true;
  if (r.candidate_inferred && r.closed_volume == null) return true;
  if (r.inferred_moving_avg == null || r.closed_moving_avg == null || r.closed_moving_avg === 0) return false;
  return Math.abs(r.inferred_moving_avg - r.closed_moving_avg) / r.closed_moving_avg > 0.1;
}

export function PriceSourcePanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.price_source.${key}`, context);
  const [search, setSearch] = useState("");
  const [onlyDiffs, setOnlyDiffs] = useState(true);
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<DataTableSortStatus<Row>>({ columnAccessor: "closed_volume", direction: "desc" });
  const { data, isPending } = useQuery({ queryKey: ["market_price_sources"], queryFn: () => api.market.priceSources(), enabled: !!isActive });
  const rows = useMemo(() => {
    const filtered = (data?.rows ?? []).filter((r) => (!onlyDiffs || differs(r)) && r.name.toLowerCase().includes(search.toLowerCase()));
    return sortRows(filtered, sort);
  }, [data, onlyDiffs, search, sort]);
  if (isPending) return <Loader size="sm" mt="md" />;
  if (!data) return null;
  const yesNo = (value: boolean) => (value ? <Badge color="green">{t("yes")}</Badge> : <Badge color="gray">{t("no")}</Badge>);

  return (
    <>
      <Text size="sm" mt="md">
        {t("mode", { mode: t(`modes.${data.mode}`), basis: t(`bases.${data.profit_basis}`), guard: data.guard_pct })}
      </Text>
      <Text size="sm" c="dimmed">
        {t("refresh", data.refresh)}
      </Text>
      <SimpleGrid cols={{ base: 1, md: 3 }} mt="sm">
        {(["inferred", "closed", "both"] as const).map((key) => (
          <Paper withBorder p="sm" key={key}>
            <Text size="xs" c="dimmed">
              {t(`candidates.${key}`)}
            </Text>
            <Text fw={700} size="lg">
              {data.candidates[key]}
            </Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Group mt="md" align="end">
        <TextInput
          label={t("search")}
          value={search}
          onChange={(e) => {
            setSearch(e.currentTarget.value);
            setPage(1);
          }}
        />
        <Switch
          label={t("only_diffs")}
          checked={onlyDiffs}
          onChange={(e) => {
            setOnlyDiffs(e.currentTarget.checked);
            setPage(1);
          }}
        />
      </Group>
      <DataTable
        mt="sm"
        withTableBorder
        striped
        records={rows.slice((page - 1) * PAGE, page * PAGE)}
        idAccessor={(r) => `${r.item_id}:${r.sub_type}`}
        totalRecords={rows.length}
        recordsPerPage={PAGE}
        page={page}
        onPageChange={setPage}
        sortStatus={sort}
        onSortStatusChange={setSort}
        columns={[
          { accessor: "name", title: t("columns.name"), sortable: true, render: (r) => (r.sub_type ? `${r.name} (${r.sub_type})` : r.name) },
          { accessor: "inferred_volume", title: t("columns.inferred_volume"), sortable: true, render: (r) => num(r.inferred_volume, 1) },
          { accessor: "closed_volume", title: t("columns.closed_volume"), sortable: true, render: (r) => num(r.closed_volume, 1) },
          { accessor: "inferred_moving_avg", title: t("columns.inferred_moving_avg"), sortable: true, render: (r) => num(r.inferred_moving_avg, 1) },
          { accessor: "closed_moving_avg", title: t("columns.closed_moving_avg"), sortable: true, render: (r) => num(r.closed_moving_avg, 1) },
          { accessor: "week_price_shift", title: t("columns.week_price_shift"), sortable: true, render: (r) => num(r.week_price_shift, 1) },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => num(r.profit, 0) },
          { accessor: "closed_range_profit", title: t("columns.closed_range_profit"), sortable: true, render: (r) => num(r.closed_range_profit, 1) },
          { accessor: "closed_days", title: t("columns.closed_days"), sortable: true },
          { accessor: "warm_inferred", title: t("columns.warm_inferred"), sortable: true, render: (r) => yesNo(r.warm_inferred) },
          { accessor: "warm_closed", title: t("columns.warm_closed"), sortable: true, render: (r) => yesNo(r.warm_closed) },
          { accessor: "candidate_inferred", title: t("columns.candidate_inferred"), sortable: true, render: (r) => yesNo(r.candidate_inferred) },
          { accessor: "candidate_closed", title: t("columns.candidate_closed"), sortable: true, render: (r) => yesNo(r.candidate_closed) },
          { accessor: "guarded", title: t("columns.guarded"), sortable: true, render: (r) => (r.guarded ? <Badge color="orange">{t("yes")}</Badge> : null) },
        ]}
      />
    </>
  );
}
