import { createContext, useContext, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import api from "@api/index";
import { TauriTypes } from "$types";
import { useTauriEvent } from "@hooks/useTauriEvent.hook";

export type CacheContextProps = {
  tradableItems: TauriTypes.CacheTradableItem[];
  weapons: TauriTypes.CacheRivenWeapon[];
  isLoading: boolean;
};

export type CacheContextProviderProps = {
  children: React.ReactNode;
};

export const CacheContext = createContext<CacheContextProps>({
  tradableItems: [],
  weapons: [],
  isLoading: true,
});

export const useCacheContext = () => useContext(CacheContext);

export function CacheContextProvider({ children }: CacheContextProviderProps) {
  const {
    data: tradableItems,
    isLoading: isLoadingItems,
    refetch: refetchItems,
  } = useQuery({
    queryKey: ["cache_items"],
    queryFn: () => api.cache.getTradableItems(),
    staleTime: Infinity, // Cache forever - items rarely change
  });

  const contextValue = useMemo(
    () => ({
      tradableItems: tradableItems || [],
      weapons: [],
      isLoading: isLoadingItems,
    }),
    [tradableItems, isLoadingItems],
  );

  const handleRefresh = async () => {
    api.cache.clearCache(); // Clear in-memory cache to ensure fresh data
    await refetchItems();
  };

  // Use the custom hook for Tauri events
  useTauriEvent(TauriTypes.Events.RefreshCache, handleRefresh, []);

  return <CacheContext.Provider value={contextValue}>{children}</CacheContext.Provider>;
}
