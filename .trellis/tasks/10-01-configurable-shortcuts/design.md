# 设计

## 已核查动作入口

| 动作 | 默认 | 执行与提示 |
| --- | --- | --- |
| paste | Enter | List.handleKeyDown、原生/Windows菜单、ShortcutList |
| pastePlain | CmdOrCtrl+Enter | List；文件维持路径粘贴现有语义；菜单 |
| copy | CmdOrCtrl+C | List（保留原生选中文字复制）；菜单 |
| open | CmdOrCtrl+O | List按 availableActions 打开链接/邮件/路径；菜单 |
| favorite | CmdOrCtrl+D | List；菜单 |
| pin | CmdOrCtrl+T | List；菜单 |
| note | CmdOrCtrl+M | List；菜单 |
| delete | CmdOrCtrl+Backspace | List删除策略与确认；菜单 |
| deleteForward | CmdOrCtrl+Delete | List当前第二个删除入口，作为独立绑定保留旧行为 |

窗口裸键首期仅允许 Enter、Backspace/Delete 或 F1—F12；Space/Esc/Tab/方向、数字及 CmdOrCtrl+F/Q/N/P/K/逗号保留。每个稳定动作 ID 一个绑定；两个删除入口共享原业务动作。

## 持久化与兼容

Shortcuts 新增 convertToPlainText 字符串、windowActions 键值表；Rust默认表供缺键补默认，空串保留禁用。未知 ID 不执行但保留；加载非法新绑定禁用并记录，更新/导入拒绝无效绑定，不覆盖无关设置。TS镜像，设置 patch保持深合并。无需DB migration。

规范化展开 CmdOrCtrl（macOS Command，Windows Control）、Ctrl/Control、Meta/Command/Super、Option/Alt，修饰顺序固定；大小写、Esc/Escape、Space统一。精确比较四个修饰键和主键，禁止重复、作用域抢占和辅助保留键。

## 注册与执行

Rust manager保留实际注册和失败列表；事件/查询可恢复状态，启动/恢复失败不声称生效。更新全局键先尝试新注册，失败恢复旧注册及旧设置；写盘失败恢复旧注册。录入期间保持计数/世代防竞态，保存需验证注册可行性。未知ID不分发。Windows钩子只吞匹配窗口配置或明确固定导航键，完整传输modifiers/repeat并配对keyup；编辑模式仍停钩子。

当前剪贴板服务复用guard的操作级撤销，独立在途保护；复核快照后仅Text写回，失败不盲目重试。跨进程复制在最后复核与写入之间仍存在不可消除窗口；底层set可能部分改变剪贴板。全局不支持/失败采用已有原生dialog反馈，Rust i18n提供中英文。

## 分工

主代理仅负责 clipboard/plain_text.rs、guard.rs、mod.rs服务导出；builder负责其它模块、接线、验证及提交。保留其它工作区改动。
