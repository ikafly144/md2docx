use crate::config::Config;
use crate::css::{CssBorder, CssRules, CssStyle};
use docx_rs::*;

/// pt → half-point (Word内部単位) への変換
/// Word は半ポイント(half-point)単位でフォントサイズを管理する
pub fn pt_to_half_point(pt: f64) -> usize {
    (pt * 2.0) as usize
}

/// pt → twip (1/20 pt) への変換
/// 段落の間隔などに使用
pub fn pt_to_twip(pt: f64) -> i32 {
    (pt * 20.0) as i32
}

/// 本文スタイルの styleId
pub const BODY_TEXT_STYLE_ID: &str = "13";

/// 見出し番号の numId (numbering.xml の num 要素 ID)
pub const HEADING_NUM_ID: usize = 2;
/// 見出し番号の abstractNumId
const HEADING_ABSTRACT_NUM_ID: usize = 8;

/// 箇条書きの numId (numbering.xml の num 要素 ID)
pub const BULLET_NUM_ID: usize = 3;
/// 箇条書きの abstractNumId
const BULLET_ABSTRACT_NUM_ID: usize = 9;
/// 箇条書きスタイルの styleId
pub const BULLET_STYLE_ID: &str = "BulletList";

/// 脚注の numId (numbering.xml の num 要素 ID)
pub const FOOTNOTE_NUM_ID: usize = 4;
/// 脚注の abstractNumId
const FOOTNOTE_ABSTRACT_NUM_ID: usize = 10;

/// 表題スタイルの styleId（Word 標準「表題」= "Title"）
pub const TITLE_STYLE_ID: &str = "Title";

const HEADING1_BEFORE_PT: f64 = 24.0;
const HEADING1_AFTER_PT: f64 = 12.0;
const HEADING2_BEFORE_PT: f64 = 18.0;
const HEADING2_AFTER_PT: f64 = 8.0;

