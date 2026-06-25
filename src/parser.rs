use anyhow::{Result, bail};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ir::{Alignment, Block, Inline, ListItem};

const PAGE_BREAK_DIRECTIVE: &str = r"\pagebreak";

pub fn parse_markdown(input: &str) -> Result<Vec<Block>> {
    let with_divs = preprocess_div_tags(input);
    let preprocessed = preprocess_inline_footnotes(&with_divs);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(&preprocessed, options);
    let events: Vec<Event> = parser.collect();

    let converter = EventConverter::new();
    let blocks = converter.convert(&events);
    validate_page_break_usage(&blocks)?;
    Ok(blocks)
}

/// インライン注釈 `^[注釈内容]` を通常の脚注記法 `[^__inline_fn_1]` および `[^__inline_fn_1]: 注釈内容` に変換する。
/// コードブロックやインラインコードの中身は変換対象外とする。
fn preprocess_inline_footnotes(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + 256);
    let mut inline_footnotes = Vec::new();

    let mut in_code_block = false;
    let mut code_block_fence = "";
    let mut in_code_span = false;
    let mut code_span_ticks = 0;

    let chars = input.chars().collect::<Vec<char>>();
    let mut i = 0;

    while i < chars.len() {
        // 1. 行頭でのコードブロックフェンスの検出とトグル
        let is_line_start = i == 0 || chars[i - 1] == '\n';
        if is_line_start {
            let remaining = &chars[i..];
            let mut fence_len = 0;
            let mut fence_char = ' ';
            if remaining.starts_with(&['`', '`', '`']) {
                fence_char = '`';
                fence_len = 3;
            } else if remaining.starts_with(&['~', '~', '~']) {
                fence_char = '~';
                fence_len = 3;
            }
            if fence_len == 3 {
                let mut idx = i + 3;
                while idx < chars.len() && chars[idx] == fence_char {
                    idx += 1;
                }
                let count = idx - i;
                if in_code_block {
                    if fence_char == code_block_fence.chars().next().unwrap() && count >= code_block_fence.len() {
                        in_code_block = false;
                    }
                } else {
                    in_code_block = true;
                    code_block_fence = if fence_char == '`' { "```" } else { "~~~" };
                }
            }
        }

        if in_code_block {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // 2. インラインコードスパンの検出とトグル
        if chars[i] == '`' {
            let mut count = 0;
            while i + count < chars.len() && chars[i + count] == '`' {
                count += 1;
            }
            if in_code_span {
                if count == code_span_ticks {
                    in_code_span = false;
                }
            } else {
                in_code_span = true;
                code_span_ticks = count;
            }
            for _ in 0..count {
                result.push('`');
            }
            i += count;
            continue;
        }

        if in_code_span {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // 3. インライン注釈の開始 `^[` の検出とパース
        if chars[i] == '^' && i + 1 < chars.len() && chars[i + 1] == '[' {
            i += 2; // `^[` をスキップ
            let mut content = String::new();
            let mut depth = 1;
            while i < chars.len() {
                let c = chars[i];
                if c == '[' {
                    depth += 1;
                    content.push(c);
                    i += 1;
                } else if c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1; // 閉じ括弧 `]` をスキップ
                        break;
                    }
                    content.push(c);
                    i += 1;
                } else {
                    content.push(c);
                    i += 1;
                }
            }
            let label = format!("__inline_fn_{}", inline_footnotes.len() + 1);
            result.push_str(&format!("[^{}]", label));
            inline_footnotes.push((label, content));
            continue;
        }

        // 4. 通常文字
        result.push(chars[i]);
        i += 1;
    }

    // 末尾に脚注定義を追加
    if !inline_footnotes.is_empty() {
        for (label, content) in inline_footnotes {
            result.push_str(&format!("\n\n[^{}]: {}", label, content));
        }
    }

    result
}

