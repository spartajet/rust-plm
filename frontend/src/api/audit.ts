import type { AxiosInstance } from "axios";

export interface AuditRow {
  id: number;
  at: string;
  username: string;
  action: string;
  object?: string | null;
  detail?: Record<string, unknown> | null;
  trace_id?: string | null;
  ip?: string | null;
}

export async function searchAudit(
  http: AxiosInstance,
  filter: { username?: string; action?: string; object?: string; page?: number; size?: number },
): Promise<{ items: AuditRow[]; total: number; page: number; size: number }> {
  const q = new URLSearchParams();
  for (const k of ["username", "action", "object"] as const) {
    const v = filter[k];
    if (v?.trim()) q.set(k, v.trim());
  }
  q.set("page", String(filter.page ?? 1));
  q.set("size", String(filter.size ?? 50));
  const res = await http.get(`/api/v1/audit?${q.toString()}`);
  return res.data;
}
