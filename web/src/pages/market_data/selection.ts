const KEY = "market_data_price_history_selection";

export interface PriceHistorySelection {
  slug: string;
  sub_type: string;
}

export function readSelection(): PriceHistorySelection | null {
  try {
    const raw = localStorage.getItem(KEY);
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
