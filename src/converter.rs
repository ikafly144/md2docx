use std::path::Path;

use anyhow::Result;
use docx_rs::*;
use image::GenericImageView;

use crate::config::{Config, PageConfig};
use crate::css::{CssRules, CssStyle};
use crate::heading::HeadingManager;
use crate::ir::{Block, Inline, ListItem};
use crate::styles;

const EMU_PER_PIXEL: u64 = 9_525;
const EMU_PER_TWIP: u64 = 635;
const TABLE_WIDTH_PCT: usize = 5_000;
const TABLE_CELL_PADDING_TWIP: usize = 80;

pub fn convert_to_docx(
    blocks: &[Block],
    config: &Config,
    css_rules: Option<&CssRules>,
    base_path: &Path,
) -> Result<Docx> {
    let mut ctx = ConvertContext::new(config, css_rules, base_path);
    let mut docx = Docx::new();

    // sample.docx 準拠のスタイル・番号定義を適用
    docx = styles::setup_document_styles(docx, config, css_rules);
    docx = docx
        .page_size(config.page.width, config.page.height)
        .page_margin(
            PageMargin::new()
                .top(config.page.margin_top)
                .right(config.page.margin_right)
                .bottom(config.page.margin_bottom)
                .left(config.page.margin_left)
                .header(config.page.margin_header)
                .footer(config.page.margin_footer)
                .gutter(config.page.margin_gutter),
        );

    // TOC（目次）の挿入
    if config.toc.enable {
        let toc = TableOfContents::new()
            .heading_styles_range(config.toc.min_level, config.toc.max_level)
            .hyperlink()
            .dirty();
        docx = docx.add_table_of_contents(toc);
        // TOC の後に改ページを挿入
        docx = docx.add_paragraph(Paragraph::new().page_break_before(true));
    }

    for block in blocks {
        docx = ctx.convert_block(docx, block)?;
    }

    Ok(docx)
}

struct ConvertContext<'a> {
    config: &'a Config,
    css_rules: Option<&'a CssRules>,
    base_path: &'a Path,
    heading_mgr: HeadingManager,
    /// 現在の H1 章番号（0 = H1 未出現）
    chapter_number: u32,
    /// 章内の図カウンタ
    figure_in_chapter: u32,
    /// 章内の表カウンタ
    table_in_chapter: u32,
    /// グローバル連番（sequential モード用）
    figure_seq: u32,
    table_seq: u32,
}

#[derive(Clone, Copy)]
enum InlineStyle {
    Body,
    TableBody,
    TableHeader,
}

impl<'a> ConvertContext<'a> {
    fn new(config: &'a Config, css_rules: Option<&'a CssRules>, base_path: &'a Path) -> Self {
        Self {
            config,
            css_rules,
            base_path,
            heading_mgr: HeadingManager::new(),
            chapter_number: 0,
            figure_in_chapter: 0,
            table_in_chapter: 0,
            figure_seq: 0,
            table_seq: 0,
        }
    }

    fn convert_block(&mut self, docx: Docx, block: &Block) -> Result<Docx> {
        match block {
            Block::Heading { level, content } => Ok(self.convert_heading(docx, *level, content)),
            Block::PageBreak => Ok(self.convert_page_break(docx)),
            Block::Paragraph { content } => Ok(self.convert_paragraph(docx, content)),
            Block::BulletList { items } => self.convert_bullet_list(docx, items, 0),
            Block::OrderedList { items, start } => {
                self.convert_ordered_list(docx, items, *start, 0)
            }
            Block::Table {
                headers,
                rows,
                alignments,
            } => Ok(self.convert_table(docx, headers, rows, alignments)),
            Block::CodeBlock { lang, code } => {
                Ok(self.convert_code_block(docx, lang.as_deref(), code))
            }
            Block::Image { alt, path } => Ok(self.convert_image(docx, alt, path)),
            Block::BlockQuote { children } => {
                let css_bq = self.css_rules.and_then(|r| r.blockquote.clone());
                let style = css_bq.unwrap_or_else(|| {
                    // デフォルト: color #666666, border-left 3px solid #CCCCCC
                    CssStyle {
                        color: Some("666666".to_string()),
                        border_left: Some(crate::css::CssBorder {
                            style: "solid".to_string(),
                            size_px: 3.0,
                            color: "CCCCCC".to_string(),
                        }),
                        padding_left_pt: Some(12.0),
                        ..CssStyle::default()
                    }
                });
                self.convert_block_with_style(docx, children, &style)
            }
            Block::StyledDiv { class, children } => {
                let css_style = self
                    .css_rules
                    .and_then(|r| r.classes.get(class.as_str()))
                    .cloned();
                match css_style {
                    Some(style) => self.convert_block_with_style(docx, children, &style),
                    None => {
                        eprintln!("warning: Undefined CSS class `{}`", class);
                        let mut d = docx;
                        for child in children {
                            d = self.convert_block(d, child)?;
                        }
                        Ok(d)
                    }
                }
            }
            Block::DisplayMath(latex) => {
                match crate::math::latex_to_omml(latex, true) {
                    Ok(omml) => {
                        let para = Paragraph::new().add_math(MathXml::new(omml));
                        Ok(docx.add_paragraph(para))
                    }
                    Err(_) => {
                        // フォールバック: LaTeX をテキスト出力
                        let para = Paragraph::new()
                            .add_run(self.make_run(&format!("$${latex}$$"), InlineStyle::Body));
                        Ok(docx.add_paragraph(para))
                    }
                }
            }
            Block::ThematicBreak => {
                // 水平線 → 空段落で代替
                Ok(docx.add_paragraph(Paragraph::new()))
            }
        }
    }

    fn convert_block_with_style(
        &mut self,
        docx: Docx,
        children: &[Block],
        css: &CssStyle,
    ) -> Result<Docx> {
        let mut d = docx;
        for child in children {
            // 子ブロックを通常変換してから段落にCSSを適用
            let before_count = count_paragraphs(&d);
            d = self.convert_block(d, child)?;
            let after_count = count_paragraphs(&d);

            // 新しく追加された段落にCSSスタイルを適用
            for i in before_count..after_count {
                if let Some(DocumentChild::Paragraph(para)) = d.document.children.get(i) {
                    let new_para = apply_css_to_paragraph(*para.clone(), css);
                    d.document.children[i] = DocumentChild::Paragraph(Box::new(new_para));
                }
            }
        }
        Ok(d)
    }

