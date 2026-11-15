<script setup lang="ts">
import { ElMessage } from "element-plus";
import { onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { searchGlobal, groupLabel, type SearchGroup } from "../api/search";
import { stateLabel, stateTagType } from "../utils/flowUi";

const route = useRoute();
const router = useRouter();

const q = ref((route.query.q as string) ?? "");
const input = ref(q.value);
const groups = ref<SearchGroup[]>([]);
const total = ref(0);
const loading = ref(false);
const searched = ref(false);

async function load() {
  const query = q.value.trim();
  if (!query) return;
  loading.value = true;
  try {
    const res = await searchGlobal(http, query);
    groups.value = res.groups;
    total.value = res.total;
    searched.value = true;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

onMounted(load);
watch(
  () => route.query.q,
  (nq) => {
    if (typeof nq === "string" && nq && nq !== q.value) {
      q.value = nq;
      input.value = nq;
      load();
    }
  },
);

/// 顶栏/页内搜索框回车 → 写 URL（守卫 watch 触发加载；同值直接搜）
function submit() {
  const v = input.value.trim();
  if (!v) return;
  if (v === q.value) {
    load();
  } else {
    router.push({ path: "/search", query: { q: v } });
  }
}

function openDetail(group: SearchGroup, objNo: string) {
  if (group.type_code === "drawing") router.push(`/docs/${objNo}`);
}
</script>

<template>
  <div class="page">
    <div class="toolbar">
      <el-input
        v-model="input"
        placeholder="全局搜索：图号 / 名称 / 材料"
        style="width: 360px"
        data-test="search-input"
        clearable
        @keyup.enter="submit"
      />
      <el-button type="primary" data-test="search-go" @click="submit">搜索</el-button>
    </div>

    <el-empty v-if="searched && total === 0 && !loading" description="无匹配结果" />

    <section v-for="g in groups" :key="g.type_code" class="cluster">
      <h3>
        {{ groupLabel(g.type_code) }}
        <span class="count">共 {{ g.total }} 条</span>
      </h3>
      <el-table v-loading="loading" :data="g.items" :data-test="`search-table-${g.type_code}`">
        <el-table-column prop="obj_no" label="编号" min-width="150" />
        <el-table-column label="名称" min-width="150">
          <template #default="{ row }">{{ row.name ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="版本" width="80">
          <template #default="{ row }">v{{ row.major_ver }}.{{ row.minor_ver }}</template>
        </el-table-column>
        <el-table-column label="密级" width="80">
          <template #default="{ row }">
            <el-tag
              :type="row.secret_level >= 3 ? 'danger' : row.secret_level === 2 ? 'warning' : 'info'"
              effect="plain"
              size="small"
            >
              {{ row.secret_level }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="stateTagType(row.state)">{{ stateLabel(row.state) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column v-if="g.type_code === 'drawing'" label="" width="80">
          <template #default="{ row }">
            <el-button link type="primary" @click="openDetail(g, row.obj_no)">查看</el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>
  </div>
</template>

<style scoped>
.cluster {
  margin-top: 16px;
}
.cluster h3 {
  margin: 8px 0;
  font-size: 15px;
}
.count {
  color: #909399;
  font-weight: 400;
  font-size: 13px;
  margin-left: 8px;
}
</style>
