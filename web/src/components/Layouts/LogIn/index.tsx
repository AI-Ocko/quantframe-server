import { AppShell, Box } from "@mantine/core";
import classes from "./LogInLayout.module.css";
import { Outlet, useNavigate } from "react-router-dom";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faBug, faGlobe, faHome, faInfoCircle, faMessage } from "@fortawesome/free-solid-svg-icons";
import { useTranslateComponent } from "@hooks/useTranslate.hook";
import { useCallback, useEffect, useMemo, useState } from "react";
import { NavbarLinkProps, NavbarMinimalColored } from "@components/Layouts/Shared/NavbarMinimalColored";
import { Header } from "@components/Layouts/Shared/Header";
import { open } from "@utils/openUrl";
import { AddMetric } from "@api/index";
import { faWarframeMarket, facTradingAnalytics } from "@icons";
import { prefetchRoute } from "../routeLoaders";

export function LogInLayout() {
  // States
  const [lastPage, setLastPage] = useState<string>("");
  // Translate general
  const useTranslate = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslateComponent(`layout.log_in.${key}`, { ...context }, i18Key);
  const useTranslateNavBar = (key: string, context?: { [key: string]: any }, i18Key?: boolean) =>
    useTranslate(`navbar.${key}`, { ...context }, i18Key);

  // States
  const navigate = useNavigate();
  const handleNavigate = useCallback(
    (link: NavbarLinkProps) => {
      if (link.web) open(link.link, "_blank");
      else navigate(link.link);

      if (link.id == lastPage || !link.id) return;
      setLastPage(link.id || "");
      AddMetric("active_page", link.id);
    },
    [navigate, lastPage],
  );

  const [devMode, setDevMode] = useState(false);

  const links = useMemo(
    () => [
      {
        align: "top",
        id: "home",
        link: "/",
        icon: <FontAwesomeIcon size={"lg"} icon={faHome} />,
        label: useTranslateNavBar("home"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("home"),
      },
      {
        align: "top",
        id: "live_scraper",
        link: "live_scraper",
        icon: <FontAwesomeIcon size={"lg"} icon={faGlobe} />,
        label: useTranslateNavBar("live_scraper"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("liveScraper"),
      },
      {
        align: "top",
        id: "warframe_market",
        link: "warframe-market",
        icon: <FontAwesomeIcon size={"xl"} icon={faWarframeMarket} />,
        label: useTranslateNavBar("warframe_market"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("warframeMarket"),
      },
      {
        align: "top",
        id: "trading_analytics",
        link: "trading_analytics",
        icon: <FontAwesomeIcon size={"lg"} icon={facTradingAnalytics} />,
        label: useTranslateNavBar("trading_analytics"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("tradingAnalytics"),
      },
      {
        align: "top",
        id: "trade_messages",
        link: "trade_messages",
        icon: <FontAwesomeIcon size={"lg"} icon={faMessage} />,
        label: useTranslateNavBar("trade_messages"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("tradeMessages"),
      },
      {
        align: "top",
        id: "debug",
        link: "debug",
        hide: !import.meta.env.DEV && !devMode,
        icon: <FontAwesomeIcon size={"lg"} icon={faBug} color="red" />,
        label: useTranslateNavBar("debug"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
      },

      {
        align: "bottom",
        id: "nav_about",
        link: "about",
        icon: <FontAwesomeIcon size={"lg"} icon={faInfoCircle} />,
        label: useTranslateNavBar("about"),
        onClick: (e: NavbarLinkProps) => handleNavigate(e),
        onPrefetch: () => prefetchRoute("about"),
      },
    ],
    [handleNavigate, devMode],
  );

  // Toggle dev mode when Shift is pressed.
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Shift" && !e.repeat) setDevMode((prev) => !prev);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  return (
    <AppShell
      classNames={classes}
      header={{ height: 65 }}
      navbar={{
        width: 70,
        breakpoint: "sm",
      }}
    >
      <AppShell.Header withBorder={false}>
        <Header />
      </AppShell.Header>

      <AppShell.Navbar withBorder={false}>
        <NavbarMinimalColored links={links} />
      </AppShell.Navbar>

      <AppShell.Main>
        <Box>
          <Outlet />
        </Box>
      </AppShell.Main>
    </AppShell>
  );
}