    fn convert_heading(&mut self, docx: Docx, level: u8, content: &[Inline]) -> Docx {
        let h1_title = self.config.numbering.h1_title;

        // effective_level: h1_title モードでは H2→1, H3→2, H4→3, H5→4
        let effective_level = if h1_title && level > 1 {
            level - 1
        } else {
            level
        };

        // テキストから既存の番号部分を除去（effective_level のフォーマットで検出）
        let plain_text: String = content.iter().map(|i| i.to_plain_text()).collect();
        let display_text = self
            .heading_mgr
            .strip_number(effective_level, plain_text.trim());

        // Run はテキストのみ（フォント・サイズ・boldはスタイルが担当）
        let run = Run::new().add_text(&display_text);

        // スタイル ID: 見出し1="1", 見出し2="2", ...
        let style_id = effective_level.to_string();

        // inline 構造から番号プレフィックスを除去
        let prefix_chars = plain_text
            .trim()
            .chars()
            .count()
            .saturating_sub(display_text.chars().count());
        let render_content = strip_prefix_from_inlines(content, prefix_chars);
        // 段落にスタイルと numbering を適用
        let para = Paragraph::new()
            .add_run(run)
            .style(&style_id)
            .numbering(
                NumberingId::new(styles::HEADING_NUM_ID),
                IndentLevel::new((effective_level as usize).saturating_sub(1)),
            )
            .keep_next(true);

        let depth = self.config.numbering.heading_numbering_depth;

        if h1_title && level == 1 {
            // H1 はタイトルとして番号なしで出力
            let mut para = Paragraph::new();
            for inline in &render_content {
                para = self.add_inline_to_heading(para, inline, false);
            }
            let para = para.style(&style_id).keep_next(true);
            return docx.add_paragraph(para);
        }

        // heading_numbering が無効、または heading_numbering_depth を超えるレベルは採番しない
        let numbering_enabled = self.config.numbering.heading_numbering && level <= depth;

        if numbering_enabled {
            // heading_mgr のカウンタを effective_level で進める
            // 番号除去済みのテキストを渡すことで、既存番号の二重検出を防ぐ
            let stripped_content = vec![Inline::Text(display_text.clone())];
            let _ = self
                .heading_mgr
                .next_heading(effective_level, &stripped_content);

            // 章番号の更新
            if h1_title {
                // h1_title モード: H2 出現時に章番号を更新
                if level == 2 {
                    self.chapter_number = self.heading_mgr.current_h1_number();
                    self.figure_in_chapter = 0;
                    self.table_in_chapter = 0;
                }
            } else {
                // 通常モード: H1 出現時に章番号を更新
                if level == 1 {
                    self.chapter_number = self.heading_mgr.current_h1_number();
                    self.figure_in_chapter = 0;
                    self.table_in_chapter = 0;
                }
            }

            // numbering の IndentLevel: effective_level - 1
            let indent_level = (effective_level as usize).saturating_sub(1);

            let mut para = Paragraph::new();
            for inline in &render_content {
                para = self.add_inline_to_heading(para, inline, false);
            }
            let para = para
                .style(&style_id)
                .numbering(
                    NumberingId::new(styles::HEADING_NUM_ID),
                    IndentLevel::new(indent_level),
                )
                .keep_next(true);

            docx.add_paragraph(para)
        } else {
            // 採番なし: スタイルのみ適用
            let mut para = Paragraph::new();
            for inline in &render_content {
                para = self.add_inline_to_heading(para, inline, false);
            }
            let para = para.style(&style_id).keep_next(true);

            docx.add_paragraph(para)
        }
    }

    /// 見出し内の Inline 要素をパラグラフに追加する。
    /// body テキストのフォント/サイズは適用せず、見出しの段落スタイルに委ねる。
    fn add_inline_to_heading(&self, para: Paragraph, inline: &Inline, bold: bool) -> Paragraph {
        match inline {
            Inline::Text(text) => {
                let processed = process_text(text);
                let mut run = Run::new().add_text(&processed);
                if bold {
                    run = run.bold();
                }
                para.add_run(run)
            }
            Inline::Code(code) => {
                let display = format!("「{}」", code);
                let mut run = Run::new().add_text(&display);
                if bold {
                    run = run.bold();
                }
                para.add_run(run)
            }
            Inline::Bold(children) => {
                let mut p = para;
                for child in children {
                    p = self.add_inline_to_heading(p, child, true);
                }
                p
            }
            Inline::Italic(children) => {
                let mut p = para;
                for child in children {
                    p = self.add_inline_to_heading(p, child, bold);
                }
                p
            }
            Inline::Link { text, url } => {
                let label: String = text.iter().map(|child| child.to_plain_text()).collect();
                let display = if label.is_empty() { url.clone() } else { label };
                let processed = process_text(&display);
                let mut run = Run::new().add_text(&processed);
                if bold {
                    run = run.bold();
                }
                let hyperlink = if let Some(anchor) = url.strip_prefix('#') {
                    Hyperlink::new(anchor, HyperlinkType::Anchor).add_run(run)
                } else {
                    Hyperlink::new(url, HyperlinkType::External).add_run(run)
                };
                para.add_hyperlink(hyperlink)
            }
            Inline::StyledSpan { class, children } => {
                let style_id = format!("css-{}", class);
                let has_style = self
                    .css_rules
                    .map(|r| r.classes.contains_key(class.as_str()))
                    .unwrap_or(false);
                if !has_style {
                    eprintln!("warning: Undefined CSS class `{}`", class);
                }
                let mut p = para;
                for child in children {
                    match child {
                        Inline::Text(text) => {
                            let processed = process_text(text);
                            let mut run = Run::new().add_text(&processed);
                            if has_style {
                                run = run.style(&style_id);
                            }
                            if bold {
                                run = run.bold();
                            }
                            p = p.add_run(run);
                        }
                        _ => {
                            p = self.add_inline_to_heading(p, child, bold);
                        }
                    }
                }
                p
            }
            Inline::InlineMath(latex) => match crate::math::latex_to_omml(latex, false) {
                Ok(omml) => para.add_math(MathXml::new(omml)),
                Err(_) => para.add_run(self.make_run(&format!("${latex}$"), InlineStyle::Body)),
            },
            Inline::SoftBreak => para.add_run(Run::new().add_text(" ")),
            Inline::HardBreak => para.add_run(Run::new().add_break(BreakType::TextWrapping)),
        }
    }

    fn convert_title(&self, docx: Docx, content: &[Inline]) -> Docx {
        let plain_text: String = content.iter().map(|i| i.to_plain_text()).collect();
        let run = Run::new().add_text(plain_text.trim());
        let para = Paragraph::new().add_run(run).style(styles::TITLE_STYLE_ID);
        docx.add_paragraph(para)
    }

    fn convert_page_break(&self, docx: Docx) -> Docx {
        let para = Paragraph::new().add_run(Run::new().add_break(BreakType::Page));
        docx.add_paragraph(para)
    }

    /// 図番号文字列を生成（chapter: "1.2", sequential: "2"）
    fn next_figure_number(&mut self) -> String {
        self.figure_seq += 1;
        self.figure_in_chapter += 1;
        match self.config.numbering.figure_format.as_str() {
            "chapter" => {
                let ch = if self.chapter_number == 0 {
                    1
                } else {
                    self.chapter_number
                };
                format!("{}.{}", ch, self.figure_in_chapter)
            }
            _ => format!("{}", self.figure_seq),
        }
    }

