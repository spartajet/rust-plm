import type { AxiosInstance } from "axios";

/// 用户列表行（增量11：GET /api/v1/sec/users，sec:manage）
export interface SecUser {
  username: string;
  max_secret_level: number;
  created_at?: string | null;
  roles: string[];
  /// 部门（增量28 demo 数据包；系统无部门实体——列承载）
  department?: string | null;
}

export interface SecRole {
  code: string;
  name: string;
}

export interface UsersPage {
  items: SecUser[];
  total: number;
  page: number;
  size: number;
}

export async function listUsers(
  http: AxiosInstance,
  search?: string,
  page = 1,
  size = 50,
  department?: string,
): Promise<UsersPage> {
  const q = new URLSearchParams();
  if (search?.trim()) q.set("search", search.trim());
  if (department?.trim()) q.set("department", department.trim());
  q.set("page", String(page));
  q.set("size", String(size));
  const res = await http.get<UsersPage>(`/api/v1/sec/users?${q.toString()}`);
  return res.data;
}

export async function listRoles(http: AxiosInstance): Promise<SecRole[]> {
  const res = await http.get<SecRole[]>("/api/v1/sec/roles");
  return res.data;
}

export async function createUser(
  http: AxiosInstance,
  form: { username: string; password: string; max_secret_level: number; roles: string[] },
): Promise<void> {
  await http.post("/api/v1/sec/users", form);
}

export async function grantRole(
  http: AxiosInstance,
  username: string,
  role: string,
): Promise<void> {
  await http.post(`/api/v1/sec/users/${username}/roles/${role}`);
}

export async function revokeRole(
  http: AxiosInstance,
  username: string,
  role: string,
): Promise<void> {
  await http.delete(`/api/v1/sec/users/${username}/roles/${role}`);
}

/// SYS-04 安全参数（增量23）
export interface SysParam {
  key: string;
  value: string;
  updated_by?: string | null;
  updated_at?: string | null;
}

export async function listParams(http: AxiosInstance): Promise<SysParam[]> {
  const res = await http.get("/api/v1/sec/params");
  return res.data;
}

export async function setParam(
  http: AxiosInstance,
  key: string,
  value: number,
): Promise<void> {
  await http.put(`/api/v1/sec/params/${key}`, { value });
}

/// 参数键展示名（纯函数可单测）
export const PARAM_LABEL: Record<string, string> = {
  password_min_length: "密码最小长度",
  lock_threshold: "锁定阈值（连续失败次数）",
  lock_minutes: "锁定时长（分钟）",
  checkout_hours: "检出时长（小时）",
};

export function paramLabel(key: string): string {
  return PARAM_LABEL[key] ?? key;
}
