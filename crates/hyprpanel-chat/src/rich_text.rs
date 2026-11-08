//! Markdown → HTML nettoyé → blocs natifs Slint. Aucun moteur JavaScript.

use html5ever::{parse_document, serialize, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use pulldown_cmark::{Options, Parser};
use slint::{ModelRc, StyledText, VecModel};

use crate::{MessageRow, RichBlockRow, RichCellRow, RichTableRow};

#[derive(Clone, Debug, Default)]
pub struct Inline {
    pub markup: String,
    pub plain: String,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub kind: String,
    pub level: i32,
    pub indent: i32,
    pub marker: String,
    pub content: Inline,
    pub language: String,
    pub rows: Vec<(bool, Vec<Inline>)>,
}

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub html: String,
    pub blocks: Vec<Block>,
}

pub fn safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    url.len() <= 8192
        && !url.chars().any(char::is_control)
        && (lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:"))
}

fn tag(node: &Handle) -> &str {
    match &node.data {
        NodeData::Element { name, .. } => name.local.as_ref(),
        _ => "",
    }
}

fn attr(node: &Handle, name: &str) -> String {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.as_ref() == name)
            .map(|a| a.value.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn clean(node: &Handle, depth: usize) {
    if depth > 64 {
        node.children.borrow_mut().clear();
        return;
    }
    if let NodeData::Element { attrs, .. } = &node.data {
        let element = tag(node);
        attrs
            .borrow_mut()
            .retain(|attribute| match attribute.name.local.as_ref() {
                "href" => element == "a" && safe_link(&attribute.value),
                "src" => element == "img" && safe_link(&attribute.value),
                "alt" | "title" => true,
                "start" => element == "ol",
                "class" => element == "code",
                "type" | "checked" | "disabled" => {
                    element == "input" && attr_input_type_safe(attribute)
                }
                _ => false,
            });
    }
    node.children.borrow_mut().retain(|child| {
        !matches!(
            tag(child),
            "script"
                | "style"
                | "iframe"
                | "object"
                | "embed"
                | "link"
                | "meta"
                | "base"
                | "form"
                | "button"
                | "textarea"
                | "svg"
                | "math"
                | "template"
                | "video"
                | "audio"
        )
    });
    for child in node.children.borrow().iter() {
        clean(child, depth + 1);
    }
}

fn attr_input_type_safe(attribute: &html5ever::Attribute) -> bool {
    attribute.name.local.as_ref() != "type" || attribute.value.as_ref() == "checkbox"
}

fn escape(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '!' | '~' | '|'
        ) {
            output.push('\\');
        }
        output.push(c);
    }
    output
}

fn text_content(node: &Handle) -> String {
    if let NodeData::Text { contents } = &node.data {
        return contents.borrow().to_string();
    }
    node.children
        .borrow()
        .iter()
        .map(text_content)
        .collect::<Vec<_>>()
        .join("")
}

fn inline(node: &Handle) -> Inline {
    if let NodeData::Text { contents } = &node.data {
        let plain = contents.borrow().to_string();
        return Inline {
            markup: escape(&plain),
            plain,
        };
    }
    let mut content = Inline::default();
    for child in node.children.borrow().iter() {
        let child = inline(child);
        content.markup.push_str(&child.markup);
        content.plain.push_str(&child.plain);
    }
    content.markup = match tag(node) {
        "strong" | "b" => format!("**{}**", content.markup),
        "em" | "i" => format!("*{}*", content.markup),
        "del" | "s" | "strike" => format!("~~{}~~", content.markup),
        "u" => format!("<u>{}</u>", content.markup),
        "code" => {
            let raw = text_content(node);
            let fence = "`".repeat(raw.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1);
            content.plain = raw.clone();
            format!("{fence} {raw} {fence}")
        }
        "a" => {
            let url = attr(node, "href");
            if safe_link(&url) {
                let url = url
                    .replace('(', "%28")
                    .replace(')', "%29")
                    .replace(' ', "%20")
                    .replace('\\', "%5C");
                format!("[{}]({url})", content.markup)
            } else {
                content.markup
            }
        }
        "img" => {
            let alt = attr(node, "alt");
            content.plain = if alt.is_empty() { "Image".into() } else { alt };
            let url = attr(node, "src");
            if safe_link(&url) {
                format!("[{}]({url})", escape(&content.plain))
            } else {
                escape(&content.plain)
            }
        }
        "input" => {
            let checked = if let NodeData::Element { attrs, .. } = &node.data {
                attrs
                    .borrow()
                    .iter()
                    .any(|a| a.name.local.as_ref() == "checked")
            } else {
                false
            };
            content.plain = if checked { "☑ " } else { "☐ " }.into();
            content.plain.clone()
        }
        "br" => {
            content.plain = "\n".into();
            "  \n".into()
        }
        _ => content.markup,
    };
    content
}

