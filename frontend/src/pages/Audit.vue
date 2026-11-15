<script setup lang="ts">
// 审计日志页（增量18 SYS-01）：多维筛选 + 分页（sec:manage 门控——无权限者 API 403 提示）
import { ElMessage } from "element-plus";
import { onMounted, ref } from "vue";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { searchAudit, type AuditRow } from "../api/audit";

const items = ref<AuditRow[]>([]);
const total = ref(0);
const page = ref(1);
const size = ref(50);
const loading = ref(false);

const fUsername = ref("");
const fAction = ref("");
const fObject = ref("");

async function load() {
  loading.value = true;
  try {
    const res = await searchAudit(http, {
      username: fUsername.value,
      action: fAction.value,
      object: fObject.value,
      page: page.value,
      size: size.value,
    });
    items.value = res.items;
    total.value = res.total;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

function doSearch() {
  page.value = 1;
  load();
}

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="toolbar">
      <el-input
        v-model="fUsername"
        placeholder="按用户"
        clearable
        style="width: 150px"
        data-test="audit-user"
        @change="doSearch"
      />
      <el-select
        v-model="fAction"
        placeholder="按动作"
        clearable
        style="width: 170px"
        data-test="audit-action"
        @change="doSearch"
      >
        <el-option label="HTTP 请求" value="http" />
        <el-option label="登录成功" value="login.ok" />
        <el-option label="登录失败" value="login.fail" />
        <el-option label="档位变更" value="flow-tier.change" />
      </el-select>
      <el-input
        v-model="fObject"
        placeholder="按对象"
        clearable
        style="width: 150px"
        data-test="audit-object"
        @change="doSearch"
      />
      <el-button data-test="audit-search" @click="doSearch">查询</el-button>
    </div>

    <el-table v-loading="loading" :data="items" data-test="audit-table">
      <el-table-column label="时间" min-width="170">
        <template #default="{ row }">{{ row.at }}</template>
      </el-table-column>
      <el-table-column prop="username" label="用户" width="110" />
      <el-table-column prop="action" label="动作" width="150" />
      <el-table-column label="对象" min-width="140">
        <template #default="{ row }">{{ row.object ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="详情" min-width="220">
        <template #default="{ row }">
          <code v-if="row.detail" style="font-size: 12px">{{ JSON.stringify(row.detail) }}</code>
          <span v-else>-</span>
        </template>
      </el-table-column>
      <el-table-column label="IP" width="120">
        <template #default="{ row }">{{ row.ip ?? "-" }}</template>
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
  </div>
</template>
