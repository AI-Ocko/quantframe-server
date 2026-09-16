import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, Text } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { sortRows } from "@utils/sortRows";
import { useAnalyticsRange } from "../../range";

export function PartnersPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.partners.${key}`, context);
  const { range, setRange, from, to } = useAnalyticsRange();
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsPartnerRow>>({ columnAccessor: "trades", direction: "desc" });
  const { data, isFetching } = useQuery({
    queryKey: ["analytics_partners", from, to],
    queryFn: () => api.analytics.partners(from, to),
    enabled: !!isActive,
  });
  const rows = useMemo(() => sortRows(data ?? [], sort), [data, sort]);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" label={t("range")} valueFormat="YYYY MMM DD" w={260} value={range} onChange={setRange} />
      </Group>
      <DataTable
        mt="md"
        striped
        fetching={isFetching}
        records={rows}
        idAccessor="user_name"
        sortStatus={sort}
        onSortStatusChange={setSort}
        noRecordsText={t("empty")}
        columns={[
          { accessor: "user_name", title: t("columns.user"), sortable: true },
          { accessor: "trades", title: t("columns.trades"), sortable: true },
          { accessor: "bought_count", title: t("columns.bought_count"), sortable: true },
          { accessor: "bought_plat", title: t("columns.bought_plat"), sortable: true },
          { accessor: "sold_count", title: t("columns.sold_count"), sortable: true },
          { accessor: "sold_plat", title: t("columns.sold_plat"), sortable: true },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => <Text c={r.profit < 0 ? "red" : "green"}>{r.profit}</Text> },
          { accessor: "last_trade_at", title: t("columns.last_trade_at"), sortable: true, render: (r) => dayjs(r.last_trade_at).format("YYYY-MM-DD HH:mm") },
        ]}
      />
    </>
  );
}