    /// 表番号文字列を生成（chapter: "1.2", sequential: "2"）
    fn next_table_number(&mut self) -> String {
        self.table_seq += 1;
        self.table_in_chapter += 1;
        match self.config.numbering.table_format.as_str() {
            "chapter" => {
                let ch = if self.chapter_number == 0 {
                    1
                } else {
                    self.chapter_number
                };
                format!("{}.{}", ch, self.table_in_chapter)
            }
            _ => format!("{}", self.table_seq),
        }
    }

    fn convert_paragraph(&self, docx: Docx, content: &[Inline]) -> Docx {
        let para = self
            .build_paragraph(content, false, InlineStyle::Body)
            .style(styles::BODY_TEXT_STYLE_ID);
        docx.add_paragraph(para)
    }

    fn build_paragraph(&self, content: &[Inline], bold: bool, style: InlineStyle) -> Paragraph {
        let mut para = Paragraph::new();
        for inline in content {
            para = self.add_inline_to_paragraph(para, inline, bold, style);
        }
        para
    }

    fn add_inline_to_paragraph(
        &self,
        para: Paragraph,
        inline: &Inline,
        bold: bool,
        style: InlineStyle,
    ) -> Paragraph {
        match inline {
            Inline::Text(text) => {
                let processed = process_text(text);
                let mut run = self.make_run(&processed, style);
                if bold {
                    run = run.bold();
                }
                para.add_run(run)
            }
            Inline::Code(code) => {
                let display = format!("「{}」", code);
                let mut run = self.make_run(&display, style);
                if bold {
                    run = run.bold();
                }
                para.add_run(run)
            }
            Inline::Bold(children) => {
                let mut p = para;
                for child in children {
                    // Bold/Italic → プレーンテキスト化（計画に従いWordスタイルとしてのbold/italicは使わない）
                    p = self.add_inline_to_paragraph(p, child, bold, style);
                }
                p
            }
            Inline::Italic(children) => {
                let mut p = para;
                for child in children {
                    p = self.add_inline_to_paragraph(p, child, bold, style);
                }
                p
            }
            Inline::Link { text, url } => {
                let label: String = text.iter().map(|child| child.to_plain_text()).collect();
                let display = if label.is_empty() { url.clone() } else { label };
                let processed = process_text(&display);

                let mut run = self.make_run(&processed, style);
                if bold {
                    run = run.bold();
                }

                let hyperlink = if let Some(anchor) = url.strip_prefix('#') {
                    Hyperlink::new(anchor, HyperlinkType::Anchor).add_run(run)
                } else {
                    Hyperlink::new(url, HyperlinkType::External).add_run(run)
                };

                para.add_hyperlink(hyperlink)
            }
            Inline::StyledSpan { class, children } => {
                let style_id = format!("css-{}", class);
                let has_style = self
                    .css_rules
                    .map(|r| r.classes.contains_key(class.as_str()))
                    .unwrap_or(false);
                if !has_style {
                    eprintln!("warning: Undefined CSS class `{}`", class);
                }
                let mut p = para;
                for child in children {
                    match child {
                        Inline::Text(text) => {
                            let processed = process_text(text);
                            let mut run = self.make_run(&processed, style);
                            if has_style {
                                run = run.style(&style_id);
                            }
                            if bold {
                                run = run.bold();
                            }
                            p = p.add_run(run);
                        }
                        _ => {
                            p = self.add_inline_to_paragraph(p, child, bold, style);
                        }
                    }
                }
                p
            }
            Inline::InlineMath(latex) => match crate::math::latex_to_omml(latex, false) {
                Ok(omml) => para.add_math(MathXml::new(omml)),
                Err(_) => para.add_run(self.make_run(&format!("${latex}$"), style)),
            },
            Inline::SoftBreak => para.add_run(self.make_run(" ", style)),
            Inline::HardBreak => para.add_run(Run::new().add_break(BreakType::TextWrapping)),
        }
    }

    fn make_run(&self, text: &str, style: InlineStyle) -> Run {
        let (fonts, size) = match style {
            InlineStyle::Body => (
                RunFonts::new()
                    .ascii(&self.config.fonts.body_en)
                    .hi_ansi(&self.config.fonts.body_en)
                    .east_asia(&self.config.fonts.body_ja)
                    .cs(&self.config.fonts.body_en),
                self.config.sizes.body,
            ),
            InlineStyle::TableBody => (
                RunFonts::new()
                    .ascii(&self.config.fonts.body_en)
                    .hi_ansi(&self.config.fonts.body_en)
                    .east_asia(&self.config.fonts.body_ja)
                    .cs(&self.config.fonts.body_en),
                self.config.sizes.table_body,
            ),
            InlineStyle::TableHeader => (
                RunFonts::new()
                    .ascii(&self.config.fonts.heading_en)
                    .hi_ansi(&self.config.fonts.heading_en)
                    .east_asia(&self.config.fonts.heading_ja)
                    .cs(&self.config.fonts.heading_en),
                self.config.sizes.table_header,
            ),
        };

        Run::new()
            .add_text(text)
            .size(styles::pt_to_half_point(size))
            .fonts(fonts)
    }

    fn convert_bullet_list(
        &mut self,
        docx: Docx,
        items: &[ListItem],
        depth: usize,
    ) -> Result<Docx> {
        let mut d = docx;
        for item in items {
            let level = depth.min(2); // 最大レベル2

            let mut para = Paragraph::new().style(styles::BULLET_STYLE_ID).numbering(
                NumberingId::new(styles::BULLET_NUM_ID),
                IndentLevel::new(level),
            );

            for inline in &item.content {
                para = self.add_inline_to_paragraph(para, inline, false, InlineStyle::Body);
            }
            d = d.add_paragraph(para);

            // ネストされたブロック（BulletList の場合は depth をインクリメント）
            for child in &item.children {
                match child {
                    Block::BulletList {
                        items: nested_items,
                    } => {
                        d = self.convert_bullet_list(d, nested_items, depth + 1)?;
                    }
                    Block::OrderedList {
                        items: nested_items,
                        start,
                    } => {
                        d = self.convert_ordered_list(d, nested_items, *start, depth + 1)?;
                    }
                    _ => {
                        d = self.convert_block(d, child)?;
                    }
                }
            }
        }
        Ok(d)
    }

    fn convert_ordered_list(
        &mut self,
        docx: Docx,
        items: &[ListItem],
        start: u64,
        depth: usize,
    ) -> Result<Docx> {
        let mut d = docx;
        for (i, item) in items.iter().enumerate() {
            let num = start + i as u64;
            let indent_twip = (depth as i32 + 1) * styles::pt_to_twip(18.0);

            let mut para = Paragraph::new().indent(Some(indent_twip), None, None, None);
            let prefix = format!("{}. ", num);
            let prefix_run = self.make_run(&prefix, InlineStyle::Body);
            para = para.add_run(prefix_run);

            for inline in &item.content {
                para = self.add_inline_to_paragraph(para, inline, false, InlineStyle::Body);
            }
            d = d.add_paragraph(para);

            // ネストされたブロック
            for child in &item.children {
                match child {
                    Block::OrderedList {
                        items: nested_items,
                        start,
                    } => {
                        d = self.convert_ordered_list(d, nested_items, *start, depth + 1)?;
                    }
                    Block::BulletList {
                        items: nested_items,
                    } => {
                        d = self.convert_bullet_list(d, nested_items, depth + 1)?;
                    }
                    _ => {
                        d = self.convert_block(d, child)?;
                    }
                }
            }
        }
        Ok(d)
    }

