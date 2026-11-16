<script setup lang="ts">
// 主布局（增量29 系统化重构）：品牌区/图标导航/顶栏（全局搜索+主题切换+用户区）/内容区。
// Industrial Slate 设计系统：token 驱动、亮暗双态、既有一切 data-test/路由零破坏。
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import {
  Monitor,
  Moon,
  Odometer,
  Setting,
  Tickets,
  Sunny,
  User,
  Box,
} from "@element-plus/icons-vue";
import { useAuthStore } from "../stores/auth";
import { toggleTheme } from "../composables/useTheme";

const auth = useAuthStore();
const router = useRouter();
const username = computed(() => auth.username || "未登录");
const keyword = ref("");
const isDark = ref(document.documentElement.classList.contains("dark"));

const NAV = [
  { path: "/", label: "工作台", icon: Odometer },
  { path: "/todos", label: "待办中心", icon: Tickets },
  { path: "/docs", label: "图纸", icon: Monitor },
  { path: "/parts", label: "物料", icon: Box },
  { path: "/admin/users", label: "用户管理", icon: User, adminOnly: true },
  { path: "/admin/audit", label: "审计日志", icon: Setting, adminOnly: true },
];

function doSearch() {
  const q = keyword.value.trim();
  if (!q) return;
  router.push({ path: "/search", query: { q } });
}

function onToggleTheme() {
  isDark.value = toggleTheme() === "dark";
}

function logout() {
  auth.clear();
  router.push("/login");
}
</script>

<template>
  <el-container style="min-height: 100vh">
    <el-aside width="220px" class="sidebar">
      <div class="brand">
        <div class="brand-mark">R</div>
        <div>
          <div class="brand-name">RustPLM</div>
          <div class="brand-sub">产品数据管理</div>
        </div>
      </div>
      <el-menu router :default-active="$route.path" class="side-menu">
        <template v-for="n in NAV" :key="n.path">
          <el-menu-item
            v-if="!n.adminOnly || auth.canManageSec"
            :index="n.path"
          >
            <el-icon><component :is="n.icon" /></el-icon>
            <span>{{ n.label }}</span>
          </el-menu-item>
        </template>
      </el-menu>
      <div class="sidebar-foot">
        <el-button
          link
          class="theme-btn"
          :data-test="isDark ? 'theme-light' : 'theme-dark'"
          @click="onToggleTheme"
        >
          <el-icon><component :is="isDark ? Sunny : Moon" /></el-icon>
          <span>{{ isDark ? "切至亮色" : "切至暗色" }}</span>
        </el-button>
      </div>
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
          <span class="avatar">{{ username.slice(0, 1).toUpperCase() }}</span>
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
.sidebar {
  background: var(--rp-sidebar-bg);
  border-right: 1px solid var(--rp-border);
  display: flex;
  flex-direction: column;
}
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 18px 16px;
}
.brand-mark {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  background: linear-gradient(135deg, var(--rp-primary), #7c3aed);
  color: #fff;
  font-weight: 700;
  font-size: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.brand-name {
  font-weight: 700;
  font-size: 16px;
  color: var(--rp-text);
}
.brand-sub {
  font-size: 12px;
  color: var(--rp-text-3);
}
.side-menu {
  border-right: none;
  background: transparent;
  flex: 1;
}
.side-menu :deep(.el-menu-item) {
  color: var(--rp-sidebar-text);
  border-radius: 8px;
  margin: 2px 10px;
  height: 42px;
}
.side-menu :deep(.el-menu-item:hover) {
  background: var(--el-fill-color-light);
}
.side-menu :deep(.el-menu-item.is-active) {
  background: var(--rp-sidebar-active-bg);
  color: var(--rp-sidebar-active-text);
  font-weight: 600;
}
.sidebar-foot {
  padding: 12px 16px;
  border-top: 1px solid var(--rp-border);
}
.theme-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--rp-text-2);
}
.topbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: var(--rp-header-bg);
  border-bottom: 1px solid var(--rp-border);
}
.title {
  font-weight: 600;
  color: var(--rp-text);
}
.user {
  color: var(--rp-text-2);
  display: flex;
  align-items: center;
  gap: 8px;
}
.avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: var(--rp-primary-light);
  color: var(--rp-primary);
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
</style>
