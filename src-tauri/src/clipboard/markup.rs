//! 结构化去除常见 Markdown / HTML 标记；不执行脚本或访问外部资源。

use std::sync::LazyLock;

use dom_query::{Document, NodeRef};
use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag};
use regex::Regex;

use crate::settings::{
    ImageStyle, LinkStyle, ListStyle, PlainTextConversionSettings, TableSeparator, TaskStyle,
};

/// 仅用标签语法识别明确 HTML；正文提取由 HTML5 DOM 完成。
/// 泛型 `<T>`、比较式、邮箱和不完整尖括号不会被当作 HTML。
static HTML_MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</?(?:html|body|head|title|p|div|span|b|strong|i|em|u|s|del|a|img|br|hr|h[1-6]|ul|ol|li|blockquote|pre|code|table|thead|tbody|tfoot|tr|td|th|section|article|main|header|footer|script|style|template|noscript|iframe|object|embed|dl|dt|dd)(?:\s[^<>]*|\s*/?)>")
        .expect("valid HTML identification pattern")
});

/// 普通文本返回 None，调用者保留其原始空白和换行。
/// CommonMark 加表格、删除线和任务列表；只有确实识别到标记才应用解析输出。
pub(super) fn convert_markup(input: &str, config: &PlainTextConversionSettings) -> Option<String> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<_> = Parser::new_ext(input, options).collect();
    let marked = events.iter().any(|event| match event {
        Event::Start(
            Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::List(_)
            | Tag::Table(_)
            | Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. },
        )
        | Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
        | Event::Code(_)
        | Event::Rule
        | Event::TaskListMarker(_) => true,
        Event::Html(value) | Event::InlineHtml(value) => HTML_MARKUP.is_match(value),
        _ => false,
    });
    if !marked {
        return None;
    }

    let mut rendered = String::new();
    html::push_html(&mut rendered, events.into_iter());
    Some(html_to_text(&rendered, config))
}

/// HTML-only 内容从 DOM 提取，HTML实体由解析器解码，script/style等子树整段跳过。
pub(super) fn html_to_text(input: &str, config: &PlainTextConversionSettings) -> String {
    let document = Document::from(input);
    let mut renderer = TextRenderer::new(config);
    let mut stack = vec![(document.root(), false)];
    while let Some((node, exiting)) = stack.pop() {
        if exiting {
            renderer.exit(node);
            continue;
        }
        if !renderer.enter(node) {
            continue;
        }
        stack.push((node, true));
        stack.extend(node.children_it(true).map(|child| (child, false)));
    }
    if let Some(start) = renderer.generated_tail {
        renderer.output.truncate(start);
    }
    if config.collapse_blank_lines {
        return collapse_blank_lines(&renderer.output, &renderer.protected_ranges);
    }

    renderer.output
}

struct TextRenderer<'a> {
    config: &'a PlainTextConversionSettings,
    output: String,
    pending_space: bool,
    pre_depth: usize,
    lists: Vec<Option<i64>>,
    row_cells: Vec<usize>,
    generated_tail: Option<usize>,
    links: Vec<(usize, Option<String>)>,
    protected_depth: usize,
    protected_start: usize,
    protected_ranges: Vec<(usize, usize)>,
}

impl<'a> TextRenderer<'a> {
    fn new(config: &'a PlainTextConversionSettings) -> Self {
        Self {
            config,
            output: String::new(),
            pending_space: false,
            pre_depth: 0,
            lists: Vec::new(),
            row_cells: Vec::new(),
            generated_tail: None,
            links: Vec::new(),
            protected_depth: 0,
            protected_start: 0,
            protected_ranges: Vec::new(),
        }
    }

    /// 文本节点外按HTML可读空白折叠；pre/code正文保留，非递归遍历防止恶意深层嵌套溢栈。
    fn enter(&mut self, node: NodeRef<'_>) -> bool {
        if node.is_text() {
            let text = node.text();
            if self.pre_depth > 0 {
                self.output.push_str(&text);
                self.generated_tail = None;
            } else {
                self.append_text(&text);
            }
            return false;
        }
        let name = node.node_name().unwrap_or_default();
        match name.as_ref() {
            "script" | "style" | "head" | "template" | "noscript" | "iframe" | "object"
            | "embed" => return false,
            "pre" => {
                self.boundary();
                self.pre_depth += 1;
                self.start_protection();
            }
            "code" => self.start_protection(),
            "br" | "hr" => {
                self.output.push('\n');
                self.pending_space = false;
                self.generated_tail = None;
            }
            "ul" | "ol" => {
                self.boundary();
                self.lists.push(if name.as_ref() == "ol" {
                    Some(
                        node.attr("start")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(1),
                    )
                } else {
                    None
                });
            }
            "li" => {
                self.boundary();
                self.output
                    .push_str(&"  ".repeat(self.lists.len().saturating_sub(1)));
                let number = self.lists.last_mut().and_then(|next| {
                    next.as_mut().map(|next| {
                        let number = node
                            .attr("value")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(*next);
                        *next = number.saturating_add(1);
                        number
                    })
                });
                if !self.in_code()
                    && self.config.list_style == ListStyle::ReadableMarkers
                    && !has_task_checkbox(node)
                {
                    if let Some(number) = number {
                        self.output.push_str(&format!("{number}. "));
                    } else {
                        self.output.push_str("• ");
                    }
                }
                self.generated_tail = None;
            }
            "tr" => {
                self.boundary();
                self.row_cells.push(0);
            }
            "td" | "th" => {
                let separator = if self.in_code() {
                    TableSeparator::Tab
                } else {
                    self.config.table_separator
                };
                if let Some(count) = self.row_cells.last_mut() {
                    if *count > 0 {
                        self.output.push(match separator {
                            TableSeparator::Tab => '\t',
                            TableSeparator::Space => ' ',
                        });
                    }
                    *count += 1;
                }
                self.pending_space = false;
            }
            "img" => {
                if let Some(alt) = node
                    .attr("alt")
                    .filter(|_| self.in_code() || self.config.image_style == ImageStyle::Alt)
                {
                    self.append_text(&alt);
                }
            }
            "a" => {
                let href = node.attr("href").map(|value| value.to_string());
                if !self.in_code() && self.config.link_style == LinkStyle::Url {
                    if let Some(href) = &href {
                        self.append_text(href);
                        return false;
                    }
                }
                self.links.push((self.output.len(), href));
            }
            "input" => {
                if (self.in_code() || self.config.task_style == TaskStyle::Status)
                    && node
                        .attr("type")
                        .is_some_and(|value| value.eq_ignore_ascii_case("checkbox"))
                {
                    self.output.push_str(if node.attr("checked").is_some() {
                        "☑ "
                    } else {
                        "☐ "
                    });
                    self.generated_tail = None;
                }
            }
            "p" if node.parent().is_some_and(|parent| {
                parent.node_name().is_some_and(|name| name.as_ref() == "li")
            }) && self.output.ends_with(' ') => {}
            name if is_block(name) => self.boundary(),
            _ => {}
        }
        true
    }

