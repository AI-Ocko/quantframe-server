import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Code, Group, Modal, Stack, Table, Text, TextInput } from "@mantine/core";
import { modals } from "@mantine/modals";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

export function HelperDevicesPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.helper_devices.${key}`, context);
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [created, setCreated] = useState<TauriTypes.HelperDeviceCreated | null>(null);
  const { data: devices } = useQuery({
    queryKey: ["helper_devices"],
    queryFn: () => api.helper_link.devices(),
    refetchInterval: 15_000,
    enabled: !!isActive,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["helper_devices"] });
  const create = useMutation({
    mutationFn: (deviceName: string) => api.helper_link.create(deviceName),
    onSuccess: (result) => {
      setCreated(result);
      setName("");
    },
    onSettled: refresh,
  });
  const revoke = useMutation({ mutationFn: (id: number) => api.helper_link.revoke(id), onSettled: refresh });
  const failure = create.error ?? revoke.error;

  const confirmRevoke = (device: TauriTypes.HelperDevice) =>
    modals.openConfirmModal({
      title: t("revoke_title"),
      children: <Text size="sm">{t("revoke_message", { name: device.name })}</Text>,
      labels: { confirm: t("revoke"), cancel: t("cancel") },
      confirmProps: { color: "red" },
      onConfirm: () => revoke.mutate(device.id),
    });

  return (
    <Stack mt="md">
      <Text size="sm" c="dimmed">
        {t("description")}
      </Text>
      <Group align="flex-end">
        <TextInput label={t("name")} placeholder={t("name_placeholder")} value={name} maxLength={64} onChange={(e) => setName(e.currentTarget.value)} />
        <Button disabled={!name.trim()} loading={create.isPending} onClick={() => create.mutate(name.trim())}>
          {t("create")}
        </Button>
      </Group>
      {failure && <Alert color="red">{String((failure as any)?.message ?? failure)}</Alert>}
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("columns.name")}</Table.Th>
            <Table.Th>{t("columns.created_at")}</Table.Th>
            <Table.Th>{t("columns.last_seen_at")}</Table.Th>
            <Table.Th>{t("columns.status")}</Table.Th>
            <Table.Th />
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(devices ?? []).map((device) => (
            <Table.Tr key={device.id}>
              <Table.Td>{device.name}</Table.Td>
              <Table.Td>{device.created_at}</Table.Td>
              <Table.Td>{device.last_seen_at ?? "—"}</Table.Td>
              <Table.Td>
                {device.revoked_at ? <Badge color="gray">{t("revoked")}</Badge> : <Badge color="green">{t("active")}</Badge>}
              </Table.Td>
              <Table.Td>
                {!device.revoked_at && (
                  <Button size="xs" color="red" variant="light" onClick={() => confirmRevoke(device)}>
                    {t("revoke")}
                  </Button>
                )}
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Modal opened={!!created} onClose={() => setCreated(null)} title={t("created_title", { name: created?.device.name ?? "" })} size="lg">
        <Stack>
          <Alert color="yellow">{t("created_warning")}</Alert>
          <Text size="sm">{t("created_config")}</Text>
          <Code block>{`server_url = "${window.location.origin}"\ndevice_key = "${created?.key ?? ""}"`}</Code>
          <Group justify="flex-end">
            <Button onClick={() => setCreated(null)}>{t("done")}</Button>
          </Group>
        </Stack>
      </Modal>
    </Stack>
  );
}