/// <div class="..."> と </div> の後に空行がない場合、空行を挿入する。
/// pulldown-cmark が各タグを独立した HTML ブロックとして認識するために必要。
fn preprocess_div_tags(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut result = String::with_capacity(input.len() + 64);
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // 単一行: <div class="...">content</div> → 複数行に分割
        if extract_div_class(trimmed).is_some() {
            if let Some(gt_pos) = trimmed.find('>') {
                let after_open = &trimmed[gt_pos + 1..];
                if let Some(close_pos) = after_open.find("</div>") {
                    let opening_tag = &trimmed[..gt_pos + 1];
                    let content = after_open[..close_pos].trim();
                    result.push_str(opening_tag);
                    result.push_str("\n\n");
                    if !content.is_empty() {
                        result.push_str(content);
                        result.push_str("\n\n");
                    }
                    result.push_str("</div>\n");
                    let next_is_blank = i + 1 >= lines.len() || lines[i + 1].trim().is_empty();
                    if !next_is_blank {
                        result.push('\n');
                    }
                    continue;
                }
            }
        }

        result.push_str(line);
        result.push('\n');
        let is_div_tag = extract_div_class(trimmed).is_some() || trimmed.starts_with("</div>");
        if is_div_tag {
            let next_is_blank = i + 1 >= lines.len() || lines[i + 1].trim().is_empty();
            if !next_is_blank {
                result.push('\n');
            }
        }
    }
    result
}

struct EventConverter {
    blocks: Vec<Block>,
    inline_stack: Vec<Vec<Inline>>,
    list_stack: Vec<ListContext>,
    table_state: Option<TableState>,
    block_quote_stack: Vec<Vec<Block>>,
    current_image_path: Option<String>,
    current_code_lang: Option<String>,
    current_link_url: Option<String>,
    span_class_stack: Vec<String>,
    div_class_stack: Vec<String>,
    div_blocks_stack: Vec<Vec<Block>>,
    footnote_stack: Vec<(String, Vec<Block>)>,
}

struct ListContext {
    ordered: bool,
    start: u64,
    items: Vec<ListItem>,
    current_item_inlines: Vec<Inline>,
    current_item_children: Vec<Block>,
}

struct TableState {
    headers: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    current_row: Vec<Vec<Inline>>,
    current_cell: Vec<Inline>,
    in_header: bool,
    alignments: Vec<Alignment>,
}

