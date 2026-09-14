import { useAppContext } from "@contexts/app.context";
import { lazy } from "react";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { routeLoaders } from "./routeLoaders";

// Layouts
import { LogInLayout } from "./LogIn";
import { LogOutLayout } from "./LogOut";

// Permissions Gate
import AuthenticatedGate from "../AuthenticatedGate";

// Lazy loaded pages for code splitting

// Home Routes
const PHome = lazy(routeLoaders.home);

// Auth Routes
const PLogin = lazy(routeLoaders.login);

// Debug Routes
const PDebug = lazy(routeLoaders.debug);

// Error Routes
const PError = lazy(routeLoaders.error);

// Live Scraper
const PLiveScraper = lazy(routeLoaders.liveScraper);

// Trading Analytics
const TradingAnalyticsPage = lazy(routeLoaders.tradingAnalytics);

// Warframe Market
const PWarframeMarket = lazy(routeLoaders.warframeMarket);

// Trade messages
const PTradeMessages = lazy(routeLoaders.tradeMessages);

// About Page
const AboutPage = lazy(routeLoaders.about);

export function AppRoutes() {
  const { app_error } = useAppContext();

  const ShowErrorPage = () => {
    if (!app_error) return false;
    if (app_error?.error.component == "WebSocket") return false;
    return true;
  };

  return (
    <BrowserRouter>
      <Routes>
        {!ShowErrorPage() && (
          <>
            <Route element={<AuthenticatedGate exclude goTo="/" />}>
              <Route path="/auth" element={<LogOutLayout />}>
                <Route path="login" element={<PLogin />} />
              </Route>
            </Route>
            <Route path="/" element={<LogInLayout />}>
              <Route element={<AuthenticatedGate goTo="/auth/login" />}>
                <Route path="/" element={<PHome />} />
                <Route path="debug">
                  <Route index element={<PDebug />} />
                </Route>
                <Route path="live_scraper" element={<PLiveScraper />} />
                <Route path="warframe-market" element={<PWarframeMarket />} />
                <Route path="trading_analytics" element={<TradingAnalyticsPage />} />
                <Route path="trade_messages" element={<PTradeMessages />} />
                <Route path="about" element={<AboutPage />} />
              </Route>
              <Route path="*" element={<PHome />} />
            </Route>
          </>
        )}
        {ShowErrorPage() && (
          <Route path="*" element={<LogOutLayout />}>
            <Route path="*" element={<PError />} />
          </Route>
        )}
      </Routes>
    </BrowserRouter>
  );
}
