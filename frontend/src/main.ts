import { createPinia } from "pinia";
import { createApp } from "vue";
import ElementPlus from "element-plus";
import zhCn from "element-plus/es/locale/lang/zh-cn";
import "dayjs/locale/zh-cn";
import "element-plus/dist/index.css";
// 暗色基座（html.dark 驱动）+ Industrial Slate 设计 tokens + 组件统一化（增量29）
import "element-plus/theme-chalk/dark/css-vars.css";
import "./styles/tokens.css";
import "./styles/components.css";
import App from "./App.vue";
import { initTheme } from "./composables/useTheme";
import { setUnauthorizedHandler } from "./api/http";
import { router } from "./router";
import { useAuthStore } from "./stores/auth";
import "./styles/main.css";

const app = createApp(App);
app.use(createPinia());
app.use(ElementPlus, { locale: zhCn });
app.use(router);

initTheme();

const auth = useAuthStore();
setUnauthorizedHandler(() => {
  auth.clear();
  router.push("/login");
});

app.mount("#app");
