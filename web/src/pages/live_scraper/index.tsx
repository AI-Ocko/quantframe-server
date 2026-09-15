import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Container, Tabs } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { DryRunLogPanel, ItemPanel, WishListPanel } from "./Tabs";
import { TraderPanel } from "./TraderPanel";

export default function LiveScraperPage() {
  // Translate general
  const useTranslateForm = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslatePages(`live_scraper.${key}`, { ...context }, i18Key);
  const useTranslateTabs = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateForm(`tabs.${key}`, { ...context }, i18Key);

  const tabs = [
    {
      label: useTranslateTabs("item.title"),
      component: (isActive: boolean) => <ItemPanel isActive={isActive} />,
      id: "item",
    },
    {
      label: useTranslateTabs("wish_list.title"),
      component: (isActive: boolean) => <WishListPanel isActive={isActive} />,
      id: "wish_list",
    },
    {
      label: useTranslateForm("trader.dry_run_log.title"),
      component: (isActive: boolean) => <DryRunLogPanel isActive={isActive} />,
      id: "dry_run_log",
    },
  ];

  const [activeTab, setActiveTab] = useLocalStorage<string>({
    key: "live_scraper.active_tab",
    defaultValue: tabs[0].id,
  });

  return (
    <Container size={"100%"}>
      <TraderPanel />
      <Tabs value={activeTab} onChange={(value) => setActiveTab(value || tabs[0].id)}>
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
    </Container>
  );
}
