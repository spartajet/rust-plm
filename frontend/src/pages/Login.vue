<script setup lang="ts">
import { ElMessage } from "element-plus";
import { ref } from "vue";
import { useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { login as apiLogin } from "../api/auth";
import { useAuthStore } from "../stores/auth";

const router = useRouter();
const auth = useAuthStore();

const username = ref("");
const password = ref("");
const loading = ref(false);

async function submit() {
  if (!username.value || !password.value) {
    ElMessage.warning("请输入用户名与密码");
    return;
  }
  loading.value = true;
  try {
    const pair = await apiLogin(http, username.value, password.value);
    auth.setSession(pair.access_token, username.value, pair.refresh_token, pair.roles);
    router.push("/");
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="login-page">
    <el-card class="login-card">
      <div class="login-brand">
        <div class="brand-mark">R</div>
        <div>
          <h2 class="login-title">RustPLM</h2>
          <div class="login-sub">产品数据管理 · 统一登录</div>
        </div>
      </div>
      <el-form label-position="top" @submit.prevent="submit">
        <el-form-item label="用户名">
          <el-input v-model="username" data-test="username" placeholder="用户名" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input
            v-model="password"
            data-test="password"
            type="password"
            show-password
            placeholder="密码"
          />
        </el-form-item>
        <el-button
          type="primary"
          data-test="login-btn"
          :loading="loading"
          style="width: 100%"
          @click="submit"
        >
          登录
        </el-button>
      </el-form>
    </el-card>
  </div>
</template>

<style scoped>
.login-page {
  display: flex;
  justify-content: center;
  align-items: center;
  min-height: 100vh;
  background:
    radial-gradient(ellipse 60% 50% at 20% 10%, rgba(79, 70, 229, 0.12), transparent),
    radial-gradient(ellipse 50% 40% at 85% 90%, rgba(234, 88, 12, 0.08), transparent),
    var(--rp-bg);
}
.login-card {
  width: 380px;
  padding: 8px 12px;
  box-shadow: var(--rp-shadow-md);
}
.login-brand {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 20px;
}
.brand-mark {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  background: linear-gradient(135deg, var(--rp-primary), #7c3aed);
  color: #fff;
  font-weight: 700;
  font-size: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.login-title {
  margin: 0;
  color: var(--rp-text);
}
.login-sub {
  font-size: 12px;
  color: var(--rp-text-3);
}
</style>