    fn convert_table(
        &mut self,
        docx: Docx,
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        alignments: &[crate::ir::Alignment],
    ) -> Docx {
        // 表番号キャプション
        let caption_fonts = RunFonts::new()
            .ascii(&self.config.fonts.heading_en)
            .hi_ansi(&self.config.fonts.heading_en)
            .east_asia(&self.config.fonts.heading_ja)
            .cs(&self.config.fonts.heading_en);
        let body_size = styles::pt_to_half_point(self.config.sizes.body);

        let table_number = self.next_table_number();

        let caption_para = match self.config.numbering.table_format.as_str() {
            "chapter" => {
                // 章番号モード: "表X.Y" をプレーンテキストで生成
                let label_run = Run::new()
                    .add_text(format!("表{}", table_number))
                    .size(body_size)
                    .bold()
                    .fonts(caption_fonts);
                Paragraph::new()
                    .add_run(label_run)
                    .align(AlignmentType::Center)
            }
            _ => {
                // 連番モード: Word SEQ フィールドを使用
                let label_run = Run::new()
                    .add_text("表")
                    .size(body_size)
                    .bold()
                    .fonts(caption_fonts.clone());
                let seq_run = Run::new()
                    .add_field_char(FieldCharType::Begin, true)
                    .add_instr_text(InstrText::Unsupported(" SEQ Table \\* ARABIC ".to_string()))
                    .add_field_char(FieldCharType::Separate, false)
                    .add_text(&table_number)
                    .add_field_char(FieldCharType::End, false)
                    .size(body_size)
                    .bold()
                    .fonts(caption_fonts);
                Paragraph::new()
                    .add_run(label_run)
                    .add_run(seq_run)
                    .align(AlignmentType::Center)
            }
        };

        let docx = docx.add_paragraph(caption_para);
        let column_count = headers
            .len()
            .max(rows.iter().map(|row| row.len()).max().unwrap_or(0));
        if column_count == 0 {
            return docx;
        }
        let column_widths = build_table_grid(column_count, &self.config.page);

        // ヘッダー行
        let header_cells: Vec<TableCell> = headers
            .iter()
            .enumerate()
            .map(|(index, cell_content)| {
                let th_color = self
                    .css_rules
                    .and_then(|r| r.table_th.as_ref())
                    .and_then(|s| s.color.clone());
                let mut para = Paragraph::new().align(AlignmentType::Center);
                for inline in cell_content {
                    para =
                        self.add_inline_to_paragraph(para, inline, true, InlineStyle::TableHeader);
                }
                // ヘッダーはゴシック体・太字
                let fonts = RunFonts::new()
                    .ascii(&self.config.fonts.heading_en)
                    .hi_ansi(&self.config.fonts.heading_en)
                    .east_asia(&self.config.fonts.heading_ja)
                    .cs(&self.config.fonts.heading_en);
                para = para.fonts(fonts).bold();
                // CSS: table th のフォント色を各 Run に適用
                if let Some(ref color) = th_color {
                    para.children = para
                        .children
                        .into_iter()
                        .map(|child| match child {
                            ParagraphChild::Run(mut run) => {
                                run.run_property.color = Some(Color::new(color));
                                ParagraphChild::Run(run)
                            }
                            other => other,
                        })
                        .collect();
                }
                let mut cell = TableCell::new();
                TableCell::new()
                    .width(column_widths[index], WidthType::Dxa)
                    .vertical_align(VAlignType::Center);

                // CSS: table th の背景色を適用
                if let Some(bg) = self
                    .css_rules
                    .and_then(|r| r.table_th.as_ref())
                    .and_then(|s| s.background_color.as_ref())
                {
                    cell = cell.shading(Shading::new().fill(bg));
                }

                cell.add_paragraph(para)
            })
            .collect();

        let header_row = TableRow::new(header_cells).cant_split();

        // データ行
        let mut table_rows = vec![header_row];
        for row in rows {
            let cells: Vec<TableCell> = row
                .iter()
                .enumerate()
                .map(|(index, cell_content)| {
                    let mut para = Paragraph::new().align(table_alignment_to_paragraph_alignment(
                        alignments.get(index),
                    ));
                    for inline in cell_content {
                        para = self.add_inline_to_paragraph(
                            para,
                            inline,
                            false,
                            InlineStyle::TableBody,
                        );
                    }
                    TableCell::new()
                        .width(column_widths[index], WidthType::Dxa)
                        .vertical_align(VAlignType::Center)
                        .add_paragraph(para)
                })
                .collect();
            table_rows.push(TableRow::new(cells).cant_split());
        }

        let table = Table::new(table_rows)
            .align(TableAlignmentType::Center)
            .layout(TableLayoutType::Fixed)
            .width(TABLE_WIDTH_PCT, WidthType::Pct)
            .set_grid(column_widths)
            .margins(TableCellMargins::new().margin(
                TABLE_CELL_PADDING_TWIP,
                TABLE_CELL_PADDING_TWIP,
                TABLE_CELL_PADDING_TWIP,
                TABLE_CELL_PADDING_TWIP,
            ));
        docx.add_table(table)
    }

    fn convert_code_block(&self, docx: Docx, _lang: Option<&str>, code: &str) -> Docx {
        let css_code = self.css_rules.and_then(|r| r.code_block.as_ref());

        // CSSからフォント/サイズを取得、未指定時はデフォルト
        let font_family = css_code
            .and_then(|c| c.font_family.as_deref())
            .unwrap_or("Courier New");
        let font_size_pt = css_code.and_then(|c| c.font_size_pt).unwrap_or(9.0);
        let east_asia_font = if font_family == "Courier New" {
            "ＭＳ ゴシック"
        } else {
            font_family
        };

        let fonts = RunFonts::new()
            .ascii(font_family)
            .hi_ansi(font_family)
            .east_asia(east_asia_font)
            .cs(font_family);

        let mut d = docx;
        let use_border = self.config.code_block.border;
        let lines: Vec<&str> = code.lines().collect();
        let n = lines.len();
        for (i, line) in lines.iter().enumerate() {
            let run = Run::new()
                .add_text(*line)
                .size(styles::pt_to_half_point(font_size_pt as f64))
                .fonts(fonts.clone());

            let mut para = Paragraph::new().add_run(run);
            if let Some(css) = css_code {
                para = apply_css_to_paragraph(para, css);
            }
            if use_border {
                para.property = para
                    .property
                    .set_borders(code_block_borders(i == 0, i == n - 1));
            }
            d = d.add_paragraph(para);
        }
        d
    }