/// sample.docx のスタイル定義を Docx に適用する
///
/// - docDefaults: minorHAnsi/minorEastAsia テーマ、sz=21 (10.5pt)
/// - Normal スタイル: id="a", jc=both
/// - Heading1-4: テーマフォント、サイズ、bold、keepNext、outlineLvl
/// - AbstractNumbering (id=8): 見出し番号 Level 0-3
/// - Numbering (id=2): abstractNumId=8
pub fn setup_document_styles(docx: Docx, config: &Config, css_rules: Option<&CssRules>) -> Docx {
    // --- docDefaults ---
    // テーマファイルを生成できないため、実フォント名を直接指定
    let default_fonts = RunFonts::new()
        .ascii(&config.fonts.body_en)
        .hi_ansi(&config.fonts.body_en)
        .east_asia(&config.fonts.body_ja)
        .cs(&config.fonts.body_en);

    let docx = docx
        .default_size(pt_to_half_point(config.sizes.body)) // 10.5pt = sz 21
        .default_fonts(default_fonts);

    // --- Normal スタイル ---
    // docx-rs は空の styleId="Normal" を自動生成するため、
    // 同じ ID で上書きする（後勝ち）。styleId="a" は使わない。
    let normal_fonts = RunFonts::new()
        .ascii(&config.fonts.body_en)
        .hi_ansi(&config.fonts.body_en)
        .east_asia(&config.fonts.body_ja)
        .cs(&config.fonts.body_en);

    let normal_style = Style::new("Normal", StyleType::Paragraph)
        .name("Normal")
        .fonts(normal_fonts)
        .size(pt_to_half_point(config.sizes.body))
        .align(AlignmentType::Both);

    // --- 見出し1 (id="1") ---
    // basedOn=Normal("a"), next=Normal("a")
    // keepNext, outlineLvl=0
    // テーマフォント: majorHAnsi / majorEastAsia / majorBidi
    // 14pt (sz=28), bold
    let heading1_fonts = RunFonts::new()
        .ascii(&config.fonts.heading_en)
        .hi_ansi(&config.fonts.heading_en)
        .east_asia(&config.fonts.heading_ja)
        .cs(&config.fonts.heading_en);

    let mut heading1_style = Style::new("1", StyleType::Paragraph)
        .name("heading 1")
        .based_on("Normal")
        .next("Normal")
        .size(pt_to_half_point(config.sizes.heading1)) // 14pt = sz 28
        .bold()
        .fonts(heading1_fonts)
        .line_spacing(
            LineSpacing::new()
                .before(pt_to_twip(HEADING1_BEFORE_PT) as u32)
                .after(pt_to_twip(HEADING1_AFTER_PT) as u32),
        )
        .outline_lvl(0);
    let depth = config.numbering.heading_numbering_depth;
    // h1_title モードでは H1 に numbering を付けない（タイトル扱い）
    // heading_numbering_depth < 1 の場合も H1 に numbering を付けない
    // heading_numbering が false の場合は全見出しの numbering を無効化
    if config.numbering.heading_numbering && !config.numbering.h1_title && depth >= 1 {
        heading1_style.paragraph_property = heading1_style
            .paragraph_property
            .numbering_property(NumberingProperty::new().id(NumberingId::new(HEADING_NUM_ID)));
    }

    // --- 見出し2 (id="2") ---
    // basedOn=見出し1("1"), next=Normal("a")
    // outlineLvl=1
    // 12pt (sz=24)
    // フォントは見出し1から継承
    let mut heading2_style = Style::new("2", StyleType::Paragraph)
        .name("heading 2")
        .based_on("1")
        .next("Normal")
        .size(pt_to_half_point(config.sizes.heading2)) // 12pt = sz 24
        .line_spacing(
            LineSpacing::new()
                .before(pt_to_twip(HEADING2_BEFORE_PT) as u32)
                .after(pt_to_twip(HEADING2_AFTER_PT) as u32),
        )
        .outline_lvl(1);
    // h1_title モード: ilvl を 1 つ下げる (H2→0, 通常: H2→1)
    if config.numbering.heading_numbering && depth >= 2 {
        let ilvl = if config.numbering.h1_title { 0 } else { 1 };
        let mut np = NumberingProperty::new();
        if config.numbering.h1_title {
            // h1_title では heading1 に numId がないため、明示的に指定
            np = np.id(NumberingId::new(HEADING_NUM_ID));
        }
        np.level = Some(IndentLevel::new(ilvl));
        heading2_style.paragraph_property =
            heading2_style.paragraph_property.numbering_property(np);
    }

    // --- 見出し3 (id="3") ---
    // basedOn=Normal("a"), next=Normal("a")
    // keepNext, outlineLvl=2
    // テーマフォント: majorHAnsi / majorEastAsia / majorBidi
    // 11pt (sz=22), bold
    let heading3_fonts = RunFonts::new()
        .ascii(&config.fonts.heading_en)
        .hi_ansi(&config.fonts.heading_en)
        .east_asia(&config.fonts.heading_ja)
        .cs(&config.fonts.heading_en);

    let mut heading3_style = Style::new("3", StyleType::Paragraph)
        .name("heading 3")
        .based_on("Normal")
        .next("Normal")
        .size(pt_to_half_point(config.sizes.heading3)) // 11pt = sz 22
        .bold()
        .fonts(heading3_fonts)
        .outline_lvl(2);
    if config.numbering.heading_numbering && depth >= 3 {
        let ilvl = if config.numbering.h1_title { 1 } else { 2 };
        heading3_style.paragraph_property = heading3_style
            .paragraph_property
            .numbering(NumberingId::new(HEADING_NUM_ID), IndentLevel::new(ilvl));
    }

    // --- 見出し4 (id="4") ---
    // basedOn=Normal("a"), next=Normal("a")
    // keepNext, outlineLvl=3
    // テーマフォント: majorEastAsia のみ
    // 11pt (sz=22), bold
    // indent: left=709, hanging=709
    let heading4_fonts = RunFonts::new().east_asia(&config.fonts.heading_ja);

    let mut heading4_style = Style::new("4", StyleType::Paragraph)
        .name("heading 4")
        .based_on("Normal")
        .next("Normal")
        .size(pt_to_half_point(config.sizes.heading4)) // 11pt = sz 22
        .bold()
        .fonts(heading4_fonts)
        .indent(
            Some(config.indent.heading4_left),
            Some(SpecialIndentType::Hanging(config.indent.heading4_hanging)),
            None,
            None,
        )
        .outline_lvl(3);
    if config.numbering.heading_numbering && depth >= 4 {
        let ilvl = if config.numbering.h1_title { 2 } else { 3 };
        heading4_style.paragraph_property = heading4_style
            .paragraph_property
            .numbering(NumberingId::new(HEADING_NUM_ID), IndentLevel::new(ilvl));
    }

    // --- 見出し5 (id="5") ---
    // basedOn=Normal, next=Normal
    // keepNext, outlineLvl=4
    // heading4 と同パターン（East Asia フォントのみ指定）
    let heading5_fonts = RunFonts::new().east_asia(&config.fonts.heading_ja);

    let mut heading5_style = Style::new("5", StyleType::Paragraph)
        .name("heading 5")
        .based_on("Normal")
        .next("Normal")
        .size(pt_to_half_point(config.sizes.heading5))
        .bold()
        .fonts(heading5_fonts)
        .outline_lvl(4);
    if config.numbering.heading_numbering && depth >= 5 {
        let ilvl = if config.numbering.h1_title { 3 } else { 4 };
        heading5_style.paragraph_property = heading5_style
            .paragraph_property
            .numbering(NumberingId::new(HEADING_NUM_ID), IndentLevel::new(ilvl));
    }

    // --- 表題 (id="Title") ---
    // heading_shift = true 時に # に対応するスタイル
    // Word 標準「表題」に合わせ: 中央揃え、大フォント、番号なし
    let title_fonts = RunFonts::new()
        .ascii(&config.fonts.heading_en)
        .hi_ansi(&config.fonts.heading_en)
        .east_asia(&config.fonts.heading_ja)
        .cs(&config.fonts.heading_en);

    let title_style = Style::new(TITLE_STYLE_ID, StyleType::Paragraph)
        .name("Title")
        .based_on("Normal")
        .next("Normal")
        .size(pt_to_half_point(config.heading.title_size))
        .bold()
        .fonts(title_fonts)
        .align(AlignmentType::Center);

    // --- 見出し番号定義 (abstractNumId=8, numId=2) ---
    let mut abstract_numbering = AbstractNumbering::new(HEADING_ABSTRACT_NUM_ID)
        // Level 0: decimal, "%1.", indent left=420, hanging=420, pStyle="1"
        .add_level(
            Level::new(
                0,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%1."),
                LevelJc::new("left"),
            )
            .paragraph_style("1")
            .indent(
                Some(config.indent.heading1_left),
                Some(SpecialIndentType::Hanging(config.indent.heading1_hanging)),
                None,
                None,
            ),
        )
        // Level 1: decimal, "%1.%2.", indent left=612, hanging=612, pStyle="2"
        .add_level(
            Level::new(
                1,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%1.%2."),
                LevelJc::new("left"),
            )
            .paragraph_style("2")
            .indent(
                Some(config.indent.heading2_left),
                Some(SpecialIndentType::Hanging(config.indent.heading2_hanging)),
                None,
                None,
            ),
        )
        // Level 2: decimal, "%1.%2.%3", indent left=783, hanging=783, pStyle="3"
        .add_level(
            Level::new(
                2,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%1.%2.%3"),
                LevelJc::new("left"),
            )
            .paragraph_style("3")
            .indent(
                Some(config.indent.heading3_left),
                Some(SpecialIndentType::Hanging(config.indent.heading3_hanging)),
                None,
                None,
            ),
        )
        // Level 3: decimal, "（%4）", indent left=709, hanging=709, pStyle="4"
        // Style 4 のインデント定義に合わせる
        .add_level(
            Level::new(
                3,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("\u{FF08}%4\u{FF09}"),
                LevelJc::new("left"),
            )
            .paragraph_style("4")
            .indent(
                Some(config.indent.heading4_left),
                Some(SpecialIndentType::Hanging(config.indent.heading4_hanging)),
                None,
                None,
            ),
        )
        // Level 4: decimalEnclosedCircle（丸数字 ① ② ...）, pStyle="5"
        .add_level(
            Level::new(
                4,
                Start::new(1),
                NumberFormat::new("decimalEnclosedCircle"),
                LevelText::new("%5"),
                LevelJc::new("left"),
            )
            .paragraph_style("5")
            .indent(
                Some(config.indent.heading5_left),
                Some(SpecialIndentType::Hanging(config.indent.heading5_hanging)),
                None,
                None,
            ),
        )
        .add_level(
            Level::new(
                5,
                Start::new(1),
                NumberFormat::new("decimalEnclosedCircle"),
                LevelText::new("%6"),
                LevelJc::new("left"),
            )
            .indent(
                Some(config.indent.heading6_left),
                Some(SpecialIndentType::Hanging(config.indent.heading6_hanging)),
                None,
                None,
            ),
        )
        .add_level(
            Level::new(
                6,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%7."),
                LevelJc::new("left"),
            )
            .indent(
                Some(2940),
                Some(SpecialIndentType::Hanging(420)),
                None,
                None,
            ),
        )
        .add_level(
            Level::new(
                7,
                Start::new(1),
                NumberFormat::new("aiueoFullWidth"),
                LevelText::new("(%8)"),
                LevelJc::new("left"),
            )
            .indent(
                Some(3360),
                Some(SpecialIndentType::Hanging(420)),
                None,
                None,
            ),
        )
        .add_level(
            Level::new(
                8,
                Start::new(1),
                NumberFormat::new("decimalEnclosedCircle"),
                LevelText::new("%9"),
                LevelJc::new("left"),
            )
            .indent(
                Some(3780),
                Some(SpecialIndentType::Hanging(420)),
                None,
                None,
            ),
        );
    abstract_numbering.multi_level_type = Some("multilevel".to_string());

    let numbering = Numbering::new(HEADING_NUM_ID, HEADING_ABSTRACT_NUM_ID);

    // --- 本文ｰ見出しレベル1~3 (id="13") ---
    // sample.docx 準拠の本文スタイル（字下げ付き）
    // sample: leftChars=100/left=210, rightChars=100/right=100, firstLineChars=100/firstLine=100
    // docx-rs は rightChars, firstLineChars を出力できないため、
    // 絶対値を全角1文字幅 (210 twip = 2 × drawingGridHorizontalSpacing) に補正する
    let mut body_text_style = Style::new(BODY_TEXT_STYLE_ID, StyleType::Paragraph)
        .name("本文ｰ見出し")
        .based_on("Normal")
        .indent(
            Some(config.indent.body_left),
            Some(SpecialIndentType::FirstLine(config.indent.body_first_line)),
            Some(config.indent.body_right),
            Some(config.indent.body_left_chars),
        );

    // 行間設定
    if config.spacing.line.is_some()
        || config.spacing.before.is_some()
        || config.spacing.after.is_some()
    {
        let mut ls = LineSpacing::new();
        if let Some(line_pt) = config.spacing.line {
            let twips = pt_to_twip(line_pt);
            ls = ls.line(twips).line_rule(LineSpacingType::Exact);
        }
        if let Some(before_pt) = config.spacing.before {
            ls = ls.before(pt_to_twip(before_pt) as u32);
        }
        if let Some(after_pt) = config.spacing.after {
            ls = ls.after(pt_to_twip(after_pt) as u32);
        }
        body_text_style.paragraph_property =
            body_text_style.paragraph_property.clone().line_spacing(ls);
    }

    // --- 箇条書き用 Numbering 定義 (abstractNumId=9, numId=3) ---
    let bullet_chars = [
        &config.bullet.level0,
        &config.bullet.level1,
        &config.bullet.level2,
    ];

    let mut bullet_abstract = AbstractNumbering::new(BULLET_ABSTRACT_NUM_ID);
    for (i, ch) in bullet_chars.iter().enumerate() {
        let left = (i as i32 + 1) * 360; // 360, 720, 1080
        let hanging = 360;
        bullet_abstract = bullet_abstract.add_level(
            Level::new(
                i,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new(*ch),
                LevelJc::new("left"),
            )
            .indent(
                Some(left),
                Some(SpecialIndentType::Hanging(hanging)),
                None,
                None,
            ),
        );
    }

    let bullet_numbering = Numbering::new(BULLET_NUM_ID, BULLET_ABSTRACT_NUM_ID);

    let bullet_style = Style::new(BULLET_STYLE_ID, StyleType::Paragraph)
        .name("Bullet List")
        .based_on("Normal");

    // --- 脚注の番号付きリスト定義 (abstractNumId=10, numId=4) ---
    let footnote_abstract = AbstractNumbering::new(FOOTNOTE_ABSTRACT_NUM_ID)
        .add_level(
            Level::new(
                0,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%1."),
                LevelJc::new("left"),
            )
            .indent(
                Some(360),
                Some(SpecialIndentType::Hanging(360)),
                None,
                None,
            ),
        );

    let footnote_numbering = Numbering::new(FOOTNOTE_NUM_ID, FOOTNOTE_ABSTRACT_NUM_ID);

    // --- CSS による見出しスタイル上書き ---
    if let Some(css) = css_rules {
        apply_css_to_heading(&mut heading1_style, css.h1.as_ref(), "h1");
        apply_css_to_heading(&mut heading2_style, css.h2.as_ref(), "h2");
        apply_css_to_heading(&mut heading3_style, css.h3.as_ref(), "h3");
        apply_css_to_heading(&mut heading4_style, css.h4.as_ref(), "h4");
        apply_css_to_heading(&mut heading5_style, css.h5.as_ref(), "h5");
    }

    // --- 脚注の参照スタイル定義 ---
    let mut footnote_ref_style = Style::new("FootnoteReference", StyleType::Character)
        .name("footnote reference");
    footnote_ref_style.run_property = footnote_ref_style.run_property.vert_align(VertAlignType::SuperScript);

    let mut docx = docx
        .add_style(normal_style)
        .add_style(title_style)
        .add_style(body_text_style)
        .add_style(heading1_style)
        .add_style(heading2_style)
        .add_style(heading3_style)
        .add_style(heading4_style)
        .add_style(heading5_style)
        .add_style(bullet_style)
        .add_style(footnote_ref_style)
        .add_abstract_numbering(abstract_numbering)
        .add_abstract_numbering(bullet_abstract)
        .add_abstract_numbering(footnote_abstract)
        .add_numbering(numbering)
        .add_numbering(bullet_numbering)
        .add_numbering(footnote_numbering);

    // --- CSS classes → Character スタイル登録 ---
    if let Some(css) = css_rules {
        for (class_name, css_style) in &css.classes {
            let style_id = format!("css-{}", class_name);
            let mut style =
                Style::new(&style_id, StyleType::Character).name(&format!("CSS: {}", class_name));

            if let Some(ref color) = css_style.color {
                style = style.color(color);
            }
            if let Some(true) = css_style.bold {
                style = style.bold();
            }
            if let Some(true) = css_style.italic {
                style = style.italic();
            }
            if let Some(true) = css_style.underline {
                style = style.underline("single");
            }
            if let Some(ref family) = css_style.font_family {
                let fonts = RunFonts::new()
                    .ascii(family)
                    .hi_ansi(family)
                    .east_asia(family)
                    .cs(family);
                style = style.fonts(fonts);
            }
            if let Some(pt) = css_style.font_size_pt {
                style = style.size(pt_to_half_point(pt as f64));
            }

            docx = docx.add_style(style);
        }
    }

    docx
}

