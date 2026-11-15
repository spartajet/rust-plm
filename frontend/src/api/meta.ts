import type { AxiosInstance } from "axios";

/// AGL-09 实施向导步（增量24）
export interface ChecklistStep {
  key: string;
  title: string;
  hint: string;
  done: boolean;
  auto: boolean;
}

export async function getChecklist(http: AxiosInstance): Promise<ChecklistStep[]> {
  const res = await http.get("/api/v1/meta/impl-checklist");
  return res.data;
}

export async function completeStep(http: AxiosInstance, step: string): Promise<void> {
  await http.post(`/api/v1/meta/impl-checklist/${step}/complete`);
}

/// AGL-05 模板包（增量24）
export interface TemplatePackage {
  format: string;
  version: number;
  name?: string;
  types: { code: string; name: string; attrs: unknown[] }[];
  classes: { code: string; name: string }[];
}

export interface ImportResult {
  applied_types: string[];
  applied_classes: string[];
  conflicts: { kind: string; key: string }[];
}

export async function getStarter(http: AxiosInstance): Promise<TemplatePackage> {
  const res = await http.get("/api/v1/meta/template/starter");
  return res.data;
}

export async function importTemplate(
  http: AxiosInstance,
  template: TemplatePackage | Record<string, unknown>,
  mode: "dry_run" | "merge",
): Promise<ImportResult> {
  const res = await http.post("/api/v1/meta/template/import", { template, mode });
  return res.data;
}

/// 步骤跳转路由（纯函数可单测）
export const STEP_ROUTE: Record<string, string> = {
  org: "/admin/users",
  perm: "/admin/users",
  coding: "/parts",
  types: "/docs",
  flow: "/docs",
  data: "/docs",
  training: "/",
  cutover: "/",
};

export function stepRoute(key: string): string {
  return STEP_ROUTE[key] ?? "/";
}
