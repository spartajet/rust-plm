import type { AxiosInstance } from "axios";

/// 待办中心行（增量19 WF-03；todo/mine 盒 action/at/detail 为 null）
export interface TodoItem {
  obj_no: string;
  name?: string | null;
  state?: string | null;
  major_ver?: number | null;
  minor_ver?: number | null;
  secret_level?: number | null;
  submitter?: string | null;
  updated_at?: string | null;
  action?: string | null;
  at?: string | null;
  detail?: Record<string, unknown> | null;
}

export type TodoBox = "todo" | "done" | "mine";

/// 三盒查询（box=todo 缺省）
export async function listTodos(
  http: AxiosInstance,
  box: TodoBox,
  page = 1,
  size = 20,
): Promise<{ items: TodoItem[]; total: number; page: number; size: number }> {
  const res = await http.get("/api/v1/todos", { params: { box, page, size } });
  return res.data;
}

/// 批量审批（逐项结果；单件失败不中断批次）
export interface BatchResult {
  obj_no: string;
  ok: boolean;
  state?: string;
  error?: string;
}

export async function batchApprove(
  http: AxiosInstance,
  objNos: string[],
  comment?: string,
): Promise<{ items: BatchResult[] }> {
  const res = await http.post("/api/v1/docs/batch-approve", {
    obj_nos: objNos,
    comment: comment || undefined,
  });
  return res.data;
}

/// 批量结果聚合（纯函数可单测）：成功数 + 失败明细
export function summarizeBatch(items: BatchResult[]): {
  okCount: number;
  failures: { obj_no: string; error?: string }[];
} {
  const failures = items
    .filter((i) => !i.ok)
    .map((i) => ({ obj_no: i.obj_no, error: i.error }));
  return { okCount: items.length - failures.length, failures };
}