fn apply_css_to_heading(style: &mut Style, css: Option<&CssStyle>, _selector: &str) {
    let css = match css {
        Some(c) => c,
        None => return,
    };

    if let Some(pt) = css.font_size_pt {
        style.run_property.sz = Some(Sz::new(pt_to_half_point(pt as f64)));
        style.run_property.sz_cs = Some(SzCs::new(pt_to_half_point(pt as f64)));
    }
    if let Some(ref color) = css.color {
        style.run_property.color = Some(Color::new(color));
    }
    if let Some(bold) = css.bold {
        if bold {
            style.run_property.bold = Some(Bold::new());
            style.run_property.bold_cs = Some(BoldCs::new());
        } else {
            style.run_property.bold = None;
            style.run_property.bold_cs = None;
        }
    }
    if let Some(true) = css.italic {
        style.run_property.italic = Some(Italic::new());
        style.run_property.italic_cs = Some(ItalicCs::new());
    }
    if let Some(true) = css.underline {
        style.run_property.underline = Some(Underline::new("single"));
    }
    if let Some(ref family) = css.font_family {
        let fonts = RunFonts::new()
            .ascii(family)
            .hi_ansi(family)
            .east_asia(family)
            .cs(family);
        style.run_property.fonts = Some(fonts);
    }
    if let Some(lh) = css.line_height {
        let spacing = (lh * 20.0) as i32;
        style.paragraph_property = style
            .paragraph_property
            .clone()
            .line_spacing(
                LineSpacing::new()
                    .line(spacing)
                    .line_rule(LineSpacingType::Exact),
            )
            .text_alignment(TextAlignmentType::Center);
    }
    if let Some(ref color) = css.background_color {
        style.paragraph_property = style
            .paragraph_property
            .clone()
            .shading(Shading::new().fill(color));
    }
    // border support
    let has_border = css.border_top.is_some()
        || css.border_bottom.is_some()
        || css.border_left.is_some()
        || css.border_right.is_some();
    if has_border {
        let mut borders = ParagraphBorders::with_empty();
        if let Some(ref b) = css.border_top {
            borders = borders.set(css_border_to_docx(b, ParagraphBorderPosition::Top));
        }
        if let Some(ref b) = css.border_bottom {
            borders = borders.set(css_border_to_docx(b, ParagraphBorderPosition::Bottom));
        }
        if let Some(ref b) = css.border_left {
            borders = borders.set(css_border_to_docx(b, ParagraphBorderPosition::Left));
        }
        if let Some(ref b) = css.border_right {
            borders = borders.set(css_border_to_docx(b, ParagraphBorderPosition::Right));
        }
        style.paragraph_property = style.paragraph_property.clone().set_borders(borders);
    }
}