impl EventConverter {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            inline_stack: Vec::new(),
            list_stack: Vec::new(),
            table_state: None,
            block_quote_stack: Vec::new(),
            current_image_path: None,
            current_code_lang: None,
            current_link_url: None,
            span_class_stack: Vec::new(),
            div_class_stack: Vec::new(),
            div_blocks_stack: Vec::new(),
            footnote_stack: Vec::new(),
        }
    }

    fn convert(mut self, events: &[Event]) -> Vec<Block> {
        for event in events {
            self.process_event(event);
        }
        self.blocks
    }

    fn push_inline(&mut self, inline: Inline) {
        if let Some(stack) = self.inline_stack.last_mut() {
            stack.push(inline);
        } else if let Some(ref mut table) = self.table_state {
            table.current_cell.push(inline);
        } else if let Some(list_ctx) = self.list_stack.last_mut() {
            list_ctx.current_item_inlines.push(inline);
        }
    }

    fn process_event(&mut self, event: &Event) {
        match event {
            Event::Start(tag) => self.handle_start(tag),
            Event::End(tag) => self.handle_end(tag),
            Event::Text(text) => {
                self.push_inline(Inline::Text(text.to_string()));
            }
            Event::Code(code) => {
                self.push_inline(Inline::Code(code.to_string()));
            }
            Event::SoftBreak => {
                self.push_inline(Inline::SoftBreak);
            }
            Event::HardBreak => {
                self.push_inline(Inline::HardBreak);
            }
            Event::InlineHtml(html) => {
                self.handle_inline_html(html);
            }
            Event::Html(html) => {
                self.handle_block_html(html);
            }
            Event::InlineMath(math) => {
                self.push_inline(Inline::InlineMath(math.to_string()));
            }
            Event::DisplayMath(math) => {
                self.add_block(Block::DisplayMath(math.to_string()));
            }
            Event::FootnoteReference(label) => {
                self.push_inline(Inline::FootnoteReference(label.to_string()));
            }
            Event::Rule => {
                self.add_block(Block::ThematicBreak);
            }
            _ => {}
        }
    }

    fn handle_start(&mut self, tag: &Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.inline_stack.push(Vec::new());
                let _ = level; // level is used in handle_end
            }
            Tag::Paragraph => {
                self.inline_stack.push(Vec::new());
            }
            Tag::Emphasis => {
                self.inline_stack.push(Vec::new());
            }
            Tag::Strong => {
                self.inline_stack.push(Vec::new());
            }
            Tag::Link { dest_url, .. } => {
                self.inline_stack.push(Vec::new());
                self.current_link_url = Some(dest_url.to_string());
            }
            Tag::List(start) => {
                let ordered = start.is_some();
                let start_num = start.unwrap_or(1);
                self.list_stack.push(ListContext {
                    ordered,
                    start: start_num,
                    items: Vec::new(),
                    current_item_inlines: Vec::new(),
                    current_item_children: Vec::new(),
                });
            }
            Tag::Item => {
                if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.current_item_inlines = Vec::new();
                    list_ctx.current_item_children = Vec::new();
                }
            }
            Tag::Table(alignments) => {
                let aligns = alignments
                    .iter()
                    .map(|a| match a {
                        pulldown_cmark::Alignment::Left => Alignment::Left,
                        pulldown_cmark::Alignment::Center => Alignment::Center,
                        pulldown_cmark::Alignment::Right => Alignment::Right,
                        pulldown_cmark::Alignment::None => Alignment::None,
                    })
                    .collect();
                self.table_state = Some(TableState {
                    headers: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: Vec::new(),
                    in_header: false,
                    alignments: aligns,
                });
            }
            Tag::TableHead => {
                if let Some(ref mut state) = self.table_state {
                    state.in_header = true;
                    state.current_row = Vec::new();
                }
            }
            Tag::TableRow => {
                if let Some(ref mut state) = self.table_state {
                    state.current_row = Vec::new();
                }
            }
            Tag::TableCell => {
                if let Some(ref mut state) = self.table_state {
                    state.current_cell = Vec::new();
                }
            }
            Tag::BlockQuote(_) => {
                self.block_quote_stack.push(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.current_code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = lang.to_string();
                        if l.is_empty() { None } else { Some(l) }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                self.inline_stack.push(Vec::new());
            }
            Tag::Image { dest_url, .. } => {
                self.inline_stack.push(Vec::new());
                self.current_image_path = Some(dest_url.to_string());
            }
            Tag::FootnoteDefinition(label) => {
                self.footnote_stack.push((label.to_string(), Vec::new()));
            }
            _ => {}
        }
    }

    fn handle_end(&mut self, tag: &TagEnd) {
        match tag {
            TagEnd::Heading(level) => {
                let content = self.inline_stack.pop().unwrap_or_default();
                let lvl = heading_level_to_u8(level);
                self.add_block(Block::Heading {
                    level: lvl,
                    content,
                });
            }
            TagEnd::Paragraph => {
                let content = self.inline_stack.pop().unwrap_or_default();
                if is_page_break_paragraph(&content) {
                    self.add_block(Block::PageBreak);
                } else if !content.is_empty() {
                    self.add_block(Block::Paragraph { content });
                }
            }
            TagEnd::Emphasis => {
                let children = self.inline_stack.pop().unwrap_or_default();
                self.push_inline(Inline::Italic(children));
            }
            TagEnd::Strong => {
                let children = self.inline_stack.pop().unwrap_or_default();
                self.push_inline(Inline::Bold(children));
            }
            TagEnd::Link => {
                let text = self.inline_stack.pop().unwrap_or_default();
                let url = self.current_link_url.take().unwrap_or_default();
                if url.is_empty() {
                    for inline in text {
                        self.push_inline(inline);
                    }
                } else {
                    self.push_inline(Inline::Link { text, url });
                }
            }
            TagEnd::List(_ordered) => {
                if let Some(list_ctx) = self.list_stack.pop() {
                    let block = if list_ctx.ordered {
                        Block::OrderedList {
                            items: list_ctx.items,
                            start: list_ctx.start,
                        }
                    } else {
                        Block::BulletList {
                            items: list_ctx.items,
                        }
                    };
                    self.add_block(block);
                }
            }
            TagEnd::Item => {
                if let Some(list_ctx) = self.list_stack.last_mut() {
                    let item = ListItem {
                        content: std::mem::take(&mut list_ctx.current_item_inlines),
                        children: std::mem::take(&mut list_ctx.current_item_children),
                    };
                    list_ctx.items.push(item);
                }
            }
            TagEnd::Table => {
                if let Some(state) = self.table_state.take() {
                    self.add_block(Block::Table {
                        headers: state.headers,
                        rows: state.rows,
                        alignments: state.alignments,
                    });
                }
            }
            TagEnd::TableHead => {
                if let Some(ref mut state) = self.table_state {
                    state.headers = std::mem::take(&mut state.current_row);
                    state.in_header = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(ref mut state) = self.table_state
                    && !state.in_header
                {
                    let row = std::mem::take(&mut state.current_row);
                    state.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                if let Some(ref mut state) = self.table_state {
                    let cell = std::mem::take(&mut state.current_cell);
                    state.current_row.push(cell);
                }
            }
            TagEnd::BlockQuote(_) => {
                let children = self.block_quote_stack.pop().unwrap_or_default();
                self.add_block(Block::BlockQuote { children });
            }
            TagEnd::CodeBlock => {
                let content = self.inline_stack.pop().unwrap_or_default();
                let code: String = content
                    .iter()
                    .map(|i| match i {
                        Inline::Text(s) => s.as_str(),
                        _ => "",
                    })
                    .collect();
                let lang = self.current_code_lang.take();
                self.add_block(Block::CodeBlock { lang, code });
            }
            TagEnd::Image => {
                let alt_inlines = self.inline_stack.pop().unwrap_or_default();
                let alt: String = alt_inlines.iter().map(|i| i.to_plain_text()).collect();
                let path = self.current_image_path.take().unwrap_or_default();
                self.add_block(Block::Image { alt, path });
            }
            TagEnd::FootnoteDefinition => {
                if let Some((label, children)) = self.footnote_stack.pop() {
                    self.add_block(Block::FootnoteDefinition { label, children });
                }
            }
            _ => {}
        }
    }

    fn handle_block_html(&mut self, html: &str) {
        for line in html.lines() {
            let trimmed = line.trim();
            if let Some(class) = extract_div_class(trimmed) {
                self.div_class_stack.push(class);
                self.div_blocks_stack.push(Vec::new());
            } else if trimmed.starts_with("</div>") && !self.div_class_stack.is_empty() {
                let class = self.div_class_stack.pop().unwrap();
                let children = self.div_blocks_stack.pop().unwrap_or_default();
                self.add_block(Block::StyledDiv { class, children });
            }
        }
    }

    fn handle_inline_html(&mut self, html: &str) {
        let html_trimmed = html.trim();
        if let Some(class) = extract_span_class(html_trimmed) {
            self.span_class_stack.push(class);
            self.inline_stack.push(Vec::new());
        } else if html_trimmed == "</span>" && !self.span_class_stack.is_empty() {
            let class = self.span_class_stack.pop().unwrap();
            let children = self.inline_stack.pop().unwrap_or_default();
            self.push_inline(Inline::StyledSpan { class, children });
        }
    }

    fn add_block(&mut self, block: Block) {
        if !self.div_blocks_stack.is_empty() {
            self.div_blocks_stack.last_mut().unwrap().push(block);
        } else if !self.block_quote_stack.is_empty() {
            self.block_quote_stack.last_mut().unwrap().push(block);
        } else if !self.footnote_stack.is_empty() {
            self.footnote_stack.last_mut().unwrap().1.push(block);
        } else if !self.list_stack.is_empty() {
            // リスト内のネストされたブロック
            if let Some(list_ctx) = self.list_stack.last_mut() {
                // Paragraph内のインラインをリストアイテムに移動
                match &block {
                    Block::Paragraph { content } => {
                        if list_ctx.current_item_inlines.is_empty() {
                            list_ctx.current_item_inlines = content.clone();
                        } else {
                            list_ctx.current_item_children.push(block);
                        }
                    }
                    _ => {
                        list_ctx.current_item_children.push(block);
                    }
                }
            }
        } else {
            self.blocks.push(block);
        }
    }
}

fn extract_span_class(html: &str) -> Option<String> {
    let s = html.trim();
    if !s.starts_with("<span") || !s.ends_with('>') {
        return None;
    }
    // Find class="..." or class='...'
    let class_idx = s.find("class=")?;
    let after = &s[class_idx + 6..];
    let (quote, rest) = if after.starts_with('"') {
        ('"', &after[1..])
    } else if after.starts_with('\'') {
        ('\'', &after[1..])
    } else {
        return None;
    };
    let end = rest.find(quote)?;
    let class_name = rest[..end].trim().to_string();
    if class_name.is_empty() {
        return None;
    }
    Some(class_name)
}

fn extract_div_class(html: &str) -> Option<String> {
    let s = html.trim();
    if !s.starts_with("<div") || !s.ends_with('>') {
        return None;
    }
    let class_idx = s.find("class=")?;
    let after = &s[class_idx + 6..];
    let (quote, rest) = if after.starts_with('"') {
        ('"', &after[1..])
    } else if after.starts_with('\'') {
        ('\'', &after[1..])
    } else {
        return None;
    };
    let end = rest.find(quote)?;
    let class_name = rest[..end].trim().to_string();
    if class_name.is_empty() {
        return None;
    }
    Some(class_name)
}

fn is_page_break_paragraph(content: &[Inline]) -> bool {
    matches!(content, [Inline::Text(text)] if text.trim() == PAGE_BREAK_DIRECTIVE)
}

fn validate_page_break_usage(blocks: &[Block]) -> Result<()> {
    for block in blocks {
        validate_block(block)?;
    }
    Ok(())
}

fn validate_block(block: &Block) -> Result<()> {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => {
            validate_inlines(content)?;
        }
        Block::BulletList { items } | Block::OrderedList { items, .. } => {
            for item in items {
                validate_inlines(&item.content)?;
                for child in &item.children {
                    validate_block(child)?;
                }
            }
        }
        Block::Table { headers, rows, .. } => {
            for cell in headers.iter().flatten() {
                validate_inline(cell)?;
            }
            for row in rows {
                for cell in row {
                    validate_inlines(cell)?;
                }
            }
        }
        Block::CodeBlock { code, .. } => {
            ensure_no_page_break_directive(code)?;
        }
        Block::Image { alt, .. } => {
            ensure_no_page_break_directive(alt)?;
        }
        Block::BlockQuote { children } => {
            for child in children {
                validate_block(child)?;
            }
        }
        Block::PageBreak | Block::ThematicBreak => {}
        Block::StyledDiv { class, children } => {
            ensure_no_page_break_directive(class)?;
            for child in children {
                validate_block(child)?;
            }
        }
        Block::FootnoteDefinition { children, .. } => {
            for child in children {
                validate_block(child)?;
            }
        }
        Block::DisplayMath(text) => ensure_no_page_break_directive(text)?,
    }
    Ok(())
}

