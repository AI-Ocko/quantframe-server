import { useMemo } from "react";
import { Tabs } from "@mantine/core";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { ItemPanel, CustomPanel } from "./Tabs";
import { useLocalStorage } from "@mantine/hooks";
import classes from "./TradeMessages.module.css";

export default function TradeMessagesPage() {
  // Translate general
  const useTranslateForm = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslatePages(`trade_messages.${key}`, { ...context }, i18Key);
  const useTranslateTabs = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateForm(`tabs.${key}`, { ...context }, i18Key);

  const tabs = useMemo(() => [
    {
      label: useTranslateTabs("item.title"),
      component: (isActive: boolean) => <ItemPanel isActive={isActive} />,
      id: "item",
    },
    {
      label: useTranslateTabs("custom.title"),
      component: (isActive: boolean) => <CustomPanel isActive={isActive} />,
      id: "custom",
    },
  ], []);

  const [activeTab, setActiveTab] = useLocalStorage<string>({
    key: "trade_messages_active_tab",
    defaultValue: tabs[0].id,
  });

  return (
    <Tabs value={activeTab} onChange={(value) => setActiveTab(value || tabs[0].id)} data-has-alert={false} className={classes.tabs}>
      <Tabs.List>
        {tabs.map((tab) => (
          <Tabs.Tab value={tab.id} key={tab.id}>
            {tab.label}
          </Tabs.Tab>
        ))}
      </Tabs.List>
      {tabs.map((tab) => (
        <Tabs.Panel value={tab.id} key={tab.id}>
          {activeTab === tab.id && tab.component(true)}
        </Tabs.Panel>
      ))}
    </Tabs>
  );
}
