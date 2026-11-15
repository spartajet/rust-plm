<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";

const auth = useAuthStore();
const router = useRouter();
const username = computed(() => auth.username || "未登录");
const keyword = ref("");

function doSearch() {
  const q = keyword.value.trim();
  if (!q) return;
  router.push({ path: "/search", query: { q } });
}

function logout() {
  auth.clear();
  router.push("/login");
}
</script>

<template>
  <el-container style="min-height: 100vh">
    <el-aside width="200px" style="background: #001529">
      <div class="brand">RustPLM</div>
      <el-menu router :default-active="$route.path" background-color="#001529" text-color="#fff">
        <el-menu-item index="/">工作台</el-menu-item>
        <el-menu-item index="/todos">待办中心</el-menu-item>
        <el-menu-item index="/docs">图纸</el-menu-item>
        <el-menu-item index="/parts">物料</el-menu-item>
        <el-menu-item v-if="auth.canManageSec" index="/admin/users">用户管理</el-menu-item>
        <el-menu-item v-if="auth.canManageSec" index="/admin/audit">审计日志</el-menu-item>
      </el-menu>
    </el-aside>
    <el-container>
      <el-header class="topbar">
        <span class="title">产品数据工作台</span>
        <el-input
          v-model="keyword"
          placeholder="全局搜索"
          style="width: 240px"
          data-test="global-search"
          clearable
          @keyup.enter="doSearch"
        />
        <span class="user">
          {{ username }}
          <el-button link type="danger" @click="logout">登出</el-button>
        </span>
      </el-header>
      <el-main>
        <router-view />
      </el-main>
    </el-container>
  </el-container>
</template>

<style scoped>
.brand {
  color: #fff;
  font-size: 18px;
  font-weight: 700;
  padding: 20px 16px;
}
.topbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: #fff;
  border-bottom: 1px solid #e4e7ed;
}
.title {
  font-weight: 600;
}
.user {
  color: #606266;
}
</style>
