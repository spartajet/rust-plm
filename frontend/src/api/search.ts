import type { AxiosInstance } from "axios";

/// 全局搜索命中行（增量20 RPT-01）
export interface SearchHit {
  type_code: string;
  obj_no: string;
  name?: string | null;
  state: string;
  major_ver: number;
  minor_ver: number;
  secret_level: number;
}

export interface SearchGroup {
  type_code: string;
  total: number;
  items: SearchHit[];
}

export interface SearchResponse {
  q: string;
  groups: SearchGroup[];
  total: number;
  page: number;
  size: number;
}

/// 全局搜索：无 type=聚类（每类型至多 10 条）；有 type=平铺分页钻取
export async function searchGlobal(
  http: AxiosInstance,
  q: string,
  type?: string,
  page = 1,
  size = 20,
): Promise<SearchResponse> {
  const res = await http.get("/api/v1/search", {
    params: type ? { q, type, page, size } : { q },
  });
  return res.data;
}

/// 聚类组展示名（纯函数可单测）
export const GROUP_LABEL: Record<string, string> = {
  drawing: "图纸",
  part: "物料",
};

export function groupLabel(typeCode: string): string {
  return GROUP_LABEL[typeCode] ?? typeCode;
}
