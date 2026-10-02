# 实施和验证

8 窗口固定状态查询/事件/按钮、Windows固定导航外部交互保护；tsc/lint/Rust编译、桌面点击。
9 Rust结构解析及快照选择，解析和故障/Guard用例；真实系统格式输出。
10 独立无焦点成功浮层，两入口共享；桌面全局转换、重复/隐藏/非成功检查。
11 完整fmt/clippy/test/lint/tsc/build，Windows主路径、剩余双平台矩阵如实标注。

每步立即更新buildlog并显式路径提交主仓和文档仓，不推送。project-plan只读。验证脚本只在_verify。

## 2026-10-02 实施进度

步骤8固定事实状态/事件、Windows外部输入保护；步骤9结构Markdown/HTML解析；
步骤10共享成功入口和无焦点反馈已实现并逐步双仓提交。步骤11进行中：本轮
React/Antd+官方mock UI16项、解析/故障Guard15项、真实Windows剪贴板13样例通过；
产品identifier与隔离测试identifier的release EXE均构建成功。正式Rust177 passed/7
ignored，lint180文件与tsc通过。真实Windows窗口/焦点矩阵由主代理通过sky操作，
macOS暂无设备仍待验收；不借旧task历史结果证明本轮通过。

审查修正pending/displayed hint ID分离，以及Windows编辑结束先恢复foreground再
重启钩子采样，避免固定态拒绝列表导航或恢复外部吞键。

用户于本轮末要求后续测试由本人进行，已停止桌面自动化和追加测试。
Windows已实测固定切换/失焦保持/重启重置、搜索结束后的列表Down/Ctrl+C及外部
编辑器左右键。默认Ctrl+Shift+V在系统注册active，但自动发送未取得转换成功；
成功浮层真实显示/焦点/计时、剩余Windows矩阵及macOS仍交用户验收。F9隔离诊断
只准备未执行，临时绑定已恢复默认，测试实例已停并恢复原安装实例。
详见project-docs/docs/clipboard-format-desktop-verification.md；任务保留in_progress，
不将已完成自动检查等同于完整双平台验收。

## 2026-10-02 转换快捷键故障修复（待用户验收）

用户真机反馈转换快捷键无转换、无提示。静态定位发现共享转换入口请求
`WritebackGuard`，启动实际注册的是`Arc<WritebackGuard>`；Tauri按精确类型查询，
导致在读取剪贴板前panic，快捷键后台任务未取得转换结果及成功反馈。
入口已改为获取原有`Arc<WritebackGuard>`并借用内部Guard，与历史写回一致。
本轮按用户要求不执行测试、构建或桌面自动化；需用户重新构建、退出旧进程并
启动新程序，验证隐藏主窗口的Markdown转换、成功浮层与连续转换、普通文本
不误报，以及历史记录复制。任务仍保持in_progress，不将修复静态审查算作验收。

## 2026-10-02 成功浮层与列表标记修复（待用户验收）

用户在新EXE确认转换后反馈浮层不消失及列表残留“- ”，最新要求浮层1秒后隐藏。
Windows原生show没有更新Tao可见性缓存，Tauri hide的false→false差量为空，未真正
隐藏HWND；已改主线程原生ShowWindow(SW_HIDE)，保留实际显示代次保护，时长改1秒。
列表renderer原先主动重新生成“- ”及编号，现仅保留列表正文、换行和层级缩进；
代码和正文内减号不删除，转义Markdown按字面文本保留，不做全局符号清洗。
本代理未运行测试、构建或桌面自动化；主代理接管构建交付，实际1秒显示/隐藏、
连续成功计时和用户列表样例结果仍由用户验收，不归档当前任务。
