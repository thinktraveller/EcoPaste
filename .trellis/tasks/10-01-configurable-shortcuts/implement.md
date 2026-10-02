# 执行顺序与验证

1. 已核查现有动作入口、范围、格式矩阵；记录步骤1并提交双仓。
2. 扩展Rust/TS设置、动作解析/验证/升级和生效状态契约；静态契约验证、可用工具检查后记录步骤2。
3. 集成Rust当前剪贴板服务、薄command与全局触发；检查无历史/粘贴/显隐副作用，编译待工具可用。
4. List与Windows hook按配置精确匹配、完整modifiers与keyup；编辑/IME与重复保护。
5. 录入scope、冲突、恢复默认、生效失败展示；速查与菜单、中英文。
6. 回归导入/rebase/reset，旧配置/未知动作/空绑定样例；不修改DB migration。
7. pnpm lint/tsc/build；Rust fmt/clippy/test在工具可用时运行；真实UI与剪贴板、macOS待设备可用。所有实际结果写buildlog；未验证不宣称项目构建完成。

每完成一步立即显式路径提交主仓与buildlog文档仓，禁止push/空提交。环境安装按builder安全要求交用户执行；可逆源码工作继续推进。
