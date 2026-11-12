import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "./App.vue";
import { setUnauthorizedHandler } from "./api/http";
import { router } from "./router";
import { useAuthStore } from "./stores/auth";
import "./styles/main.css";

const app = createApp(App);
app.use(createPinia());
app.use(router);

const auth = useAuthStore();
setUnauthorizedHandler(() => {
  auth.clear();
  router.push("/login");
});

app.mount("#app");
