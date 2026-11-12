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
    auth.setSession(pair.access_token, username.value);
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
      <h2>RustPLM 登录</h2>
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
}
.login-card {
  width: 360px;
}
</style>
