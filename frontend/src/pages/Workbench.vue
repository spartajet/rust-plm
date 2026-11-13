<script setup lang="ts">
// 工作台看板（增量11）：按状态计数（复用 list total，ABAC 密级内）；点击卡片联动列表筛选
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { http } from "../api/index";
import { listDocs } from "../api/docs";
import { stateLabel, stateTagType } from "../utils/flowUi";

const router = useRouter();

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

onMounted(load);

function open(state: string) {
  router.push({ path: "/docs", query: { state } });
}
</script>

<template>
  <div class="page">
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
