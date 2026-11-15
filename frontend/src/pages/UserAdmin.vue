<script setup lang="ts">
// 用户管理（增量11）：建号 / 授权 / 回收（后端 sec:manage 门控；无权限者 403 提示）
import { ElMessage } from "element-plus";
import { onMounted, onUnmounted, ref, watch } from "vue";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import {
  createUser,
  grantRole,
  listParams,
  listRoles,
  listUsers,
  revokeRole,
  setParam,
  paramLabel,
  type SecRole,
  type SecUser,
  type SysParam,
} from "../api/sec";

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

// ==================== SYS-04 安全参数面板（增量23；canManageSec 同权限门） ====================
const params = ref<SysParam[]>([]);
const paramDraft = ref<Record<string, number>>({});
const paramSaving = ref<Record<string, boolean>>({});

async function loadParams() {
  try {
    params.value = await listParams(http);
    paramDraft.value = Object.fromEntries(params.value.map((p) => [p.key, Number(p.value)]));
  } catch (e) {
    // 无权限等场景静默（面板不渲染数据即可）
    void e;
  }
}

async function saveParam(key: string) {
  const v = paramDraft.value[key];
  if (!Number.isInteger(v) || v <= 0) {
    ElMessage.warning("须为正整数");
    return;
  }
  paramSaving.value[key] = true;
  try {
    await setParam(http, key, v);
    ElMessage.success("已保存");
    await loadParams();
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    paramSaving.value[key] = false;
  }
}

onMounted(() => {
  load();
  loadParams();
});

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

    <!-- SYS-04 安全参数（增量23；后端 sec:manage 门控） -->
    <el-card style="margin-top: 16px" data-test="params-card">
      <template #header>
        <span>安全参数（密码策略 / 登录锁定）</span>
      </template>
      <el-table :data="params" size="small" data-test="params-table">
        <el-table-column label="参数" min-width="200">
          <template #default="{ row }">{{ paramLabel(row.key) }}</template>
        </el-table-column>
        <el-table-column label="键" min-width="160" prop="key" />
        <el-table-column label="值" width="180">
          <template #default="{ row }">
            <el-input-number
              v-model="paramDraft[row.key]"
              :min="1"
              size="small"
              :data-test="`param-input-${row.key}`"
            />
          </template>
        </el-table-column>
        <el-table-column label="修改人" width="110">
          <template #default="{ row }">{{ row.updated_by ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="" width="90">
          <template #default="{ row }">
            <el-button
              link
              type="primary"
              size="small"
              :data-test="`param-save-${row.key}`"
              :loading="!!paramSaving[row.key]"
              @click="saveParam(row.key)"
            >
              保存
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

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
