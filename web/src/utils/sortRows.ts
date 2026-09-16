import { DataTableSortStatus } from "mantine-datatable";

export const num = (value?: number | null, digits = 1) => (value == null ? "—" : value.toFixed(digits));

/** Client-side sort on one column; strings compare with localeCompare, numbers and booleans numerically, nulls last. */
export function sortRows<T>(rows: T[], status: DataTableSortStatus<T>): T[] {
  const key = status.columnAccessor as keyof T;
  const dir = status.direction === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = a[key] as unknown,
      y = b[key] as unknown;
    if (x == null && y == null) return 0;
    if (x == null) return 1;
    if (y == null) return -1;
    if (typeof x === "number" && typeof y === "number") return (x - y) * dir;
    if (typeof x === "boolean" && typeof y === "boolean") return (Number(x) - Number(y)) * dir;
    return String(x).localeCompare(String(y)) * dir;
  });
}
