import type { AxiosInstance } from "axios";

export interface ClassNode {
  id: string;
  code: string;
  name: string;
  has_rule: boolean;
  children: ClassNode[];
}

export interface Duplicate {
  obj_no: string;
  name: string;
  material?: string | null;
}

export interface PartRow {
  obj_no: string;
  major: number;
  minor: number;
  state: string;
  attrs: Record<string, unknown>;
  secret_level: number;
}

export async function classTree(http: AxiosInstance): Promise<ClassNode[]> {
  const res = await http.get<ClassNode[]>("/api/v1/classes");
  return res.data;
}

export async function createPart(
  http: AxiosInstance,
  form: { class_code: string; attrs: Record<string, unknown> },
): Promise<{ obj_no: string; class_code: string; duplicates: Duplicate[] }> {
  const res = await http.post("/api/v1/parts", form);
  return res.data;
}

export async function listParts(
  http: AxiosInstance,
  page = 1,
  size = 20,
  name?: string,
): Promise<{ items: PartRow[]; total: number }> {
  const q = new URLSearchParams({ page: String(page), size: String(size) });
  if (name && name.trim()) q.set("name", name.trim());
  const res = await http.get(`/api/v1/parts?${q.toString()}`);
  return res.data;
}

export async function duplicates(
  http: AxiosInstance,
  name: string,
  material?: string,
): Promise<Duplicate[]> {
  const q = new URLSearchParams({ name });
  if (material?.trim()) q.set("material", material.trim());
  const res = await http.get(`/api/v1/parts/duplicates?${q.toString()}`);
  return res.data;
}
