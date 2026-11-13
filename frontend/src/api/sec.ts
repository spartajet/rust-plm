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

export async function listUsers(http: AxiosInstance, search?: string): Promise<SecUser[]> {
  const q = search?.trim() ? `?search=${encodeURIComponent(search.trim())}` : "";
  const res = await http.get<SecUser[]>(`/api/v1/sec/users${q}`);
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