    fn exit(&mut self, node: NodeRef<'_>) {
        let name = node.node_name().unwrap_or_default();
        match name.as_ref() {
            "pre" => {
                self.pre_depth -= 1;
                self.end_protection();
                self.boundary();
            }
            "code" => self.end_protection(),
            "ul" | "ol" => {
                self.lists.pop();
                self.boundary();
            }
            "tr" => {
                self.row_cells.pop();
                self.boundary();
            }
            "li" => self.boundary(),
            "a" => {
                if let Some((start, Some(href))) = self.links.pop() {
                    if !self.in_code() && self.config.link_style == LinkStyle::TitleAndUrl {
                        let title = self.output[start..].trim();
                        if title.is_empty() {
                            self.output.truncate(start);
                            self.protected_ranges
                                .retain_mut(|(range_start, range_end)| {
                                    *range_end = (*range_end).min(start);
                                    *range_start < *range_end
                                });
                            self.generated_tail = None;
                            self.append_text(&href);
                        } else if title != href.trim() && !href.trim().is_empty() {
                            self.append_text(&format!(" ({})", href.trim()));
                        }
                    }
                }
            }
            name if is_block(name) => self.boundary(),
            _ => {}
        }
    }

    fn append_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_ascii_whitespace() || ch == '\u{a0}' {
                self.pending_space = true;
                continue;
            }
            if self.pending_space
                && !self.output.is_empty()
                && !self.output.ends_with(['\n', '\t', ' '])
            {
                self.output.push(' ');
            }
            self.pending_space = false;
            self.output.push(ch);
            self.generated_tail = None;
        }
    }

    fn boundary(&mut self) {
        self.pending_space = false;
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.generated_tail = Some(self.output.len());
            self.output.push('\n');
        }
    }

    fn start_protection(&mut self) {
        if self.protected_depth == 0 {
            self.protected_start = self.output.len();
        }
        self.protected_depth += 1;
    }

    /// 代码内部沿用默认结构提取，仅允许后续自定义全文规则主动改写。
    fn in_code(&self) -> bool {
        self.protected_depth > 0
    }

    fn end_protection(&mut self) {
        self.protected_depth -= 1;
        if self.protected_depth == 0 {
            self.protected_ranges
                .push((self.protected_start, self.output.len()));
        }
    }
}

/// 只识别本层列表项内的任务框，嵌套列表的任务状态不能改变父项符号。
fn has_task_checkbox(node: NodeRef<'_>) -> bool {
    let mut stack: Vec<_> = node.children_it(false).collect();
    while let Some(node) = stack.pop() {
        let name = node.node_name().unwrap_or_default();
        if matches!(name.as_ref(), "ul" | "ol" | "li") {
            continue;
        }
        if name.as_ref() == "input"
            && node
                .attr("type")
                .is_some_and(|value| value.eq_ignore_ascii_case("checkbox"))
        {
            return true;
        }
        stack.extend(node.children_it(false));
    }

    false
}

/// 结构阶段最多保留一个空行；代码区间逐字保留，普通原生 Text 不经过这里。
fn collapse_blank_lines(input: &str, protected: &[(usize, usize)]) -> String {
    let mut output = String::new();
    let mut range = 0;
    let mut newlines = 0;
    for (index, ch) in input.char_indices() {
        while range < protected.len() && protected[range].1 <= index {
            range += 1;
        }
        let is_protected = protected
            .get(range)
            .is_some_and(|&(start, end)| index >= start && index < end);
        if ch == '\n' {
            newlines += 1;
            if !is_protected && newlines > 2 {
                continue;
            }
        } else {
            newlines = 0;
        }
        output.push(ch);
    }

    output
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "table"
            | "dl"
            | "dt"
            | "dd"
    )
}
