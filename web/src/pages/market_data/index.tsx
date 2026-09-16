import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Tabs } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { useMemo } from "react";
import { CollectorPanel, OverviewPanel, PriceHistoryPanel, WarmupPanel } from "./Tabs";
import classes from "./MarketData.module.css";

export default function MarketDataPage() {
  const useTranslateTabs = (key: string) => useTranslatePages(`market_data.tabs.${key}`);

  const [activeTab, setActiveTab] = useLocalStorage<string>({ key: "market_data_active_tab", defaultValue: "collector" });

  const tabs = useMemo(
    () => [
      { id: "collector", label: useTranslateTabs("collector.title"), component: (isActive: boolean) => <CollectorPanel isActive={isActive} /> },
      {
        id: "overview",
        label: useTranslateTabs("overview.title"),
        component: (isActive: boolean) => <OverviewPanel isActive={isActive} onOpenItem={() => setActiveTab("price_history")} />,
      },
      { id: "warmup", label: useTranslateTabs("warmup.title"), component: (isActive: boolean) => <WarmupPanel isActive={isActive} /> },
      { id: "price_history", label: useTranslateTabs("price_history.title"), component: (isActive: boolean) => <PriceHistoryPanel isActive={isActive} /> },
    ],
    [],
  );

  return (
    <Tabs value={activeTab} onChange={(value) => setActiveTab(value || tabs[0].id)} className={classes.tabs}>
      <Tabs.List>
        {tabs.map((tab) => (
          <Tabs.Tab value={tab.id} key={tab.id}>
            {tab.label}
          </Tabs.Tab>
        ))}
      </Tabs.List>
      {tabs.map((tab) => (
        <Tabs.Panel value={tab.id} key={tab.id}>
          {tab.component(activeTab === tab.id)}
        </Tabs.Panel>
      ))}
    </Tabs>
  );
}
