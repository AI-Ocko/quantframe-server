import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Group, List, Paper, Stack, Switch, Text, ThemeIcon } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

const STATE_COLOR: Record<TauriTypes.LifecycleState, string> = {
  offline: "gray",
  ready: "blue",
  trading: "green",
  stopping: "orange",
};

type OptionsInput = { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean };

export function TraderPanel() {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.${key}`, context);
  const queryClient = useQueryClient();
  const { data: status, error } = useQuery({
    queryKey: ["trader_status"],
    queryFn: () => api.live_scraper.status(),
    refetchInterval: 5000,
    retry: false,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["trader_status"] });
  const start = useMutation({ mutationFn: () => api.live_scraper.start(), onSettled: refresh });
  const stop = useMutation({ mutationFn: () => api.live_scraper.stop(), onSettled: refresh });
  const options = useMutation({ mutationFn: (input: OptionsInput) => api.live_scraper.setOptions(input), onSettled: refresh });

  if (error) return <Alert color="red">{String((error as any)?.message ?? error)}</Alert>;
  if (!status) return null;

  const active = status.state === "trading" || status.state === "stopping";
  const checks: Array<[string, boolean]> = [
    ["token_valid", status.checklist.token_valid],
    ["ws_connected", status.checklist.ws_connected],
    ["game_data_loaded", status.checklist.game_data_loaded],
    ["helper_connected", status.checklist.helper_connected],
    ["warframe_running", status.checklist.warframe_running],
  ];
  const failure = start.error ?? stop.error ?? options.error;

  return (
    <Paper withBorder p="md" my="md">
      <Stack gap="sm">
        <Group justify="space-between">
          <Group>
            <Badge color={STATE_COLOR[status.state]} size="lg">
              {t(`states.${status.state}`)}
            </Badge>
            <Badge color={status.options.dry_run ? "yellow" : "red"} variant="outline">
              {status.options.dry_run ? t("dry_run") : t("live")}
            </Badge>
            {status.running_since && (
              <Text size="sm" c="dimmed">
                {t("running_since", { at: status.running_since })}
              </Text>
            )}
          </Group>
          {active ? (
            <Button color="red" loading={stop.isPending || status.state === "stopping"} onClick={() => stop.mutate()}>
              {t("stop")}
            </Button>
          ) : (
            <Button disabled={status.state !== "ready"} loading={start.isPending} onClick={() => start.mutate()}>
              {t("start")}
            </Button>
          )}
        </Group>
        <List spacing={4} size="sm">
          {checks.map(([key, ok]) => (
            <List.Item
              key={key}
              icon={
                <ThemeIcon color={ok ? "green" : "red"} size={16} radius="xl">
                  {ok ? "✓" : "✕"}
                </ThemeIcon>
              }
            >
              {t(`checklist.${key}`)}
            </List.Item>
          ))}
        </List>
        <Text size="sm" c="dimmed">
          {status.helper.last_heartbeat_at
            ? t("helper_line", {
                device: status.helper.device_name ?? "",
                version: status.helper.version ?? "",
                seconds: status.helper.seconds_since_heartbeat ?? 0,
              })
            : t("helper_none")}
        </Text>
        <Group>
          <Switch
            label={t("options.dry_run")}
            checked={status.options.dry_run}
            disabled={active}
            onChange={(e) => options.mutate({ dryRun: e.currentTarget.checked })}
          />
          <Switch
            label={t("options.delete_buy_orders_on_stop")}
            checked={status.options.delete_buy_orders_on_stop}
            onChange={(e) => options.mutate({ deleteBuyOrdersOnStop: e.currentTarget.checked })}
          />
        </Group>
        {status.options.last_stop_reason && (
          <Text size="sm" c="dimmed">
            {t("last_stop", { reason: status.options.last_stop_reason, at: status.options.last_stop_at ?? "" })}
          </Text>
        )}
        {failure && <Alert color="red">{String((failure as any)?.message ?? failure)}</Alert>}
      </Stack>
    </Paper>
  );
}
