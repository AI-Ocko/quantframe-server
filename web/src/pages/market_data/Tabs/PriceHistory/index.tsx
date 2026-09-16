import api from "@api/index";
import { SelectTradableItem } from "@components/Forms/SelectTradableItem";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Box, Group, Paper, SegmentedControl, Select, SimpleGrid, Stack, Table, Text, useMantineTheme } from "@mantine/core";
import { num } from "@utils/sortRows";
import dayjs from "dayjs";
import { useEffect, useState } from "react";
import { Bar, Line } from "react-chartjs-2";
import classes from "../../MarketData.module.css";
import { takeSelection } from "../../selection";

export function PriceHistoryPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.price_history.${key}`, context);
  const theme = useMantineTheme();
  const [wfmUrl, setWfmUrl] = useState<string>("");
  const [subType, setSubType] = useState<string | undefined>(undefined);
  const [days, setDays] = useState<string>("7");
  useEffect(() => {
    if (!isActive) return;
    const selection = takeSelection();
    if (selection) {
      setWfmUrl(selection.slug);
      setSubType(selection.sub_type || undefined);
    }
  }, [isActive]);
  const { data, error } = api.collector.itemHistory(wfmUrl || undefined, subType, Number(days));
  if (!isActive) return null;

  const stats = data?.stats;
  return (
    <Stack mt="md">
      <Group align="end">
        <Box w={420}>
          <SelectTradableItem
            hideSubType
            value={wfmUrl}
            onChange={(item) => {
              setWfmUrl(item.wfmUrl);
              setSubType(undefined);
            }}
          />
        </Box>
        <Select
          label={t("sub_type")}
          data={(data?.sub_types ?? []).map((s) => ({ value: s, label: s === "" ? t("no_sub_type") : s }))}
          value={data?.sub_type ?? null}
          onChange={(value) => setSubType(value ?? undefined)}
          disabled={!data || data.sub_types.length < 2}
        />
        <SegmentedControl data={["1", "7", "30", "90"].map((d) => ({ value: d, label: t("days", { days: d }) }))} value={days} onChange={setDays} />
      </Group>
      {error && <Alert color="red">{String((error as any)?.message ?? error)}</Alert>}
      {!wfmUrl && <Text c="dimmed">{t("pick_item")}</Text>}
      {data && (
        <>
          <Group>
            <Badge color={stats?.warm ? "green" : "gray"}>{stats?.warm ? t("warm") : t("warming_up", { days: stats?.history_days ?? 0 })}</Badge>
            <Text size="sm" c="dimmed">
              {t("last_swept", { at: data.last_swept_at ? dayjs(data.last_swept_at).format("YYYY-MM-DD HH:mm:ss") : "—" })}
            </Text>
          </Group>
          <SimpleGrid cols={{ base: 2, md: 4, lg: 7 }}>
            {[
              [t("volume"), num(stats?.volume, 2)],
              [t("avg_price"), num(stats?.avg_price)],
              [t("moving_avg"), num(stats?.moving_avg)],
              [t("median"), num(stats?.median)],
              [t("profit"), num(stats?.profit)],
              [t("min_price"), num(stats?.min_price, 0)],
              [t("max_price"), num(stats?.max_price, 0)],
            ].map(([label, value]) => (
              <Paper withBorder p="sm" key={label}>
                <Text size="xs" c="dimmed">
                  {label}
                </Text>
                <Text fw={700}>{value}</Text>
              </Paper>
            ))}
          </SimpleGrid>
          <Paper withBorder p="sm">
            <Text fw={600} mb="xs">
              {t("hourly_title")}
            </Text>
            <Box className={classes.chart}>
              <Line
                options={{ responsive: true, maintainAspectRatio: false, spanGaps: true }}
                data={{
                  labels: data.hourly.map((h) => dayjs(h.hour).format("MM-DD HH:mm")),
                  datasets: [
                    {
                      label: t("min_sell"),
                      data: data.hourly.map((h) => h.min_sell_avg ?? null),
                      borderColor: theme.colors.green[6],
                      backgroundColor: theme.colors.green[6],
                    },
                    {
                      label: t("max_buy"),
                      data: data.hourly.map((h) => h.max_buy_avg ?? null),
                      borderColor: theme.colors.blue[6],
                      backgroundColor: theme.colors.blue[6],
                    },
                  ],
                }}
              />
            </Box>
          </Paper>
          <Paper withBorder p="sm">
            <Text fw={600} mb="xs">
              {t("daily_title")}
            </Text>
            <Box className={classes.chart}>
              <Bar
                options={{ responsive: true, maintainAspectRatio: false }}
                data={{
                  labels: data.daily.map((d) => d.day),
                  datasets: [{ label: t("trades"), data: data.daily.map((d) => d.volume), backgroundColor: theme.colors.violet[6] }],
                }}
              />
            </Box>
          </Paper>
          <SimpleGrid cols={{ base: 1, md: 2 }}>
            <Paper withBorder p="sm">
              <Text fw={600} mb="xs">
                {t("book_title")}
              </Text>
              {!data.book ? (
                <Text c="dimmed">{t("no_book")}</Text>
              ) : (
                <>
                  <Text size="xs" c="dimmed">
                    {t("last_swept", { at: dayjs(data.book.swept_at).format("YYYY-MM-DD HH:mm:ss") })}
                  </Text>
                  <SimpleGrid cols={2}>
                    {(["sells", "buys"] as const).map((side) => (
                      <div key={side}>
                        <Text fw={500}>{t(side)}</Text>
                        {(side === "sells" ? data.book!.top_sells : data.book!.top_buys).map(([platinum, quantity], i) => (
                          <Text key={i} size="sm">
                            {platinum} p × {quantity}
                          </Text>
                        ))}
                      </div>
                    ))}
                  </SimpleGrid>
                </>
              )}
            </Paper>
            <Paper withBorder p="sm">
              <Text fw={600} mb="xs">
                {t("trades_title")}
              </Text>
              {data.trades.length === 0 ? (
                <Text c="dimmed">{t("no_trades")}</Text>
              ) : (
                <Table striped withTableBorder>
                  <Table.Thead>
                    <Table.Tr>
                      <Table.Th>{t("vanished_at")}</Table.Th>
                      <Table.Th>{t("side")}</Table.Th>
                      <Table.Th>{t("platinum")}</Table.Th>
                      <Table.Th>{t("quantity")}</Table.Th>
                    </Table.Tr>
                  </Table.Thead>
                  <Table.Tbody>
                    {data.trades.map((trade, i) => (
                      <Table.Tr key={i}>
                        <Table.Td>{dayjs(trade.vanished_at).format("MM-DD HH:mm")}</Table.Td>
                        <Table.Td>{trade.side}</Table.Td>
                        <Table.Td>{trade.platinum}</Table.Td>
                        <Table.Td>{trade.quantity}</Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              )}
            </Paper>
          </SimpleGrid>
        </>
      )}
    </Stack>
  );
}
