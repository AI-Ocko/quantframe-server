import api from "@api/index";
import { SearchField } from "@components/Forms/SearchField";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, Text } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { useAnalyticsRange } from "../../range";

export const num = (value?: number | null, digits = 1) => (value == null ? "—" : value.toFixed(digits));

/** Client-side sort on one column; strings compare with localeCompare, numbers numerically, nulls last. */
export function sortRows<T>(rows: T[], status: DataTableSortStatus<T>): T[] {
  const key = status.columnAccessor as keyof T;
  const dir = status.direction === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = a[key] as unknown,
      y = b[key] as unknown;
    if (x == null && y == null) return 0;
    if (x == null) return 1;
    if (y == null) return -1;
    if (typeof x === "number" && typeof y === "number") return (x - y) * dir;
    return String(x).localeCompare(String(y)) * dir;
  });
}

export function ItemsPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.items.${key}`, context);
  const { range, setRange, from, to } = useAnalyticsRange();
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsItemRow>>({ columnAccessor: "profit", direction: "desc" });
  const { data, isFetching } = useQuery({
    queryKey: ["analytics_items", from, to],
    queryFn: () => api.analytics.items(from, to),
    enabled: !!isActive,
  });
  const rows = useMemo(() => {
    const filtered = (data ?? []).filter((r) => r.item_name.toLowerCase().includes(search.toLowerCase()));
    return sortRows(filtered, sort);
  }, [data, search, sort]);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" clearable label={t("range")} valueFormat="YYYY MMM DD" w={260} value={range} onChange={setRange} />
        <SearchField value={search} onChange={setSearch} description={t("search")} />
      </Group>
      <DataTable
        mt="md"
        striped
        fetching={isFetching}
        records={rows}
        idAccessor={(r) => `${r.wfm_url}|${r.sub_type}`}
        sortStatus={sort}
        onSortStatusChange={setSort}
        noRecordsText={t("empty")}
        columns={[
          { accessor: "item_name", title: t("columns.item"), sortable: true },
          { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
          { accessor: "purchases", title: t("columns.purchases"), sortable: true },
          { accessor: "bought_qty", title: t("columns.bought_qty"), sortable: true },
          { accessor: "spend", title: t("columns.spend"), sortable: true },
          { accessor: "sales", title: t("columns.sales"), sortable: true },
          { accessor: "sold_qty", title: t("columns.sold_qty"), sortable: true },
          { accessor: "revenue", title: t("columns.revenue"), sortable: true },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => <Text c={r.profit < 0 ? "red" : "green"}>{r.profit}</Text> },
          { accessor: "avg_buy", title: t("columns.avg_buy"), sortable: true, render: (r) => num(r.avg_buy) },
          { accessor: "avg_sell", title: t("columns.avg_sell"), sortable: true, render: (r) => num(r.avg_sell) },
          { accessor: "avg_days_held", title: t("columns.avg_days_held"), sortable: true, render: (r) => num(r.avg_days_held) },
        ]}
      />
    </>
  );
}
