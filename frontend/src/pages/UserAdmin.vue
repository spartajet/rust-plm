<script setup lang="ts">
// 用户管理（增量11）：建号 / 授权 / 回收（后端 sec:manage 门控；无权限者 403 提示）
import { ElMessage } from "element-plus";
import { onMounted, onUnmounted, ref, watch } from "vue";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { createUser, grantRole, listRoles, listUsers, revokeRole, type SecRole, type SecUser } from "../api/sec";

const users = ref<SecUser[]>([]);
const roles = ref<SecRole[]>([]);
const total = ref(0);
const page = ref(1);
const size = ref(50);
const loading = ref(false);
let searchTimer: ReturnType<typeof setTimeout> | null = null;
const search = ref("");
// 搜索防抖 300ms（增量13）；unmount 清理（增量15）
watch(search, () => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    page.value = 1;
    load();
  }, 300);
});
onUnmounted(() => {
  if (searchTimer) clearTimeout(searchTimer);
});

const dlgVisible = ref(false);
const uName = ref("");
const uPass = ref("");
const uLevel = ref(0);
const uRoles = ref<string[]>([]);
const submitting = ref(false);

async function load() {
  loading.value = true;
  try {
    const res = await listUsers(http, search.value, page.value, size.value);
    users.value = res.items;
    total.value = res.total;
    roles.value = await listRoles(http);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

onMounted(load);

async function submit() {
  if (!uName.value || !uPass.value) {
    ElMessage.warning("需填写用户名与密码");
    return;
  }
  submitting.value = true;
  try {
    await createUser(http, {
      username: uName.value,
      password: uPass.value,
      max_secret_level: uLevel.value,
      roles: uRoles.value,
    });
    ElMessage.success("创建成功");
    dlgVisible.value = false;
    uName.value = "";
    uPass.value = "";
    uLevel.value = 0;
    uRoles.value = [];
    await load();
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    submitting.value = false;
  }
}

async function toggleRole(user: SecUser, role: string, has: boolean) {
  try {
    if (has) {
      await revokeRole(http, user.username, role);
    } else {
      await grantRole(http, user.username, role);
    }
    await load();
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}
</script>

<template>
  <div class="page">
    <div class="toolbar">
      <el-input
        v-model="search"
        placeholder="按用户名搜索"
        clearable
        style="width: 200px"
        data-test="user-search"
      />
      <el-button type="primary" data-test="open-create" @click="dlgVisible = true">新建用户</el-button>
      <el-button data-test="reload-users" @click="load">刷新</el-button>
    </div>

    <el-table v-loading="loading" :data="users" data-test="user-table">
      <el-table-column prop="username" label="用户名" min-width="140" />
      <el-table-column label="密级上限" width="100">
        <template #default="{ row }">{{ row.max_secret_level }}</template>
      </el-table-column>
      <el-table-column label="角色" min-width="260">
        <template #default="{ row }">
          <el-tag
            v-for="r in roles"
            :key="r.code"
            style="margin-right: 8px"
            :type="(row.roles ?? []).includes(r.code) ? 'success' : 'info'"
            :effect="(row.roles ?? []).includes(r.code) ? 'dark' : 'plain'"
            :data-test="`role-${row.username}-${r.code}`"
            @click="toggleRole(row, r.code, (row.roles ?? []).includes(r.code))"
          >
            {{ r.name }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="创建时间" min-width="160">
        <template #default="{ row }">{{ row.created_at ?? "-" }}</template>
      </el-table-column>
    </el-table>

    <el-pagination
      v-model:current-page="page"
      v-model:page-size="size"
      layout="total, prev, pager, next"
      :total="total"
      style="margin-top: 12px"
      @current-change="load"
    />

    <el-dialog v-model="dlgVisible" title="新建用户" width="440px">
      <el-form label-width="90px">
        <el-form-item label="用户名">
          <el-input v-model="uName" data-test="new-username" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="uPass" type="password" data-test="new-password" show-password />
        </el-form-item>
        <el-form-item label="密级上限">
          <el-input-number v-model="uLevel" :min="0" :max="3" data-test="new-level" />
        </el-form-item>
        <el-form-item label="角色">
          <el-select v-model="uRoles" multiple data-test="new-roles" style="width: 100%">
            <el-option v-for="r in roles" :key="r.code" :label="r.name" :value="r.code" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dlgVisible = false">取消</el-button>
        <el-button type="primary" data-test="create-submit" :loading="submitting" @click="submit">
          创建
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>