pub fn css_border_to_docx(css: &CssBorder, pos: ParagraphBorderPosition) -> ParagraphBorder {
    let border_type = match css.style.as_str() {
        "solid" => BorderType::Single,
        "double" => BorderType::Double,
        "dotted" => BorderType::Dotted,
        "dashed" => BorderType::Dashed,
        "none" => BorderType::Nil,
        _ => BorderType::Single,
    };
    let size = (css.size_px * 8.0) as usize;
    ParagraphBorder::new(pos)
        .val(border_type)
        .size(size)
        .color(&css.color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_styles_include_spacing_for_levels_one_and_two() {
        let xml = String::from_utf8(
            setup_document_styles(Docx::new(), &Config::default(), None)
                .build()
                .styles,
        )
        .unwrap();

        assert!(xml.contains(r#"<w:spacing w:before="480" w:after="240" />"#));
        assert!(xml.contains(r#"<w:spacing w:before="360" w:after="160" />"#));
    }

    #[test]
    fn heading_numberings_use_shallow_indent_for_levels_five_and_six() {
        let xml = String::from_utf8(
            setup_document_styles(Docx::new(), &Config::default(), None)
                .build()
                .numberings,
        )
        .unwrap();

        assert_eq!(xml.matches(r#"w:left="709""#).count(), 3);
        assert_eq!(xml.matches(r#"w:hanging="709""#).count(), 3);
    }

    #[test]
    fn heading_numberings_follow_configured_indents_for_levels_one_to_six() {
        let mut config = Config::default();
        config.indent.heading1_left = 401;
        config.indent.heading1_hanging = 402;
        config.indent.heading2_left = 501;
        config.indent.heading2_hanging = 502;
        config.indent.heading3_left = 601;
        config.indent.heading3_hanging = 602;
        config.indent.heading4_left = 701;
        config.indent.heading4_hanging = 702;
        config.indent.heading5_left = 801;
        config.indent.heading5_hanging = 802;
        config.indent.heading6_left = 901;
        config.indent.heading6_hanging = 902;

        let xml = String::from_utf8(
            setup_document_styles(Docx::new(), &config, None)
                .build()
                .numberings,
        )
        .unwrap();

        assert!(xml.contains(r#"w:left="401""#));
        assert!(xml.contains(r#"w:hanging="402""#));
        assert!(xml.contains(r#"w:left="501""#));
        assert!(xml.contains(r#"w:hanging="502""#));
        assert!(xml.contains(r#"w:left="601""#));
        assert!(xml.contains(r#"w:hanging="602""#));
        assert!(xml.contains(r#"w:left="701""#));
        assert!(xml.contains(r#"w:hanging="702""#));
        assert!(xml.contains(r#"w:left="801""#));
        assert!(xml.contains(r#"w:hanging="802""#));
        assert!(xml.contains(r#"w:left="901""#));
        assert!(xml.contains(r#"w:hanging="902""#));
    }
}