fn validate_inlines(inlines: &[Inline]) -> Result<()> {
    for inline in inlines {
        validate_inline(inline)?;
    }
    Ok(())
}

fn validate_inline(inline: &Inline) -> Result<()> {
    match inline {
        Inline::Text(text) | Inline::Code(text) => ensure_no_page_break_directive(text)?,
        Inline::Bold(children) | Inline::Italic(children) => validate_inlines(children)?,
        Inline::Link { text, url } => {
            validate_inlines(text)?;
            ensure_no_page_break_directive(url)?;
        }
        Inline::SoftBreak | Inline::HardBreak => {}
        Inline::StyledSpan { class, children } => {
            ensure_no_page_break_directive(class)?;
            validate_inlines(children)?;
        }
        Inline::FootnoteReference(label) => ensure_no_page_break_directive(label)?,
        Inline::InlineMath(text) => ensure_no_page_break_directive(text)?,
    }
    Ok(())
}

fn ensure_no_page_break_directive(text: &str) -> Result<()> {
    if text.contains(PAGE_BREAK_DIRECTIVE) {
        bail!(
            "`{}`は段落単位で単独指定した場合のみ利用できます",
            PAGE_BREAK_DIRECTIVE
        );
    }
    Ok(())
}

fn heading_level_to_u8(level: &HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_link_url_in_inline_ir() {
        let blocks = parse_markdown("[Rust](https://www.rust-lang.org/)").unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::Paragraph { content } => match &content[0] {
                Inline::Link { text, url } => {
                    assert_eq!(url, "https://www.rust-lang.org/");
                    assert_eq!(text.len(), 1);
                    assert!(matches!(&text[0], Inline::Text(t) if t == "Rust"));
                }
                other => panic!("unexpected inline: {other:?}"),
            },
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_span_class_to_styled_span() {
        let blocks = parse_markdown("text <span class=\"warning\">important</span> end").unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::Paragraph { content } => {
                let span = content
                    .iter()
                    .find(|i| matches!(i, Inline::StyledSpan { .. }));
                assert!(span.is_some(), "StyledSpan should be found");
                match span.unwrap() {
                    Inline::StyledSpan { class, children } => {
                        assert_eq!(class, "warning");
                        assert_eq!(children.len(), 1);
                        assert!(matches!(&children[0], Inline::Text(t) if t == "important"));
                    }
                    _ => unreachable!(),
                }
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn extract_span_class_parses_correctly() {
        assert_eq!(
            super::extract_span_class("<span class=\"warning\">"),
            Some("warning".to_string())
        );
        assert_eq!(
            super::extract_span_class("<span class='info'>"),
            Some("info".to_string())
        );
        assert_eq!(super::extract_span_class("</span>"), None);
        assert_eq!(super::extract_span_class("<div class=\"x\">"), None);
    }

    #[test]
    fn extract_div_class_parses_correctly() {
        assert_eq!(
            super::extract_div_class("<div class=\"todo\">"),
            Some("todo".to_string())
        );
        assert_eq!(
            super::extract_div_class("<div class='info'>"),
            Some("info".to_string())
        );
        assert_eq!(super::extract_div_class("</div>"), None);
        assert_eq!(super::extract_div_class("<span class=\"x\">"), None);
    }

    #[test]
    fn parses_div_block_to_styled_div() {
        let md = "<div class=\"todo\">\n\nSome text.\n\n</div>\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::StyledDiv { class, children } => {
                assert_eq!(class, "todo");
                assert!(!children.is_empty());
                match &children[0] {
                    Block::Paragraph { content } => {
                        assert!(matches!(&content[0], Inline::Text(t) if t == "Some text."));
                    }
                    other => panic!("unexpected child block: {other:?}"),
                }
            }
            other => panic!("expected StyledDiv, got: {other:?}"),
        }
    }

    #[test]
    fn parses_blockquote() {
        let md = "> Quoted text.\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::BlockQuote { children } => {
                assert_eq!(children.len(), 1);
                match &children[0] {
                    Block::Paragraph { content } => {
                        assert!(matches!(&content[0], Inline::Text(t) if t == "Quoted text."));
                    }
                    other => panic!("unexpected child: {other:?}"),
                }
            }
            other => panic!("expected BlockQuote, got: {other:?}"),
        }
    }

    #[test]
    fn parses_nested_blockquote() {
        let md = "> outer\n>\n> > inner\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::BlockQuote { children } => {
                // outer paragraph + nested blockquote
                let has_nested = children
                    .iter()
                    .any(|b| matches!(b, Block::BlockQuote { .. }));
                assert!(has_nested, "nested BlockQuote should exist: {children:?}");

                let nested = children
                    .iter()
                    .find(|b| matches!(b, Block::BlockQuote { .. }))
                    .unwrap();
                match nested {
                    Block::BlockQuote { children: inner } => {
                        assert!(!inner.is_empty(), "inner blockquote should have content");
                    }
                    _ => unreachable!(),
                }
            }
            other => panic!("expected BlockQuote, got: {other:?}"),
        }
    }

    #[test]
    fn does_not_mix_urls_between_multiple_links() {
        let blocks = parse_markdown("[A](https://a.example) [B](https://b.example)").unwrap();
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            Block::Paragraph { content } => {
                let links: Vec<&Inline> = content
                    .iter()
                    .filter(|i| matches!(i, Inline::Link { .. }))
                    .collect();
                assert_eq!(links.len(), 2);

                match links[0] {
                    Inline::Link { url, .. } => assert_eq!(url, "https://a.example"),
                    _ => unreachable!(),
                }
                match links[1] {
                    Inline::Link { url, .. } => assert_eq!(url, "https://b.example"),
                    _ => unreachable!(),
                }
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn single_line_div_does_not_swallow_following_content() {
        let md = "# Before\n\n<div class=\"todo\">TODO: text</div>\nAfter div.\n\n## Also after\n";
        let blocks = parse_markdown(md).unwrap();
        assert!(blocks.len() >= 3, "got {} blocks: {blocks:?}", blocks.len());
        assert!(matches!(&blocks[0], Block::Heading { level: 1, .. }));
        assert!(matches!(&blocks[1], Block::StyledDiv { .. }));
        // "After div." and "## Also after" must be present
        let has_after = blocks.iter().any(|b| matches!(b, Block::Paragraph { content } if content.iter().any(|i| matches!(i, Inline::Text(t) if t.contains("After div")))));
        assert!(has_after, "paragraph after div is missing: {blocks:?}");
    }

    #[test]
    fn div_followed_by_content_without_blank_line() {
        let md = "<div class=\"todo\">\n\nTODO item.\n\n</div>\nAfter div.\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 2, "should have StyledDiv + Paragraph");
        assert!(matches!(&blocks[0], Block::StyledDiv { .. }));
        assert!(matches!(&blocks[1], Block::Paragraph { .. }));
    }

    #[test]
    fn parses_inline_math() {
        let blocks = parse_markdown("The equation $x^2$ is simple.").unwrap();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph { content } => {
                let math = content.iter().find(|i| matches!(i, Inline::InlineMath(_)));
                assert!(math.is_some(), "InlineMath should be found");
                match math.unwrap() {
                    Inline::InlineMath(s) => assert_eq!(s, "x^2"),
                    _ => unreachable!(),
                }
            }
            other => panic!("expected Paragraph, got: {other:?}"),
        }
    }

    #[test]
    fn parses_display_math() {
        let blocks = parse_markdown("$$\\frac{a}{b}$$\n").unwrap();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::DisplayMath(s) => assert_eq!(s, "\\frac{a}{b}"),
            other => panic!("expected DisplayMath, got: {other:?}"),
        }
    }

    #[test]
    fn div_with_content_after_blank_line_still_works() {
        let md = "<div class=\"todo\">\n\nTODO item.\n\n</div>\n\nAfter div.\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::StyledDiv { .. }));
        assert!(matches!(&blocks[1], Block::Paragraph { .. }));
    }

    #[test]
    fn parses_page_break_directive_as_dedicated_block() {
        let blocks = parse_markdown("\\pagebreak").unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], Block::PageBreak));
    }

    #[test]
    fn rejects_page_break_directive_inside_regular_paragraph() {
        let error = parse_markdown("before \\pagebreak after").unwrap_err();
        assert!(
            error
                .to_string()
                .contains(r"`\pagebreak`は段落単位で単独指定した場合のみ利用できます")
        );
    }

    #[test]
    fn parses_footnotes() {
        let md = "Here is a reference[^1].\n\n[^1]: This is the footnote content.\n";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 2);

        match &blocks[0] {
            Block::Paragraph { content } => {
                assert_eq!(content.len(), 3);
                assert!(matches!(&content[0], Inline::Text(t) if t == "Here is a reference"));
                assert!(matches!(&content[1], Inline::FootnoteReference(label) if label == "1"));
                assert!(matches!(&content[2], Inline::Text(t) if t == "."));
            }
            other => panic!("expected paragraph, got {:?}", other),
        }

        match &blocks[1] {
            Block::FootnoteDefinition { label, children } => {
                assert_eq!(label, "1");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    Block::Paragraph { content } => {
                        assert_eq!(content.len(), 1);
                        assert!(matches!(&content[0], Inline::Text(t) if t == "This is the footnote content."));
                    }
                    other => panic!("expected paragraph inside footnote, got {:?}", other),
                }
            }
            other => panic!("expected FootnoteDefinition, got {:?}", other),
        }
    }

    #[test]
    fn parses_inline_footnotes() {
        let md = "Here is an inline footnote^[This is the inline footnote content.].";
        let blocks = parse_markdown(md).unwrap();
        assert_eq!(blocks.len(), 2);

        match &blocks[0] {
            Block::Paragraph { content } => {
                assert_eq!(content.len(), 3);
                assert!(matches!(&content[0], Inline::Text(t) if t == "Here is an inline footnote"));
                assert!(matches!(&content[1], Inline::FootnoteReference(label) if label == "__inline_fn_1"));
                assert!(matches!(&content[2], Inline::Text(t) if t == "."));
            }
            other => panic!("expected paragraph, got {:?}", other),
        }

        match &blocks[1] {
            Block::FootnoteDefinition { label, children } => {
                assert_eq!(label, "__inline_fn_1");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    Block::Paragraph { content } => {
                        assert_eq!(content.len(), 1);
                        assert!(matches!(&content[0], Inline::Text(t) if t == "This is the inline footnote content."));
                    }
                    other => panic!("expected paragraph inside footnote, got {:?}", other),
                }
            }
            other => panic!("expected FootnoteDefinition, got {:?}", other),
        }
    }
}
