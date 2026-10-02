//! 结构化去除常见 Markdown / HTML 标记；不执行脚本或访问外部资源。

use std::sync::LazyLock;

use dom_query::{Document, NodeRef};
use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag};
use regex::Regex;

/// 仅用标签语法识别明确 HTML；正文提取由 HTML5 DOM 完成。
/// 泛型 `<T>`、比较式、邮箱和不完整尖括号不会被当作 HTML。
static HTML_MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</?(?:html|body|head|title|p|div|span|b|strong|i|em|u|s|del|a|img|br|hr|h[1-6]|ul|ol|li|blockquote|pre|code|table|thead|tbody|tfoot|tr|td|th|section|article|main|header|footer|script|style|template|noscript|iframe|object|embed|dl|dt|dd)(?:\s[^<>]*|\s*/?)>")
        .expect("valid HTML identification pattern")
});

/// 普通文本返回 None，调用者保留其原始空白和换行。
/// CommonMark 加表格、删除线和任务列表；只有确实识别到标记才应用解析输出。
pub(super) fn convert_markup(input: &str) -> Option<String> {
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
    Some(html_to_text(&rendered))
}

/// HTML-only 内容从 DOM 提取，HTML实体由解析器解码，script/style等子树整段跳过。
pub(super) fn html_to_text(input: &str) -> String {
    let document = Document::from(input);
    let mut renderer = TextRenderer::default();
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
    renderer.output
}

#[derive(Default)]
struct TextRenderer {
    output: String,
    pending_space: bool,
    pre_depth: usize,
    list_depth: usize,
    row_cells: Vec<usize>,
    generated_tail: Option<usize>,
}

impl TextRenderer {
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
            }
            "br" | "hr" => {
                self.output.push('\n');
                self.pending_space = false;
                self.generated_tail = None;
            }
            "ul" | "ol" => {
                self.boundary();
                self.list_depth += 1;
            }
            "li" => {
                self.boundary();
                self.output
                    .push_str(&"  ".repeat(self.list_depth.saturating_sub(1)));
                self.generated_tail = None;
            }
            "tr" => {
                self.boundary();
                self.row_cells.push(0);
            }
            "td" | "th" => {
                if let Some(count) = self.row_cells.last_mut() {
                    if *count > 0 {
                        self.output.push('\t');
                    }
                    *count += 1;
                }
                self.pending_space = false;
            }
            "img" => {
                if let Some(alt) = node.attr("alt") {
                    self.append_text(&alt);
                }
            }
            "input" => {
                if node
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
                self.boundary();
            }
            "ul" | "ol" => {
                self.list_depth -= 1;
                self.boundary();
            }
            "tr" => {
                self.row_cells.pop();
                self.boundary();
            }
            "li" => self.boundary(),
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