    fn convert_image(&mut self, docx: Docx, alt: &str, path: &str) -> Docx {
        let image_path = self.base_path.join(path);

        let buf = match std::fs::read(&image_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "警告: 画像ファイルを読み込めません: {} ({})",
                    image_path.display(),
                    e
                );
                // 画像が見つからない場合はaltテキストのみ表示
                let run = self.make_run(&format!("[画像: {}]", alt), InlineStyle::Body);
                return docx.add_paragraph(Paragraph::new().add_run(run));
            }
        };

        // 画像をPNGに変換しつつ寸法を取得する
        let (png_buf, width_px, height_px) = match convert_to_png_with_dimensions(&buf) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("警告: 画像の変換に失敗しました: {} ({})", path, e);
                let run = self.make_run(&format!("[画像: {}]", alt), InlineStyle::Body);
                return docx.add_paragraph(Paragraph::new().add_run(run));
            }
        };

        let (width_emu, height_emu) = fit_image_to_body(width_px, height_px, &self.config.page);
        let pic = Pic::new(&png_buf).size(width_emu, height_emu);

        let image_para = Paragraph::new()
            .add_run(Run::new().add_image(pic))
            .align(AlignmentType::Center);

        let docx = docx.add_paragraph(image_para);
        self.add_figure_caption(docx, alt)
    }

    /// 図番号キャプションを追加する共通メソッド
    fn add_figure_caption(&mut self, docx: Docx, alt: &str) -> Docx {
        let caption_fonts = RunFonts::new()
            .ascii(&self.config.fonts.body_en)
            .hi_ansi(&self.config.fonts.body_en)
            .east_asia(&self.config.fonts.body_ja)
            .cs(&self.config.fonts.body_en);
        let body_size = styles::pt_to_half_point(self.config.sizes.body);

        let figure_number = self.next_figure_number();

        let mut caption_para = match self.config.numbering.figure_format.as_str() {
            "chapter" => {
                let label_run = Run::new()
                    .add_text(format!("図{}", figure_number))
                    .size(body_size)
                    .fonts(caption_fonts.clone());
                Paragraph::new()
                    .add_run(label_run)
                    .align(AlignmentType::Center)
            }
            _ => {
                let label_run = Run::new()
                    .add_text("図")
                    .size(body_size)
                    .fonts(caption_fonts.clone());
                let seq_run = Run::new()
                    .add_field_char(FieldCharType::Begin, true)
                    .add_instr_text(InstrText::Unsupported(
                        " SEQ Figure \\* ARABIC ".to_string(),
                    ))
                    .add_field_char(FieldCharType::Separate, false)
                    .add_text(&figure_number)
                    .add_field_char(FieldCharType::End, false)
                    .size(body_size)
                    .fonts(caption_fonts.clone());
                Paragraph::new()
                    .add_run(label_run)
                    .add_run(seq_run)
                    .align(AlignmentType::Center)
            }
        };

        if !alt.is_empty() {
            let alt_run = Run::new()
                .add_text(format!(" {}", alt))
                .size(body_size)
                .fonts(caption_fonts);
            caption_para = caption_para.add_run(alt_run);
        }

        docx.add_paragraph(caption_para)
    }
}

/// Docx の children 数を返す
fn count_paragraphs(docx: &Docx) -> usize {
    docx.document.children.len()
}

/// CSSスタイルを段落に適用するヘルパー
fn apply_css_to_paragraph(mut para: Paragraph, css: &CssStyle) -> Paragraph {
    // background-color → shading
    if let Some(ref color) = css.background_color {
        para.property = para.property.shading(Shading::new().fill(color));
    }
    // border
    let has_border = css.border_top.is_some()
        || css.border_bottom.is_some()
        || css.border_left.is_some()
        || css.border_right.is_some();
    if has_border {
        let mut borders = ParagraphBorders::with_empty();
        if let Some(ref b) = css.border_top {
            borders = borders.set(styles::css_border_to_docx(b, ParagraphBorderPosition::Top));
        }
        if let Some(ref b) = css.border_bottom {
            borders = borders.set(styles::css_border_to_docx(
                b,
                ParagraphBorderPosition::Bottom,
            ));
        }
        if let Some(ref b) = css.border_left {
            borders = borders.set(styles::css_border_to_docx(b, ParagraphBorderPosition::Left));
        }
        if let Some(ref b) = css.border_right {
            borders = borders.set(styles::css_border_to_docx(
                b,
                ParagraphBorderPosition::Right,
            ));
        }
        para.property = para.property.set_borders(borders);
    }
    // color → 全Runに適用
    if let Some(ref color) = css.color {
        para.children = para
            .children
            .into_iter()
            .map(|child| match child {
                ParagraphChild::Run(mut run) => {
                    run.run_property.color = Some(Color::new(color));
                    ParagraphChild::Run(run)
                }
                other => other,
            })
            .collect();
    }
    // font-family → 全Runに適用
    if let Some(ref family) = css.font_family {
        let fonts = RunFonts::new()
            .ascii(family)
            .hi_ansi(family)
            .east_asia(family)
            .cs(family);
        para.children = para
            .children
            .into_iter()
            .map(|child| match child {
                ParagraphChild::Run(mut run) => {
                    run.run_property.fonts = Some(fonts.clone());
                    ParagraphChild::Run(run)
                }
                other => other,
            })
            .collect();
    }
    // font-size → 全Runに適用
    if let Some(pt) = css.font_size_pt {
        let half_pt = styles::pt_to_half_point(pt as f64);
        para.children = para
            .children
            .into_iter()
            .map(|child| match child {
                ParagraphChild::Run(mut run) => {
                    run.run_property.sz = Some(Sz::new(half_pt));
                    run.run_property.sz_cs = Some(SzCs::new(half_pt));
                    ParagraphChild::Run(run)
                }
                other => other,
            })
            .collect();
    }
    // italic → 全Runに適用
    if let Some(true) = css.italic {
        para.children = para
            .children
            .into_iter()
            .map(|child| match child {
                ParagraphChild::Run(mut run) => {
                    run.run_property.italic = Some(Italic::new());
                    run.run_property.italic_cs = Some(ItalicCs::new());
                    ParagraphChild::Run(run)
                }
                other => other,
            })
            .collect();
    }
    // padding → indent (left/right) + spacing (top/bottom)
    let pad_left = css.padding_left_pt.map(|pt| styles::pt_to_twip(pt as f64));
    let pad_right = css.padding_right_pt.map(|pt| styles::pt_to_twip(pt as f64));
    if pad_left.is_some() || pad_right.is_some() {
        para = para.indent(pad_left, None, pad_right, None);
    }
    if css.padding_top_pt.is_some() || css.padding_bottom_pt.is_some() {
        let mut ls = LineSpacing::new();
        if let Some(pt) = css.padding_top_pt {
            ls = ls.before(styles::pt_to_twip(pt as f64) as u32);
        }
        if let Some(pt) = css.padding_bottom_pt {
            ls = ls.after(styles::pt_to_twip(pt as f64) as u32);
        }
        para.property = para.property.line_spacing(ls);
    }
    para
}

