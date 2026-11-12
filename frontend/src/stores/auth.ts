import { defineStore } from "pinia";

const TOKEN_KEY = "rustplm_token";
const USER_KEY = "rustplm_user";

export const useAuthStore = defineStore("auth", {
  state: () => ({
    token: localStorage.getItem(TOKEN_KEY) ?? "",
    username: localStorage.getItem(USER_KEY) ?? "",
  }),
  getters: {
    isAuthenticated: (s) => s.token.length > 0,
  },
  actions: {
    setSession(token: string, username: string) {
      this.token = token;
      this.username = username;
      localStorage.setItem(TOKEN_KEY, token);
      localStorage.setItem(USER_KEY, username);
    },
    clear() {
      this.token = "";
      this.username = "";
      localStorage.removeItem(TOKEN_KEY);
      localStorage.removeItem(USER_KEY);
    },
  },
});
