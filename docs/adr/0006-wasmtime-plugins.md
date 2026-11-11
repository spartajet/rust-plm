# ADR-0006 插件运行时：wasmtime 组件模型 + 能力级授权

- 状态：已接受（2026-11-11）
- 关联需求：PLG-01~07（插件化平台，V1.5）

## 背景

第三方扩展备选：①动态库（.so/.dll）——无内存安全边界、崩溃即拖垮主进程、跨平台分发难；②独立进程+RPC——部署重、延迟高；③WASM——内存沙箱、跨平台单文件分发、宿主可注入受控 API（wasmtime 是 Rust 生态最成熟运行时，Component Model 支持结构化接口）。

## 决策

后端插件统一走 **wasmtime Component Model**：插件以 wit（WASM Interface Types）声明宿主接口（`plm.db.read / plm.objects / plm.events / plm.http / plm.logger`），安装时按 manifest 授权能力子集；配额（CPU fuels/内存 limiter/超时）+ trap 隔离（插件崩溃仅自身停用）。**连接器插件**因需原生 SDK/高性能 IO，允许 Rust 动态库形态（abi_stable ABI + 同样的生命周期管理），作为受信度更高的第二类运行时。热加载 = 新版本实例化于独立 PluginSlot → arc-swap 原子切换 → 在途调用排空。

## 后果

- ✅ 安全边界清晰（PLG-02）；Rust/AS/Go 均可编译目标（PLG-01 SDK 多语言）；
- ✅ 官方连接器（IM/FA/ERP）先以动态库形态交付并 dogfood 生命周期管理，倒逼 SDK 成熟；
- ⚠️ wit 接口演进需版本协商——SDK 语义化版本 + wit 世界版本号双轨；
- ⚠️ wasmtime 预编译开销——安装时 AOT 编译缓存，运行期零编译。
