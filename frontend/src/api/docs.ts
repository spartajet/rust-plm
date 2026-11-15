import type { AxiosInstance } from "axios";

export interface BizObject {
  id: string;
  type_code: string;
  obj_no: string;
  major: number;
  minor: number;
  state: string;
  attrs: Record<string, unknown>;
  attrs_rev: number;
  secret_level: number;
}

export interface DocResponse {
  object: BizObject;
  file?: { name: string; hash: string; size: number; mime?: string };
}

export interface ListResponse {
  items: BizObject[];
  total: number;
  page: number;
  size: number;
}

/// 列表查询参数（计划 Review Focus #3：与后端契约一致的拼装）
export interface ListParams {
  material?: string;
  state?: string;
  page?: number;
  size?: number;
  sort?: string;
}

export function buildListQuery(p: ListParams): string {
  const q = new URLSearchParams();
  if (p.material) q.set("filter[material]", p.material);
  if (p.state) q.set("filter[state]", p.state);
  if (p.sort) q.set("sort", p.sort);
  q.set("page", String(p.page ?? 1));
  q.set("size", String(p.size ?? 20));
  return q.toString();
}

export async function listDocs(
  http: AxiosInstance,
  p: ListParams,
): Promise<ListResponse> {
  const res = await http.get<ListResponse>(`/api/v1/docs?${buildListQuery(p)}`);
  return res.data;
}

export async function getDoc(http: AxiosInstance, objNo: string): Promise<DocResponse> {
  const res = await http.get<DocResponse>(`/api/v1/docs/${objNo}`);
  return res.data;
}

export async function uploadDoc(
  http: AxiosInstance,
  form: { obj_no: string; attrs: Record<string, unknown>; file: File; auto_code?: boolean },
): Promise<DocResponse> {
  const fd = new FormData();
  fd.set("obj_no", form.obj_no);
  if (form.auto_code) fd.set("auto_code", "1");
  if (Object.keys(form.attrs).length) {
    fd.set("attrs", JSON.stringify(form.attrs));
  }
  fd.set("file", form.file);
  const res = await http.post<DocResponse>("/api/v1/docs", fd);
  return res.data;
}

export type LifecycleAction = "submit" | "approve" | "reject" | "obsolete" | "revise";

export async function transition(
  http: AxiosInstance,
  objNo: string,
  action: LifecycleAction,
): Promise<BizObject> {
  const res = await http.post<BizObject>(`/api/v1/docs/${objNo}/${action}`);
  return res.data;
}

export async function downloadFile(http: AxiosInstance, objNo: string): Promise<Blob> {
  // blob 下载走带 Authorization 的共享 http 实例——window.open 新窗口无鉴权头必 401（增量8 终审 I3）
  const res = await http.get(`/api/v1/docs/${objNo}/file`, { responseType: "blob" });
  return res.data as Blob;
}

/// BOM 展开行（NDJSON 解析后）
export interface BomRow {
  child: string;
  qty_total: number;
}

export async function bomExpand(
  http: AxiosInstance,
  objNo: string,
  mode: "full" | "leaf" | "onelevel" = "full",
  major?: number,
): Promise<BomRow[]> {
  // major=N → 发布快照 as-of 展开（增量9 BOM 版本链）；缺省=live
  // responseType:"text"——NDJSON 单行时是合法 JSON，axios 默认 json 模式会解析成对象
  // 导致 String(res.data)="[object Object]"、展开结果为空（增量11 e2e 实证）
  const res = await http.get(
    `/api/v1/boms/${objNo}/expand?mode=${mode}${major !== undefined ? `&major=${major}` : ""}`,
    { responseType: "text" },
  );
  const text = String(res.data ?? "");
  if (!text.trim()) return [];
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as BomRow);
}

/// as-of 历史反查行（增量14：发布快照中 child 被谁使用）
export interface WhereUsedAsofRow {
  release_root_no: string;
  major: number;
  parent_no: string;
  qty_total: number;
}

export async function whereUsedAsof(http: AxiosInstance, objNo: string): Promise<WhereUsedAsofRow[]> {
  // responseType text——NDJSON（同 bomExpand 的单行 JSON 陷阱）
  const res = await http.get(`/api/v1/boms/${objNo}/where-used?asof=1`, { responseType: "text" });
  const text = String(res.data ?? "");
  if (!text.trim()) return [];
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as WhereUsedAsofRow);
}

