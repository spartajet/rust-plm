import type { AxiosInstance } from "axios";

/// 用户列表行（增量11：GET /api/v1/sec/users，sec:manage）
export interface SecUser {
  username: string;
  max_secret_level: number;
  created_at?: string | null;
  roles: string[];
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
): Promise<UsersPage> {
  const q = new URLSearchParams();
  if (search?.trim()) q.set("search", search.trim());
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
