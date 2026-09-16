import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Text } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { num, sortRows } from "../Items";

export function StockPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.stock.${key}`, context);
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsStockRow>>({ columnAccessor: "unrealised", direction: "desc" });
  const { data, isFetching } = useQuery({
    queryKey: ["analytics_stock"],
    queryFn: () => api.analytics.stock(),
    enabled: !!isActive,
  });
  const rows = useMemo(() => sortRows(data ?? [], sort), [data, sort]);
  const signed = (v?: number | null) => (v == null ? <Text c="dimmed">—</Text> : <Text c={v < 0 ? "red" : "green"}>{v.toFixed(0)}</Text>);

  return (
    <DataTable
      mt="md"
      striped
      fetching={isFetching}
      records={rows}
      idAccessor="id"
      sortStatus={sort}
      onSortStatusChange={setSort}
      noRecordsText={t("empty")}
      columns={[
        { accessor: "item_name", title: t("columns.item"), sortable: true },
        { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
        { accessor: "owned", title: t("columns.owned"), sortable: true },
        { accessor: "bought", title: t("columns.bought"), sortable: true },
        { accessor: "list_price", title: t("columns.list_price"), sortable: true, render: (r) => r.list_price ?? "—" },
        { accessor: "median", title: t("columns.median"), sortable: true, render: (r) => num(r.median) },
        { accessor: "moving_avg", title: t("columns.moving_avg"), sortable: true, render: (r) => num(r.moving_avg) },
        { accessor: "volume", title: t("columns.volume"), sortable: true, render: (r) => num(r.volume, 2) },
        { accessor: "unrealised", title: t("columns.unrealised"), sortable: true, render: (r) => signed(r.unrealised) },
        { accessor: "list_vs_median", title: t("columns.list_vs_median"), sortable: true, render: (r) => signed(r.list_vs_median) },
        { accessor: "days_in_stock", title: t("columns.days_in_stock"), sortable: true, render: (r) => r.days_in_stock.toFixed(0) },
        {
          accessor: "warm",
          title: t("columns.status"),
          render: (r) =>
            r.median == null ? <Badge color="gray">{t("no_stats")}</Badge> : <Badge color={r.warm ? "green" : "yellow"}>{r.warm ? t("warm") : t("cold")}</Badge>,
        },
      ]}
    />
  );
}