/// 画像データをPNG形式に変換し、元のピクセル寸法も返す
fn convert_to_png_with_dimensions(buf: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(buf)?;
    let (width, height) = img.dimensions();
    let mut png_buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_buf, image::ImageFormat::Png)?;
    Ok((png_buf.into_inner(), width, height))
}

fn fit_image_to_body(width_px: u32, height_px: u32, page: &PageConfig) -> (u32, u32) {
    let width_emu = width_px as u64 * EMU_PER_PIXEL;
    let height_emu = height_px as u64 * EMU_PER_PIXEL;
    let body_width_twip = page
        .width
        .saturating_sub(page.margin_left.max(0) as u32)
        .saturating_sub(page.margin_right.max(0) as u32) as u64;
    let max_width_emu = body_width_twip * EMU_PER_TWIP;

    // 幅でスケーリング
    let (mut w, mut h) = if width_emu > max_width_emu {
        let scaled_h = height_emu * max_width_emu / width_emu;
        (max_width_emu, scaled_h)
    } else {
        (width_emu, height_emu)
    };

    // 高さでスケーリング
    if let Some(max_h_twip) = page.image_max_height {
        let max_h_emu = max_h_twip as u64 * EMU_PER_TWIP;
        if h > max_h_emu {
            w = w * max_h_emu / h;
            h = max_h_emu;
        }
    }

    (w as u32, h as u32)
}

fn body_width_twip(page: &PageConfig) -> u32 {
    page.width
        .saturating_sub(page.margin_left.max(0) as u32)
        .saturating_sub(page.margin_right.max(0) as u32)
}

fn build_table_grid(column_count: usize, page: &PageConfig) -> Vec<usize> {
    let body_width = body_width_twip(page).max(column_count as u32);
    let base = body_width as usize / column_count;
    let remainder = body_width as usize % column_count;

    (0..column_count)
        .map(|index| base + usize::from(index < remainder))
        .collect()
}

fn table_alignment_to_paragraph_alignment(
    alignment: Option<&crate::ir::Alignment>,
) -> AlignmentType {
    match alignment {
        Some(crate::ir::Alignment::Center) => AlignmentType::Center,
        Some(crate::ir::Alignment::Right) => AlignmentType::Right,
        _ => AlignmentType::Left,
    }
}

/// 見出し inline 配列から先頭の番号プレフィックス（文字数指定）を除去する。
fn strip_prefix_from_inlines(content: &[Inline], prefix_chars: usize) -> Vec<Inline> {
    if prefix_chars == 0 {
        return content.to_vec();
    }

    let mut result = Vec::new();
    let mut remaining = prefix_chars;

    for inline in content {
        if remaining == 0 {
            result.push(inline.clone());
            continue;
        }

        match inline {
            Inline::Text(text) => {
                let char_count = text.chars().count();
                if remaining >= char_count {
                    remaining -= char_count;
                } else {
                    let new_text: String = text.chars().skip(remaining).collect();
                    remaining = 0;
                    let trimmed = new_text.trim_start().to_string();
                    if !trimmed.is_empty() {
                        result.push(Inline::Text(trimmed));
                    }
                }
            }
            _ => {
                remaining = 0;
                result.push(inline.clone());
            }
        }
    }

    result
}

/// テキスト処理: 英日間スペースの削除
fn process_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' && i > 0 && i + 1 < chars.len() {
            let prev = chars[i - 1];
            let next = chars[i + 1];
            // 英語→スペース→日本語 or 日本語→スペース→英語 のスペースを削除
            if (is_ascii_char(prev) && is_japanese_char(next))
                || (is_japanese_char(prev) && is_ascii_char(next))
            {
                i += 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn is_ascii_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c.is_ascii_punctuation()
}

fn is_japanese_char(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{309F}' | // ひらがな
        '\u{30A0}'..='\u{30FF}' | // カタカナ
        '\u{4E00}'..='\u{9FFF}' | // CJK統合漢字
        '\u{3400}'..='\u{4DBF}' | // CJK統合漢字拡張A
        '\u{FF00}'..='\u{FFEF}' | // 全角文字
        '\u{3000}'..='\u{303F}'   // CJK記号
    )
}

