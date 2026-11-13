import type { AxiosInstance } from "axios";

export interface TokenPair {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  roles: string[];
}

export async function login(http: AxiosInstance, username: string, password: string): Promise<TokenPair> {
  const res = await http.post<TokenPair>("/api/v1/auth/login", { username, password });
  return res.data;
}

export async function refresh(http: AxiosInstance, refreshToken: string): Promise<TokenPair> {
  const res = await http.post<TokenPair>("/api/v1/auth/refresh", { refresh_token: refreshToken });
  return res.data;
}
