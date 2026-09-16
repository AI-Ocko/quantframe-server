import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, NumberInput, Switch, Text, TextInput } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { num, sortRows } from "@utils/sortRows";
import dayjs from "dayjs";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { writeSelection } from "../../selection";

const PAGE = 50;

export function OverviewPanel({ isActive, onOpenItem }: { isActive?: boolean; onOpenItem: () => void }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.overview.${key}`, context);
  const [search, setSearch] = useState("");
  const [warmOnly, setWarmOnly] = useState(false);
  const [minVolume, setMinVolume] = useState<number>(0);
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.MarketOverviewRow>>({ columnAccessor: "profit", direction: "desc" });
  const { data, isFetching } = useQuery({
    queryKey: ["market_overview"],
    queryFn: () => api.market.overview(),
    enabled: !!isActive,
  });
  const rows = useMemo(() => {
    const filtered = (data ?? []).filter(
      (r) => (!warmOnly || r.warm) && r.volume >= minVolume && r.name.toLowerCase().includes(search.toLowerCase()),
    );
    return sortRows(filtered, sort);
  }, [data, warmOnly, minVolume, search, sort]);
  const shown = rows.slice((page - 1) * PAGE, page * PAGE);

  return (
    <>
      <Group mt="md" align="end">
        <TextInput
          label={t("search")}
          value={search}
          onChange={(e) => {
            setSearch(e.currentTarget.value);
            setPage(1);
          }}
          w={260}
        />
        <NumberInput
          label={t("min_volume")}
          value={minVolume}
          onChange={(v) => {
            setMinVolume(Number(v) || 0);
            setPage(1);
          }}
          min={0}
          step={0.5}
          w={160}
        />
        <Switch
          label={t("warm_only")}
          checked={warmOnly}
          onChange={(e) => {
            setWarmOnly(e.currentTarget.checked);
            setPage(1);
          }}
        />
        <Text size="sm" c="dimmed">
          {t("rows", { shown: rows.length, total: data?.length ?? 0 })}
        </Text>
      </Group>
      <DataTable
        mt="md"
        striped
        highlightOnHover
        fetching={isFetching}
        records={shown}
        idAccessor={(r) => `${r.item_id}|${r.sub_type}`}
        totalRecords={rows.length}
        recordsPerPage={PAGE}
        page={page}
        onPageChange={setPage}
        sortStatus={sort}
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
          { accessor: "name", title: t("columns.item"), sortable: true },
          { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
          { accessor: "volume", title: t("columns.volume"), sortable: true, render: (r) => num(r.volume, 2) },
          { accessor: "median", title: t("columns.median"), sortable: true, render: (r) => num(r.median) },
          { accessor: "moving_avg", title: t("columns.moving_avg"), sortable: true, render: (r) => num(r.moving_avg) },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => num(r.profit) },
          { accessor: "min_price", title: t("columns.min_price"), sortable: true, render: (r) => r.min_price ?? "—" },
          { accessor: "max_price", title: t("columns.max_price"), sortable: true, render: (r) => r.max_price ?? "—" },
          { accessor: "history_days", title: t("columns.history_days"), sortable: true },
          {
            accessor: "warm",
            title: t("columns.warm"),
            sortable: true,
            render: (r) => <Badge color={r.warm ? "green" : "gray"}>{r.warm ? "✓" : "—"}</Badge>,
          },
          { accessor: "updated_at", title: t("columns.updated_at"), sortable: true, render: (r) => dayjs(r.updated_at).format("MM-DD HH:mm") },
        ]}
      />
    </>
  );
}
