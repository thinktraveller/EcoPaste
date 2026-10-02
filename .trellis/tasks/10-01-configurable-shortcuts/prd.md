# 可配置窗口快捷键与当前剪贴板纯文本转换

## 需求与范围

执行 project-docs/project-plan.md 的 R001/R002，采用 D1—D8 推荐范围。仅 macOS 与 Windows；Rust 主导系统能力及持久化。

- 当前系统剪贴板独立全局转换，默认 Control+Shift+V，可禁用和改绑；不粘贴、不改变窗口或历史使用数据。
- 原生 Text 加 HTML/RTF 转为仅 Text；纯文本及空内容不操作；图片/文件混合与 HTML/RTF-only 保留。
- 开放历史粘贴、纯文本粘贴、复制、打开、收藏、置顶、备注、删除绑定；导航、预览、Esc、Tab、数字及其它辅助动作保持默认。
- 旧 openClipboard/openPreference/winV 保留；缺字段补默认，空串禁用；未知窗口动作保留但不执行。
- 绑定语义一致、修饰键精确匹配，更新失败回滚，启动或恢复失败可查看真实注册状态。

## 验收

1. 新绑定在浏览器和 Windows 非聚焦钩子执行，旧键失效；编辑、录入及 IME 保留原生行为。
2. 设置、速查和两平台菜单展示相同绑定，禁用不显示快捷键；单项/窗口组恢复默认不影响其它设置。
3. 转换使用当前系统内容，无历史 ID、捕获过滤或粘贴副作用；重入与陈旧快照不覆盖新复制内容。
4. 无效/重复/保留键拒绝保存；旧配置保留、自定义值/禁用不被默认覆盖。
5. 运行 lint/typecheck/构建与可用 Rust 检查；真实桌面验收按平台记录。缺工具或设备只记待验收。

## 当前环境

无活动 task；初始两仓 clean。Node bundle 24.19.0；系统 pnpm 9.15.9 不识别当前 workspace 配置，bundle 11.19.0；Rust 工具链不存在，需用户安装才能编译/运行桌面。

## 2026-10-01 实施与验收状态

- 已实施：设置升级/校验、9个窗口动作、独立当前剪贴板转换、全局注册事务/status、完整Windows modifier/repeat/keyup、录入恢复、两端菜单/速查、双语反馈、导入/rebase同步和加载诊断。
- 已通过：pnpm lint（199文件）、pnpm tsc、pnpm build；实际React界面与官方Tauri mock操作36项（设置12、列表20、加载诊断4），无pageerror。平台flag不代表真实macOS设备或Windows OS hook。
- 已修复实操发现：纯modifier误匹配、editable modifier handoff失焦、Shift数字/标点归一、deleteForward提示遗漏。
- 待运行：Rust fmt/clippy/test、_verify/settings-shortcuts与_verify/clipboard-conversion故障注入；当前没有Rust工具链。
- 待真实桌面：双平台Text/HTML/RTF/混合/失败/竞态与焦点、独立于采集/历史、OS全局占用、Windows非聚焦hook与Meta/Alt系统行为、非美式键盘布局、macOS原生菜单。
- 已通过临时UI验证源按builder要求删除；结果JSON/截图及未运行Rustharness仅保留在Git忽略的_verify。所有步骤日志仅写project-docs/buildlog.md。
- 保持in_progress，不归档，不宣称完整构建验收已通过。

## 2026-10-01 23:41 Windows构建与覆盖续验

- 用户追加要求构建exe并覆盖；已使用隔离Rust 1.96.0和现有VS2022 C++/SDK构建Windows release EXE。
- 最终fmt、release all-targets/all-features clippy -D warnings通过；正式cargo test 177 passed/7 ignored；真实逻辑+OS/IO mock的独立Rust harness 8+12通过。前述“无Rust工具链”阻塞已解决。
- 最终源码提交8112283；Tauri无bundle构建成功，<安装目录>/EcoPaste.exe已覆盖并实际重启，源/安装SHA256一致，启动日志设置/SQLite/监听就绪。
- EXE及准确重启前用户数据备份、产物hash、测试日志、安装结果仅保留在Git忽略的_verify/windows-build；详情见project-docs/buildlog.md最新条目。
- 自动审批策略拒绝清理已通过Rust验证目录，未绕过，两个ignored目录仍留本机。
- 该EXE交付已完成；完整Windows功能矩阵与macOS构建/真机验收仍待执行，保持in_progress，不归档。
