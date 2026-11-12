<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";

const auth = useAuthStore();
const router = useRouter();
const username = computed(() => auth.username || "未登录");

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
        <el-menu-item index="/docs">图纸</el-menu-item>
      </el-menu>
    </el-aside>
    <el-container>
      <el-header class="topbar">
        <span class="title">产品数据工作台</span>
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
