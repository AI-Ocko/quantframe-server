import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Tabs } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { useMemo } from "react";
import { ItemsPanel, PartnersPanel, StockPanel, TimelinePanel, TransactionPanel } from "./Tabs";
import classes from "./TradingAnalytics.module.css";

export default function TradingAnalyticsPage() {
  // Translate general
  const useTranslateForm = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslatePages(`trading_analytics.${key}`, { ...context }, i18Key);
  const useTranslateTabs = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateForm(`tabs.${key}`, { ...context }, i18Key);

  const tabs = useMemo(
    () => [
      {
        label: useTranslateTabs("transaction.title"),
        component: (isActive: boolean) => <TransactionPanel isActive={isActive} />,
        id: "transaction",
      },
      { label: useTranslateTabs("items.title"), component: (isActive: boolean) => <ItemsPanel isActive={isActive} />, id: "items" },
      { label: useTranslateTabs("stock.title"), component: (isActive: boolean) => <StockPanel isActive={isActive} />, id: "stock" },
      { label: useTranslateTabs("partners.title"), component: (isActive: boolean) => <PartnersPanel isActive={isActive} />, id: "partners" },
      { label: useTranslateTabs("timeline.title"), component: (isActive: boolean) => <TimelinePanel isActive={isActive} />, id: "timeline" },
    ],
    [],
  );

  const [activeTab, setActiveTab] = useLocalStorage<string>({
    key: "trading_analytics_active_tab",
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