/// コードブロック用の段落罫線を生成する。
/// 複数行のコードブロック全体を一つの枠で囲むため、先頭/末尾で設定を変える。
/// - 先頭行: 上・左・右の罫線 ＋ Between（次行との間を繋ぐ）
/// - 中間行: 左・右の罫線 ＋ Between
/// - 末尾行: 下・左・右の罫線（Between なし）
/// - 1行のみ: 上下左右すべて
fn code_block_borders(is_first: bool, is_last: bool) -> ParagraphBorders {
    let make = |pos| ParagraphBorder::new(pos).size(4).space(4);
    let mut pb = ParagraphBorders::with_empty();
    pb = pb.set(make(ParagraphBorderPosition::Left));
    pb = pb.set(make(ParagraphBorderPosition::Right));
    if is_first {
        pb = pb.set(make(ParagraphBorderPosition::Top));
    }
    if is_last {
        pb = pb.set(make(ParagraphBorderPosition::Bottom));
    }
    pb
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::{DocumentChild, HyperlinkData, ParagraphChild, RunChild};

    #[test]
    fn converts_inline_link_to_word_hyperlink() {
        let blocks = vec![Block::Paragraph {
            content: vec![Inline::Link {
                text: vec![Inline::Text("Rust".to_string())],
                url: "https://www.rust-lang.org/".to_string(),
            }],
        }];

        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let para = match &docx.document.children[0] {
            DocumentChild::Paragraph(p) => p,
            other => panic!("unexpected child: {other:?}"),
        };

        let hyperlink = para
            .children
            .iter()
            .find_map(|child| match child {
                ParagraphChild::Hyperlink(link) => Some(link),
                _ => None,
            })
            .expect("hyperlink should exist");

        match &hyperlink.link {
            HyperlinkData::External { path, .. } => {
                assert_eq!(path, "https://www.rust-lang.org/");
            }
            other => panic!("unexpected hyperlink type: {other:?}"),
        }

        let link_text = hyperlink
            .children
            .iter()
            .find_map(|child| match child {
                ParagraphChild::Run(run) => {
                    run.children.iter().find_map(|run_child| match run_child {
                        RunChild::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("hyperlink text should exist");
        assert_eq!(link_text, "Rust");
    }

    #[test]
    fn inserts_space_for_soft_break() {
        let blocks = vec![Block::Paragraph {
            content: vec![
                Inline::Text("foo".to_string()),
                Inline::SoftBreak,
                Inline::Text("bar".to_string()),
            ],
        }];

        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let para = match &docx.document.children[0] {
            DocumentChild::Paragraph(p) => p,
            other => panic!("unexpected child: {other:?}"),
        };

        let mut joined = String::new();
        for child in &para.children {
            if let ParagraphChild::Run(run) = child {
                for run_child in &run.children {
                    if let RunChild::Text(t) = run_child {
                        joined.push_str(&t.text);
                    }
                }
            }
        }

        assert_eq!(joined, "foo bar");
    }

    #[test]
    fn converts_page_break_block_to_word_page_break() {
        let docx = convert_to_docx(
            &[Block::PageBreak],
            &Config::default(),
            None,
            Path::new("."),
        )
        .unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();
        assert!(xml.contains(r#"<w:br w:type="page" />"#));
    }

    #[test]
    fn indents_nested_ordered_lists_by_depth() {
        let nested = Block::OrderedList {
            start: 1,
            items: vec![ListItem {
                content: vec![Inline::Text("outer".to_string())],
                children: vec![Block::OrderedList {
                    start: 1,
                    items: vec![ListItem {
                        content: vec![Inline::Text("inner".to_string())],
                        children: vec![],
                    }],
                }],
            }],
        };

        let docx = convert_to_docx(&[nested], &Config::default(), None, Path::new(".")).unwrap();
        let indents: Vec<Option<i32>> = docx
            .document
            .children
            .iter()
            .filter_map(|child| match child {
                DocumentChild::Paragraph(p) => {
                    Some(p.property.indent.as_ref().and_then(|i| i.start))
                }
                _ => None,
            })
            .collect();

        assert_eq!(indents.len(), 2);
        assert_eq!(indents[0], Some(360));
        assert_eq!(indents[1], Some(720));
    }

    #[test]
    fn shrinks_wide_images_to_body_width() {
        let (width_emu, height_emu) = fit_image_to_body(2532, 729, &Config::default().page);
        assert_eq!(width_emu, 5_400_040);
        assert!(height_emu < width_emu);
    }

    #[test]
    fn keeps_small_images_original_size() {
        let (width_emu, height_emu) = fit_image_to_body(382, 376, &Config::default().page);
        assert_eq!(width_emu, 3_638_550);
        assert_eq!(height_emu, 3_581_400);
    }

    #[test]
    fn uses_configured_page_width_for_image_scaling() {
        let mut config = Config::default();
        config.page.width = 8_000;
        config.page.margin_left = 1_000;
        config.page.margin_right = 1_000;

        let (width_emu, height_emu) = fit_image_to_body(2532, 729, &config.page);
        assert_eq!(width_emu, 3_810_000);
        assert_eq!(height_emu, 1_096_954);
    }

    #[test]
    fn limits_tall_image_by_max_height() {
        let mut config = Config::default();
        // 高さ上限を5000twipに設定
        config.page.image_max_height = Some(5_000);

        // 縦長画像: 200x800px
        let (width_emu, height_emu) = fit_image_to_body(200, 800, &config.page);
        let max_h_emu = 5_000u64 * EMU_PER_TWIP;
        assert_eq!(height_emu, max_h_emu as u32);
        // アスペクト比が維持されていること
        assert!(width_emu < 200 * EMU_PER_PIXEL as u32);
    }

    #[test]
    fn no_height_limit_when_none() {
        let config = Config::default();
        // image_max_height が None なら高さ制限なし
        let (_, height_emu) = fit_image_to_body(200, 800, &config.page);
        assert_eq!(height_emu, (800u64 * EMU_PER_PIXEL) as u32);
    }

    #[test]
    fn makes_table_full_width_with_padding_and_centered_headers() {
        let blocks = vec![Block::Table {
            headers: vec![
                vec![Inline::Text("H1".to_string())],
                vec![Inline::Text("H2".to_string())],
            ],
            rows: vec![vec![
                vec![Inline::Text("L".to_string())],
                vec![Inline::Text("R".to_string())],
            ]],
            alignments: vec![crate::ir::Alignment::Left, crate::ir::Alignment::Right],
        }];

        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        assert!(xml.contains(r#"<w:tblW w:w="5000" w:type="pct" />"#));
        assert!(xml.contains(r#"<w:tblLayout w:type="fixed" />"#));
        assert!(xml.contains(r#"<w:tblCellMar><w:top w:w="80" w:type="dxa" /><w:left w:w="80" w:type="dxa" /><w:bottom w:w="80" w:type="dxa" /><w:right w:w="80" w:type="dxa" /></w:tblCellMar>"#));
        assert!(xml.contains(
            r#"<w:gridCol w:w="4252" w:type="dxa" /><w:gridCol w:w="4252" w:type="dxa" />"#
        ));
        assert!(xml.contains(r#"<w:jc w:val="center" />"#));
        assert!(xml.contains(r#"<w:jc w:val="right" />"#));
        assert!(xml.contains(r#"<w:sz w:val="19" />"#));
    }

    #[test]
    fn uses_configured_table_font_sizes() {
        let blocks = vec![Block::Table {
            headers: vec![vec![Inline::Text("Header".to_string())]],
            rows: vec![vec![vec![Inline::Text("Body".to_string())]]],
            alignments: vec![crate::ir::Alignment::Left],
        }];

        let mut config = Config::default();
        config.sizes.table_header = 8.5;
        config.sizes.table_body = 8.0;

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        assert!(xml.contains(r#"<w:t xml:space="preserve">Header</w:t>"#));
        assert!(xml.contains(r#"<w:t xml:space="preserve">Body</w:t>"#));
        assert!(xml.contains(r#"<w:sz w:val="17" />"#));
        assert!(xml.contains(r#"<w:sz w:val="16" />"#));
    }

    #[test]
    fn heading_css_background_color_produces_paragraph_shading() {
        let css = crate::css::parse_css("h1 { background-color: #F0F0F0; }");
        let blocks = vec![Block::Heading {
            level: 1,
            content: vec![Inline::Text("Title".to_string())],
        }];

        let docx =
            convert_to_docx(&blocks, &Config::default(), Some(&css), Path::new(".")).unwrap();

        // Check style definition XML contains paragraph-level shading
        let styles_xml = String::from_utf8(docx.styles.build()).unwrap();
        assert!(
            styles_xml.contains(r#"<w:shd w:val="clear" w:color="auto" w:fill="F0F0F0" />"#),
            "paragraph shading not found in styles XML"
        );
    }

    #[test]
    fn heading_css_border_produces_paragraph_borders() {
        let css = crate::css::parse_css("h1 { border: 2px double #000000; }");
        let blocks = vec![Block::Heading {
            level: 1,
            content: vec![Inline::Text("Title".to_string())],
        }];

        let docx =
            convert_to_docx(&blocks, &Config::default(), Some(&css), Path::new(".")).unwrap();

        let styles_xml = String::from_utf8(docx.styles.build()).unwrap();
        // double border, size=16 (2px * 8), color=000000
        assert!(
            styles_xml.contains(r#"w:val="double"#),
            "double border type not found in styles XML"
        );
        assert!(
            styles_xml.contains(r#"w:sz="16"#),
            "border size not found in styles XML"
        );
        assert!(
            styles_xml.contains(r#"w:color="000000"#),
            "border color not found in styles XML"
        );
    }

    #[test]
    fn blockquote_default_style_applies_border_and_color() {
        let blocks = vec![Block::BlockQuote {
            children: vec![Block::Paragraph {
                content: vec![Inline::Text("quoted".to_string())],
            }],
        }];

        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // デフォルトのblockquoteスタイル: 左ボーダー(CCCCCC), Run色(666666)
        // Run色は <w:color w:val="666666" /> 形式
        assert!(
            xml.contains(r#"w:val="666666"#),
            "blockquote default run color not found in: {}",
            xml
        );
        // ボーダー色は属性 w:color="CCCCCC"
        assert!(
            xml.contains(r#"w:color="CCCCCC"#),
            "blockquote default border color not found"
        );
    }

    #[test]
    fn blockquote_css_overrides_default() {
        let css =
            crate::css::parse_css("blockquote { color: #333333; border-left: 5px solid #ff0000; }");
        let blocks = vec![Block::BlockQuote {
            children: vec![Block::Paragraph {
                content: vec![Inline::Text("quoted".to_string())],
            }],
        }];

        let docx =
            convert_to_docx(&blocks, &Config::default(), Some(&css), Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // Run色は <w:color w:val="333333" /> 形式
        assert!(
            xml.contains(r#"w:val="333333"#),
            "CSS blockquote run color not applied"
        );
        // ボーダー色は属性 w:color="FF0000"
        assert!(
            xml.contains(r#"w:color="FF0000"#),
            "CSS blockquote border color not applied"
        );
    }

    #[test]
    fn code_block_default_without_css() {
        let blocks = vec![Block::CodeBlock {
            lang: None,
            code: "let x = 1;".to_string(),
        }];

        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // デフォルト: Courier New, 9pt = sz 18
        assert!(xml.contains("Courier New"), "default code font not found");
        assert!(
            xml.contains(r#"w:val="18"#),
            "default code size (18 half-pt = 9pt) not found"
        );
    }

    #[test]
    fn code_block_css_applies_background() {
        let css = crate::css::parse_css("pre { background-color: #f5f5f5; font-size: 8pt; }");
        let blocks = vec![Block::CodeBlock {
            lang: Some("rust".to_string()),
            code: "fn main() {}".to_string(),
        }];

        let docx =
            convert_to_docx(&blocks, &Config::default(), Some(&css), Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        assert!(
            xml.contains(r#"w:fill="F5F5F5"#),
            "code block CSS background not applied"
        );
        // 8pt = sz 16
        assert!(
            xml.contains(r#"w:val="16"#),
            "code block CSS font size not applied"
        );
    }

    #[test]
    fn styled_div_applies_css_to_children() {
        let css = crate::css::parse_css(".todo { background-color: #fff3cd; color: #856404; }");
        let blocks = vec![Block::StyledDiv {
            class: "todo".to_string(),
            children: vec![Block::Paragraph {
                content: vec![Inline::Text("task".to_string())],
            }],
        }];

        let docx =
            convert_to_docx(&blocks, &Config::default(), Some(&css), Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        assert!(
            xml.contains(r#"w:fill="FFF3CD"#),
            "div CSS background not applied"
        );
        // Run色は <w:color w:val="856404" /> 形式
        assert!(
            xml.contains(r#"w:val="856404"#),
            "div CSS color not applied"
        );
    }

    #[test]
    fn styled_div_without_css_falls_through() {
        let blocks = vec![Block::StyledDiv {
            class: "unknown".to_string(),
            children: vec![Block::Paragraph {
                content: vec![Inline::Text("text".to_string())],
            }],
        }];

        // CSSなしでもpanicしないこと
        let docx = convert_to_docx(&blocks, &Config::default(), None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();
        assert!(xml.contains("text"), "text should still appear");
    }

    #[test]
    fn spacing_config_applies_line_spacing() {
        let mut config = Config::default();
        config.spacing.line = Some(18.0);
        config.spacing.before = Some(6.0);
        config.spacing.after = Some(6.0);

        let blocks = vec![Block::Paragraph {
            content: vec![Inline::Text("test".to_string())],
        }];

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let styles_xml = String::from_utf8(docx.styles.build()).unwrap();

        // 18pt = 360 twips
        assert!(
            styles_xml.contains(r#"w:line="360"#),
            "line spacing not found in styles"
        );
        // 6pt = 120 twips
        assert!(
            styles_xml.contains(r#"w:before="120"#),
            "before spacing not found in styles"
        );
        assert!(
            styles_xml.contains(r#"w:after="120"#),
            "after spacing not found in styles"
        );
    }

    #[test]
    fn heading_numbering_default_true() {
        let config = Config::default();
        assert!(config.numbering.heading_numbering);
    }

    #[test]
    fn toc_default_disabled() {
        let config = Config::default();
        assert!(!config.toc.enable);
        assert_eq!(config.toc.min_level, 1);
        assert_eq!(config.toc.max_level, 3);
    }

    #[test]
    fn heading_numbering_disabled_skips_numbering() {
        let mut config = Config::default();
        config.numbering.heading_numbering = false;

        let blocks = vec![Block::Heading {
            level: 1,
            content: vec![Inline::Text("Test".to_string())],
        }];

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // numbering 無効時は numId が段落プロパティに含まれないことを確認
        assert!(
            !xml.contains("w:numId"),
            "numId should not appear in document when heading_numbering is disabled"
        );
    }

    #[test]
    fn toc_enabled_adds_table_of_contents() {
        let mut config = Config::default();
        config.toc.enable = true;
        config.toc.min_level = 1;
        config.toc.max_level = 3;

        let blocks = vec![Block::Heading {
            level: 1,
            content: vec![Inline::Text("Test".to_string())],
        }];

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // TOC フィールドが含まれることを確認
        assert!(
            xml.contains("TOC"),
            "TOC field should be present when toc is enabled"
        );
    }

    #[test]
    fn toc_heading_range_respected() {
        let mut config = Config::default();
        config.toc.enable = true;
        config.toc.min_level = 2;
        config.toc.max_level = 4;

        let blocks = vec![Block::Heading {
            level: 2,
            content: vec![Inline::Text("Test".to_string())],
        }];

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // \o スイッチに "2-4" が反映されることを確認
        assert!(
            xml.contains(r#"\o &quot;2-4&quot;"#) || xml.contains(r#"\o "2-4""#),
            "TOC should contain \\o switch with range 2-4, got: {}",
            xml.chars().take(2000).collect::<String>()
        );
    }

    #[test]
    fn toc_disabled_by_default() {
        let config = Config::default();

        let blocks = vec![Block::Heading {
            level: 1,
            content: vec![Inline::Text("Test".to_string())],
        }];

        let docx = convert_to_docx(&blocks, &config, None, Path::new(".")).unwrap();
        let xml = String::from_utf8(docx.document.build()).unwrap();

        // デフォルトではTOCが出力されないことを確認
        assert!(!xml.contains("TOC "), "TOC should not appear by default");
    }
}