#[derive(Clone, Default)]
struct Context {
    indent: i32,
    marker: String,
    quote: bool,
    in_list: bool,
}

fn emit(blocks: &mut Vec<Block>, mut block: Block, context: &mut Context) {
    block.indent = context.indent.min(4);
    block.marker = std::mem::take(&mut context.marker);
    if context.quote && block.kind == "paragraph" {
        block.kind = "quote".into();
    }
    blocks.push(block);
}

fn is_block(node: &Handle) -> bool {
    matches!(
        tag(node),
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "ul"
            | "ol"
            | "table"
            | "hr"
            | "div"
            | "section"
            | "article"
            | "html"
            | "body"
            | "head"
            | "dl"
            | "dt"
            | "dd"
    )
}

fn flush(blocks: &mut Vec<Block>, pending: &mut Inline, context: &mut Context) {
    if !pending.plain.trim().is_empty() {
        let mut content = std::mem::take(pending);
        content.markup = content.markup.trim().to_string();
        content.plain = content.plain.trim().to_string();
        emit(
            blocks,
            Block {
                kind: "paragraph".into(),
                content,
                ..Default::default()
            },
            context,
        );
    } else {
        *pending = Inline::default();
    }
}

fn container(node: &Handle, context: &mut Context, blocks: &mut Vec<Block>, depth: usize) {
    if depth > 64 || blocks.len() >= 4096 {
        return;
    }
    let mut pending = Inline::default();
    for child in node.children.borrow().iter() {
        if is_block(child) {
            flush(blocks, &mut pending, context);
            block(child, context, blocks, depth + 1);
        } else {
            let content = inline(child);
            pending.markup.push_str(&content.markup);
            pending.plain.push_str(&content.plain);
        }
    }
    flush(blocks, &mut pending, context);
}

fn table_rows(node: &Handle, rows: &mut Vec<(bool, Vec<Inline>)>) {
    if tag(node) == "tr" {
        let children = node.children.borrow();
        let cells: Vec<_> = children
            .iter()
            .filter(|n| matches!(tag(n), "th" | "td"))
            .map(inline)
            .collect();
        let header = children.iter().any(|n| tag(n) == "th");
        if !cells.is_empty() {
            rows.push((header, cells));
        }
    } else {
        for child in node.children.borrow().iter() {
            table_rows(child, rows);
        }
    }
}

