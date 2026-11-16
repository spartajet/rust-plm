import type { AxiosInstance } from "axios";

/// DOC-12 对象关联（增量27）：物料↔文档
export interface LinkItem {
  link_id: string;
  obj_no: string;
  name?: string | null;
  class_name?: string | null;
}

export async function docLinks(http: AxiosInstance, objNo: string): Promise<LinkItem[]> {
  const res = await http.get(`/api/v1/docs/${objNo}/links`);
  return res.data.items;
}

export async function createLink(
  http: AxiosInstance,
  fromNo: string,
  toNo: string,
): Promise<void> {
  await http.post("/api/v1/links", { from_no: fromNo, to_no: toNo, kind: "doc_part" });
}

export async function deleteLink(http: AxiosInstance, linkId: string): Promise<void> {
  await http.delete(`/api/v1/links/${linkId}`);
}