/// live 反查行（增量15：对称过滤+ancestor_no 回填——与 asof 同构）
export interface WhereUsedLiveRow {
  ancestor: string;
  ancestor_no: string;
  qty_total: number;
}

export async function whereUsedLive(http: AxiosInstance, objNo: string): Promise<WhereUsedLiveRow[]> {
  const res = await http.get(`/api/v1/boms/${objNo}/where-used`, { responseType: "text" });
  const text = String(res.data ?? "");
  if (!text.trim()) return [];
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as WhereUsedLiveRow);
}

/// 版本历史快照行（增量8）
export interface VersionSnapshot {
  major: number;
  minor: number;
  state: string;
  file_hash?: string | null;
  created_at?: string | null;
}

export async function listVersions(
  http: AxiosInstance,
  objNo: string,
): Promise<{ current: BizObject; snapshots: VersionSnapshot[] }> {
  const res = await http.get(`/api/v1/docs/${objNo}/versions`);
  return res.data;
}

/// NDJSON 行解析（纯函数可单测）
export function parseNdjson(text: string): Record<string, unknown>[] {
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as Record<string, unknown>);
}

/// BOM 富行（增量21 BOM-01：直接子件+编号/名称；child_id=提交 set_lines 的 uuid）
export interface BomLineInfo {
  line_no: number;
  child_id: string;
  child_obj_no: string;
  child_name?: string | null;
  qty: number;
}

export async function bomLines(
  http: AxiosInstance,
  objNo: string,
  major?: number,
): Promise<BomLineInfo[]> {
  const q = major ? `?major=${major}` : "";
  const res = await http.get(`/api/v1/boms/${objNo}/lines${q}`);
  return res.data;
}

/// 全量替换 BOM 行（增量21 编辑器保存；循环引用 → 409）
export async function setBomLines(
  http: AxiosInstance,
  objNo: string,
  lines: { line_no: number; child: string; qty: number }[],
): Promise<void> {
  await http.post(`/api/v1/boms/${objNo}/lines`, lines);
}

/// 编辑态行（qty 可空=待填写）
export interface BomEditRow {
  child_id: string;
  child_obj_no: string;
  child_name?: string | null;
  qty: number | null;
}

/// 纯函数（可单测）：编辑行 → set_lines payload（line_no 重排 1..n；child 去重由 UI 兜底）
export function buildSetLinesPayload(rows: BomEditRow[]): {
  line_no: number;
  child: string;
  qty: number;
}[] {
  return rows.map((r, i) => ({
    line_no: i + 1,
    child: r.child_id,
    qty: r.qty ?? 1,
  }));
}

/// BOM-07 汇总行（叶子用量多路径和+分类上下文）
export interface BomSummaryRow {
  obj_no: string;
  name?: string | null;
  class_code?: string | null;
  class_name?: string | null;
  qty_total: number;
}

export async function bomSummary(
  http: AxiosInstance,
  objNo: string,
  major?: number,
): Promise<BomSummaryRow[]> {
  const q = major ? `?major=${major}` : "";
  const res = await http.get(`/api/v1/boms/${objNo}/summary${q}`);
  return res.data;
}

/// CSV 下载（后端 text/csv + UTF-8 BOM）
export async function downloadBomSummaryCsv(
  http: AxiosInstance,
  objNo: string,
  major?: number,
): Promise<Blob> {
  const q = major ? `?major=${major}&format=csv` : "?format=csv";
  const res = await http.get(`/api/v1/boms/${objNo}/summary${q}`, { responseType: "blob" });
  return res.data as Blob;
}

/// 在线预览（增量22 DOC-05）：blob + content-type（MIME 白名单由后端把关；415=不支持）
export async function previewDoc(
  http: AxiosInstance,
  objNo: string,
): Promise<{ blob: Blob; mime: string }> {
  const res = await http.get(`/api/v1/docs/${objNo}/preview`, { responseType: "blob" });
  const ct = (res.headers?.["content-type"] as string | undefined) ?? "";
  return { blob: res.data as Blob, mime: ct.split(";")[0].trim() };
}