fn block(node: &Handle, context: &mut Context, blocks: &mut Vec<Block>, depth: usize) {
    match tag(node) {
        "p" | "dt" | "dd" => emit(
            blocks,
            Block {
                kind: "paragraph".into(),
                content: inline(node),
                ..Default::default()
            },
            context,
        ),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let mut content = inline(node);
            if !content.plain.trim().is_empty() {
                content.markup = format!("**{}**", escape(&content.plain));
                emit(
                    blocks,
                    Block {
                        kind: "heading".into(),
                        level: tag(node)[1..].parse().unwrap_or(2),
                        content,
                        ..Default::default()
                    },
                    context,
                );
            }
        }
        "pre" => {
            let language = node
                .children
                .borrow()
                .iter()
                .find(|n| tag(n) == "code")
                .map(|n| {
                    attr(n, "class")
                        .strip_prefix("language-")
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
            let plain = text_content(node);
            emit(
                blocks,
                Block {
                    kind: "code".into(),
                    content: Inline {
                        markup: String::new(),
                        plain,
                    },
                    language,
                    ..Default::default()
                },
                context,
            );
        }
        "ul" | "ol" => {
            let ordered = tag(node) == "ol";
            let mut index: u64 = attr(node, "start").parse().unwrap_or(1);
            for item in node.children.borrow().iter().filter(|n| tag(n) == "li") {
                let mut item_context = Context {
                    indent: context.indent + i32::from(context.in_list),
                    marker: if ordered {
                        format!("{index}.")
                    } else {
                        "•".into()
                    },
                    in_list: true,
                    quote: context.quote,
                };
                container(item, &mut item_context, blocks, depth + 1);
                index = index.saturating_add(1);
            }
        }
        "blockquote" => {
            let mut quote = context.clone();
            quote.quote = true;
            container(node, &mut quote, blocks, depth + 1);
            context.marker.clear();
        }
        "hr" => emit(
            blocks,
            Block {
                kind: "rule".into(),
                ..Default::default()
            },
            context,
        ),
        "table" => {
            let mut rows = Vec::new();
            table_rows(node, &mut rows);
            emit(
                blocks,
                Block {
                    kind: "table".into(),
                    rows,
                    ..Default::default()
                },
                context,
            );
        }
        "head" => {}
        _ => container(node, context, blocks, depth + 1),
    }
}

pub fn parse(markdown: &str) -> Document {
    let mut html = String::new();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    pulldown_cmark::html::push_html(&mut html, Parser::new_ext(markdown, options));
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    clean(&dom.document, 0);
    let mut document = Document::default();
    container(
        &dom.document,
        &mut Context::default(),
        &mut document.blocks,
        0,
    );
    let mut serialized = Vec::new();
    if serialize(
        &mut serialized,
        &SerializableHandle::from(dom.document),
        Default::default(),
    )
    .is_ok()
    {
        document.html = String::from_utf8(serialized).unwrap_or_default();
    }
    document
}

fn styled(content: &Inline, strong: bool) -> StyledText {
    let markup = if strong {
        format!("**{}**", content.markup)
    } else {
        content.markup.clone()
    };
    StyledText::from_markdown(&markup)
        .unwrap_or_else(|_| StyledText::from_plain_text(&content.plain))
}

impl Document {
    pub fn apply(self, row: &mut MessageRow) {
        row.html = self.html.into();
        let blocks: Vec<_> = self
            .blocks
            .into_iter()
            .map(|block| {
                let rows: Vec<_> = block
                    .rows
                    .iter()
                    .map(|(header, cells)| {
                        let cells: Vec<_> = cells
                            .iter()
                            .map(|cell| RichCellRow {
                                content: styled(cell, *header),
                            })
                            .collect();
                        RichTableRow {
                            header: *header,
                            cells: ModelRc::new(VecModel::from(cells)),
                        }
                    })
                    .collect();
                RichBlockRow {
                    content: styled(&block.content, false),
                    text: block.content.plain.into(),
                    kind: block.kind.into(),
                    level: block.level,
                    indent: block.indent,
                    marker: block.marker.into(),
                    language: block.language.into(),
                    rows: ModelRc::new(VecModel::from(rows)),
                }
            })
            .collect();
        row.rich_blocks = ModelRc::new(VecModel::from(blocks));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titres_listes_code_tableaux_et_citations() {
        let source = "# Bonjour **LYRA**\n\nUn [lien](https://hypr.land) et du `code`.\n\n1. Premier\n2. Second\n   - Enfant\n\n> Une citation\n\n```lua\nif a < 3 then print(\"&\") end\n```\n\n| Réglage | Valeur |\n|---|---|\n| gaps | **10** |\n";
        let doc = parse(source);
        assert!(doc.html.contains("<h1>"));
        assert_eq!(doc.blocks[0].kind, "heading");
        assert!(doc.blocks.iter().any(|b| b.marker == "2."));
        assert!(doc.blocks.iter().any(|b| b.indent == 1));
        assert!(doc.blocks.iter().any(|b| b.kind == "quote"));
        let code = doc.blocks.iter().find(|b| b.kind == "code").unwrap();
        assert_eq!(code.language, "lua");
        assert_eq!(code.content.plain, "if a < 3 then print(\"&\") end\n");
        assert_eq!(
            doc.blocks
                .iter()
                .find(|b| b.kind == "table")
                .unwrap()
                .rows
                .len(),
            2
        );
    }

    #[test]
    fn html_nettoye_et_liens_sans_execution() {
        let doc = parse(
            "<h2 onclick='alert(1)'>Titre</h2><script>secret()</script><p><b>Gras</b> &amp; <a href='javascript:alert(1)'>non</a></p>",
        );
        assert!(!doc.html.contains("onclick"));
        assert!(!doc.html.contains("<script"));
        assert!(!doc.html.contains("javascript:"));
        assert!(
            doc.blocks
                .iter()
                .any(|b| b.content.plain.contains("Gras & non"))
        );
        assert!(safe_link("https://hypr.land"));
        assert!(!safe_link("file:///etc/passwd"));
        assert!(!safe_link("https://site\ncommande"));
    }

    #[test]
    fn chaque_fragment_de_streaming_est_valide() {
        let source = "## Un titre\n\n**Bonjour** à tous.\n\n```lua\na = '<b>&</b>'\n```\n";
        for (index, _) in source.char_indices() {
            let doc = parse(&source[..index]);
            for block in &doc.blocks {
                if block.kind != "code" {
                    assert!(
                        StyledText::from_markdown(&block.content.markup).is_ok(),
                        "{}",
                        block.content.markup
                    );
                }
            }
        }
    }
}
