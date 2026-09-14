import { ResponseError, TauriTypes } from "$types";
import api from "@api/index";
import { SplashScreen } from "@components/Layouts/Shared/SplashScreen";
import { TextTranslate } from "@components/Shared/TextTranslate";
import { useTauriEvent } from "@hooks/useTauriEvent.hook";
import { useTranslateCommon, useTranslateContexts } from "@hooks/useTranslate.hook";
import { useLocalStorage } from "@mantine/hooks";
import { notifications } from "@mantine/notifications";
import { rpcInvoke as invoke } from "@api/transport";
import { listen } from "@api/socket";
import { PlaySound } from "@utils/helper";
import i18n from "i18next";
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { AppError } from "../model";
import { AuthContextProvider } from "./auth.context";
import { CacheContextProvider } from "./cache.context";
import { LiveScraperContextProvider } from "./liveScraper.context";
export async function loadLanguage(lang: string) {
  try {
    const response = await fetch(`/lang/${lang}.json`);
    const translations = await response.json();
    // Add the translations to i18next
    i18n.addResourceBundle(lang, "translation", translations, true, true);
    await i18n.changeLanguage(lang);
    console.log(`Language "${lang}" loaded successfully.`);
  } catch (err) {
    console.error("Failed to load language:", err);
  }
}
interface NotificationData {
  i18n_key: string;
  color: string;
  type: string;
  settings: {
    autoClose?: number | false;
  };
  values?: Record<string, any>;
}
export type AppContextProps = {
  app_info: TauriTypes.AppInfo | undefined;
  app_error: AppError | undefined;
  settings: TauriTypes.Settings | undefined;
  loading?: boolean;
  setLang?: (lang: string) => void;
};

export type AppContextProviderProps = {
  children: React.ReactNode;
};
export const AppContext = createContext<AppContextProps>({
  settings: undefined,
  app_info: undefined,
  app_error: undefined,
});

export const useAppContext = () => useContext(AppContext);
export const useIsDev = () => {
  const { app_info } = useAppContext();
  return app_info?.is_dev ?? false;
};
export const useAppError = () => {
  const { app_error } = useAppContext();
  return app_error;
};

export function AppContextProvider({ children }: AppContextProviderProps) {
  const [error, setError] = useState<AppError | undefined>(undefined);

  if (window.location.href.includes("clean"))
    return <AppContext.Provider value={{ settings: undefined, app_info: undefined, app_error: error }}>{children}</AppContext.Provider>;

  const { data: settings, refetch: refetchSettings } = api.app.get_settings();
  const { data: app_info, refetch: refetchAppInfo } = api.app.get_app_info();
  const [startingUp, setStartingUp] = useState<{ i18n_key: string; values: {} }>({ i18n_key: "starting_up", values: {} });
  const [loading, setLoading] = useState(true);
  const [lang, setLang] = useLocalStorage<string>({ key: "app_language", defaultValue: "en" });

  const handleAppError = (error: ResponseError | undefined) => {
    // setError(error ? new AppError(error) : undefined);
    setError(() => {
      if (!error || Object.keys(error).length === 0) return undefined; // No error to set
      return error ? new AppError(error) : undefined;
    });
  };

  const handleOnNotify = ({ i18n_key, color, type, settings, values }: NotificationData) => {
    const key = `notifications.${i18n_key}.${type}`;

    notifications.show({
      title: useTranslateCommon(`${key}.title`, values),
      color,
      autoClose: settings.autoClose ?? 3000,
      message: <TextTranslate i18nKey={`common.${key}.message`} values={values} />,
    });
  };


  useEffect(() => {
    loadLanguage(lang);
  }, [lang]);


  const InitializeApp = async () => {
    await refetchAppInfo();
    await refetchSettings();
    setLoading(false);
  };


  // Hook on tauri events from rust side
  useTauriEvent(TauriTypes.Events.OnError, handleAppError, []);
  useTauriEvent(TauriTypes.Events.RefreshSettings, refetchSettings, []);
  useTauriEvent(TauriTypes.Events.OnNotify, handleOnNotify, []);
  useTauriEvent(TauriTypes.Events.OnStartingUp, setStartingUp, []);

  useEffect(() => {
    invoke("initialized")
      .then((wasInitialized) => (wasInitialized ? InitializeApp() : console.log("App was not initialized")))
      .catch((e) => console.error("Error checking initialization:", e));
    listen("app:ready", () => InitializeApp());
    listen<{ file_name: string; volume: number }>("play_sound", ({ payload }) => {
      PlaySound(payload.file_name, payload.volume).catch((error) => {
        console.error("Error playing sound:", error);
      });
    });
    return () => {};
  }, []);
  const contextValue = useMemo(
    () => ({
      settings,
      app_info,
      app_error: error,
      loading,
      setLang,
    }),
    [settings, app_info, error, loading, setLang],
  );

  return (
    <AppContext.Provider value={contextValue}>
      <SplashScreen opened={loading} text={useTranslateContexts(`app.${startingUp.i18n_key}`, startingUp.values)} />
      {!loading && (
        <AuthContextProvider>
          <LiveScraperContextProvider>
            <CacheContextProvider>{children}</CacheContextProvider>
          </LiveScraperContextProvider>
        </AuthContextProvider>
      )}
    </AppContext.Provider>
  );
}
