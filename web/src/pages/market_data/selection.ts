const KEY = "market_data_price_history_selection";

export interface PriceHistorySelection {
  slug: string;
  sub_type: string;
}

/** One-shot hand-off: reading the selection also consumes it, so it cannot clobber a later manual pick. */
export function takeSelection(): PriceHistorySelection | null {
  try {
    const raw = localStorage.getItem(KEY);
    localStorage.removeItem(KEY);
    return raw ? (JSON.parse(raw) as PriceHistorySelection) : null;
  } catch {
    return null;
  }
}

export function writeSelection(selection: PriceHistorySelection) {
  try {
    localStorage.setItem(KEY, JSON.stringify(selection));
  } catch {
    /* storage unavailable: the click still switches tabs */
  }
}
