# 设计

窗口固定沿用现有OS顶层和失焦保护，不改变未固定层级；进程内Rust状态增加revision及查询/事件，前端只展示。显式Esc/全局toggle/关闭隐藏窗口仍保留固定状态，再次显示仍固定，重启重置。文件选择临时暂停独立。

转换优先解析原生Text中的明确Markdown/HTML，原生Text本来可读时保留（避免把代码里的标记再次解析）；Text缺失采用HTML正文。CommonMark加表格、删除线、任务列表；链接保留标题、图片保留alt，表格列tab分隔，列表保留层级缩进与正文但不生成项目符号或编号，代码正文原样；不扩展RTF-only。解析在第二快照和代次复核前完成；空输出保守不写，hash使用最终trim输出。

成功浮层独立Rust管理的非聚焦窗口，复用单窗、位于当前屏幕下方、1秒后隐藏，连续成功刷新单条不堆叠。两入口统一成功服务触发，Rust i18n文本；ready后再显示保障初次WebView渲染，不抢主窗或应用焦点。macOS使用non-key NSPanel并配对show/hide；Windows noactivate原生显示后使用ShowWindow(SW_HIDE)隐藏，不依赖Tao尚未更新的可见性缓存。
