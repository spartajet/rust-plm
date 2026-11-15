<script setup lang="ts">
// 工作台看板（增量11）：按状态计数（复用 list total，ABAC 密级内）；点击卡片联动列表筛选
import { ElMessage } from "element-plus";
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { http } from "../api/index";
import { listDocs } from "../api/docs";
import { stateLabel, stateTagType } from "../utils/flowUi";
import { extractApiError } from "../api/http";
import {
  getChecklist,
  completeStep,
  getStarter,
  importTemplate,
  stepRoute,
  type ChecklistStep,
  type ImportResult,
} from "../api/meta";
import { useAuthStore } from "../stores/auth";

const router = useRouter();
const auth = useAuthStore();

// ==================== AGL-09 实施向导 + AGL-05 模板导入（增量24） ====================
const steps = ref<ChecklistStep[]>([]);
const wizardOpen = ref(false);
const importing = ref(false);
const importReport = ref<ImportResult | null>(null);
const importingStarter = ref(false);

async function loadWizard() {
  try {
    steps.value = await getChecklist(http);
  } catch {
    steps.value = []; // 非关键卡：静默
  }
}

const wizardDone = () => steps.value.length > 0 && steps.value.every((s) => s.done);

function toggleWizard() {
  wizardOpen.value = !wizardOpen.value;
  if (wizardOpen.value && steps.value.length === 0) loadWizard();
}

function gotoStep(key: string) {
  router.push(stepRoute(key));
}

async function markDone(key: string) {
  try {
    await completeStep(http, key);
    await loadWizard();
  } catch (e) {
    // 终审 I1-2：失败必须可见（400 按序约束/403 无权限——原静默吞为死兜底）
    ElMessage.error(extractApiError(e));
  }
}

/// 一键导入 Starter 包：dry_run 预检展示冲突 → 恒 merge（服务端冲突项自动跳过，安全；
/// 终审 M1：原“有可应用项才 merge”条件死代码——dry 响应 applied 恒空，退化为零冲突才 merge）
async function importStarter() {
  importingStarter.value = true;
  try {
    const pkg = await getStarter(http);
    await importTemplate(http, pkg, "dry_run");
    const merged = await importTemplate(http, pkg, "merge");
    importReport.value = merged;
    await loadWizard(); // coding/types 步可能被模板翻转
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    importingStarter.value = false;
  }
}

interface Card {
  state: string;
  count: number | null;
}
const cards = ref<Card[]>([
  { state: "working", count: null },
  { state: "review", count: null },
  { state: "checked", count: null },
  { state: "released", count: null },
  { state: "obsolete", count: null },
]);

async function load() {
  // 5 个状态各取 total（size=1 最小化传输；计数在 ABAC 密级内=用户可见视角）
  await Promise.all(
    cards.value.map(async (c) => {
      try {
        const res = await listDocs(http, { state: c.state, page: 1, size: 1 });
        c.count = res.total;
      } catch {
        c.count = null; // 看板非关键路径：静默兜底不弹全局 toast
      }
    }),
  );
}

onMounted(() => {
  load();
  loadWizard();
});

function open(state: string) {
  router.push({ path: "/docs", query: { state } });
}
</script>

<template>
  <div class="page">
    <!-- AGL-09 实施向导卡（增量24；全员可见，manual 完成按钮 sec:manage 后端门控） -->
    <el-card style="margin-bottom: 16px" data-test="wizard-card">
      <div style="display: flex; align-items: center; gap: 12px; cursor: pointer" @click="toggleWizard">
        <b>7 天上线向导</b>
        <el-tag
          :type="wizardDone() ? 'success' : 'warning'"
          size="small"
          data-test="wizard-status"
        >
          {{ wizardDone() ? "已完成" : `${steps.filter((x) => x.done).length}/${steps.length || 8} 步` }}
        </el-tag>
        <span style="color: #909399; font-size: 12px">
          {{ wizardOpen ? "收起" : "展开实施清单（组织→权限→编码→类型→流程→数据→培训→切换）" }}
        </span>
        <span style="flex: 1"></span>
        <el-button
          v-if="auth.canManageSec"
          size="small"
          type="primary"
          plain
          data-test="import-starter"
          :loading="importingStarter"
          @click.stop="importStarter"
        >
          导入 Starter 配置包
        </el-button>
      </div>
      <div v-if="wizardOpen" style="margin-top: 12px" data-test="wizard-steps">
        <div v-for="st in steps" :key="st.key" class="wiz-step">
          <el-tag :type="st.done ? 'success' : 'info'" size="small" data-test="wizard-step-done">
            {{ st.done ? "已完成" : "待完成" }}
          </el-tag>
          <b style="margin: 0 8px">{{ st.title }}</b>
          <span style="color: #909399">{{ st.hint }}</span>
          <span style="flex: 1"></span>
          <el-button v-if="!st.done && st.auto" link type="primary" size="small" @click="gotoStep(st.key)">
            去操作
          </el-button>
          <el-button
            v-if="!st.done && !st.auto && auth.canManageSec"
            link
            type="success"
            size="small"
            :data-test="`wizard-complete-${st.key}`"
            @click="markDone(st.key)"
          >
            标记完成
          </el-button>
        </div>
        <div v-if="importReport" style="margin-top: 8px; color: #67c23a; font-size: 12px" data-test="import-report">
          模板导入：应用类型 {{ importReport.applied_types.length }} 个、分类 {{ importReport.applied_classes.length }} 个、冲突跳过 {{ importReport.conflicts.length }} 项
        </div>
      </div>
    </el-card>

    <el-row :gutter="16">
      <el-col v-for="c in cards" :key="c.state" :span="4">
        <el-card
          shadow="hover"
          class="board-card"
          :data-test="`board-${c.state}`"
          @click="open(c.state)"
        >
          <div class="count" :data-test="`board-count-${c.state}`">{{ c.count ?? "-" }}</div>
          <el-tag :type="stateTagType(c.state)">{{ stateLabel(c.state) }}</el-tag>
        </el-card>
      </el-col>
    </el-row>
    <el-row :gutter="16" style="margin-top: 16px">
      <el-col :span="24">
        <el-card>点击卡片进入对应状态列表；数据含你密级权限内的全部对象。</el-card>
      </el-col>
    </el-row>
  </div>
</template>

<style scoped>
.wiz-step {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
}
</style>

<style scoped>
.board-card {
  text-align: center;
  cursor: pointer;
}
.count {
  font-size: 28px;
  font-weight: 700;
  margin-bottom: 8px;
}
</style>
