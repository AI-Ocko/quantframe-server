import { TauriTypes } from "$types";
import { useTranslateForms } from "@hooks/useTranslate.hook";
import { Box, Button, Grid, Group } from "@mantine/core";
import { UseFormReturnType } from "@mantine/form";
import { notifications } from "@mantine/notifications";
export type LogPanelProps = {
  form: UseFormReturnType<TauriTypes.Settings>;
};
export const LogPanel = ({ form: _form }: LogPanelProps) => {
  // Translate general
  const useTranslateForm = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateForms(`settings.tabs.advanced.log.${key}`, { ...context }, i18Key);
  const useTranslateFormButtons = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateForm(`buttons.${key}`, { ...context }, i18Key);

  return (
    <Box p={"md"}>
      <Grid>
        <Grid.Col span={4}>
          <Group>
            <Button mt="md" onClick={() => notifications.cleanQueue()} color="blue">
              {useTranslateFormButtons("clean_notifications")}
            </Button>
          </Group>
        </Grid.Col>
      </Grid>
    </Box>
  );
};

