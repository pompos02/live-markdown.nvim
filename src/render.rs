use std::collections::{HashMap, HashSet, VecDeque};

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag,
    TagEnd,
};

#[derive(Debug, Clone)]
pub struct MarkdownRenderer {
    options: Options,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        let mut options = Options::empty();
        options.insert(Options::all());
        Self { options }
    }
}

impl MarkdownRenderer {
    pub fn render(&self, markdown: &str) -> String {
        let mut output = String::with_capacity(markdown.len().saturating_mul(2) + 128);
        output.push_str("<article id=\"md-root\">");

        let line_starts = line_start_indices(markdown);
        let heading_ids = collect_heading_ids(markdown, self.options);
        let parser = Parser::new_ext(markdown, self.options).into_offset_iter();

        let mut last_line = 1usize;
        let mut heading_index = 0usize;
        let mut image_titles: Vec<Option<String>> = Vec::new();
        let mut in_table_head = false;

        for (event, range) in parser {
            let mut line = line_for_offset(range.start, &line_starts);
            if line < last_line {
                line = last_line;
            } else {
                last_line = line;
            }

            if !image_titles.is_empty() {
                render_image_alt_event(&mut output, &mut image_titles, event);
                continue;
            }

            match event {
                Event::Start(tag) => render_start_tag(
                    &mut output,
                    tag,
                    line,
                    &heading_ids,
                    &mut heading_index,
                    &mut image_titles,
                    &mut in_table_head,
                ),
                Event::End(tag) => render_end_tag(&mut output, tag, &mut in_table_head),
                Event::Text(text) => push_escaped_html(&mut output, text.as_ref()),
                Event::Code(text) => {
                    output.push_str("<code>");
                    push_escaped_html(&mut output, text.as_ref());
                    output.push_str("</code>");
                }
                Event::InlineMath(math) => {
                    output.push_str("<span class=\"math-inline\">");
                    push_escaped_html(&mut output, math.as_ref());
                    output.push_str("</span>");
                }
                Event::DisplayMath(math) => {
                    output.push_str("<div class=\"math-display\">");
                    push_escaped_html(&mut output, math.as_ref());
                    output.push_str("</div>");
                }
                Event::Html(raw) | Event::InlineHtml(raw) => {
                    push_escaped_html(&mut output, raw.as_ref())
                }
                Event::FootnoteReference(label) => {
                    output.push_str("<sup>");
                    push_escaped_html(&mut output, label.as_ref());
                    output.push_str("</sup>");
                }
                Event::SoftBreak => output.push('\n'),
                Event::HardBreak => output.push_str("<br />\n"),
                Event::Rule => output.push_str("<hr />"),
                Event::TaskListMarker(checked) => {
                    if checked {
                        output.push_str("<input type=\"checkbox\" checked disabled /> ");
                    } else {
                        output.push_str("<input type=\"checkbox\" disabled /> ");
                    }
                }
            }
        }

        output.push_str("</article>");
        output
    }
}

fn render_start_tag(
    out: &mut String,
    tag: Tag<'_>,
    line: usize,
    heading_ids: &[String],
    heading_index: &mut usize,
    image_titles: &mut Vec<Option<String>>,
    in_table_head: &mut bool,
) {
    match tag {
        Tag::Paragraph => open_block_tag(out, "p", line),
        Tag::Heading {
            level,
            id: _,
            classes: _,
            attrs: _,
        } => {
            let level = heading_level_number(level);
            out.push_str("<h");
            out.push_str(&level.to_string());
            out.push_str(" data-line=\"");
            out.push_str(&line.to_string());
            out.push('"');
            if let Some(heading_id) = heading_ids.get(*heading_index) {
                out.push_str(" id=\"");
                push_escaped_attr(out, heading_id);
                out.push('"');
            }
            out.push('>');
            *heading_index = heading_index.saturating_add(1);
        }
        Tag::BlockQuote(kind) => {
            out.push_str("<blockquote data-line=\"");
            out.push_str(&line.to_string());
            out.push('"');
            if let Some(kind) = kind {
                let kind_name = block_quote_kind_name(kind);
                out.push_str(" data-alert=\"");
                out.push_str(kind_name);
                out.push_str("\" class=\"markdown-alert markdown-alert-");
                out.push_str(kind_name);
                out.push_str("\">");
                render_alert_title(out, kind);
            } else {
                out.push('>');
            }
        }
        Tag::CodeBlock(kind) => {
            out.push_str("<pre data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\"><code");
            if let CodeBlockKind::Fenced(lang) = kind {
                let trimmed = lang.trim();
                if !trimmed.is_empty() {
                    out.push_str(" class=\"language-");
                    push_escaped_attr(out, trimmed);
                    out.push('"');
                }
            }
            out.push('>');
        }
        Tag::List(start) => {
            if let Some(start) = start {
                out.push_str("<ol start=\"");
                out.push_str(&start.to_string());
                out.push_str("\">");
            } else {
                out.push_str("<ul>");
            }
        }
        Tag::DefinitionList => out.push_str("<dl>"),
        Tag::DefinitionListTitle => open_block_tag(out, "dt", line),
        Tag::DefinitionListDefinition => open_block_tag(out, "dd", line),
        Tag::Item => open_block_tag(out, "li", line),
        Tag::Emphasis => out.push_str("<em>"),
        Tag::Superscript => out.push_str("<sup>"),
        Tag::Subscript => out.push_str("<sub>"),
        Tag::Strong => out.push_str("<strong>"),
        Tag::Strikethrough => out.push_str("<del>"),
        Tag::Link {
            link_type: _,
            dest_url,
            title,
            id: _,
        } => {
            out.push_str("<a href=\"");
            push_escaped_attr(out, &sanitize_url(dest_url.as_ref()));
            out.push('"');
            if !title.is_empty() {
                out.push_str(" title=\"");
                push_escaped_attr(out, title.as_ref());
                out.push('"');
            }
            out.push('>');
        }
        Tag::Image {
            link_type: _,
            dest_url,
            title,
            id: _,
        } => {
            out.push_str("<img src=\"");
            push_escaped_attr(out, &sanitize_image_url(dest_url.as_ref()));
            out.push_str("\" alt=\"");
            if title.is_empty() {
                image_titles.push(None);
            } else {
                image_titles.push(Some(title.to_string()));
            }
        }
        Tag::HtmlBlock => {
            out.push_str("<pre data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\" class=\"html-block\">");
        }
        Tag::FootnoteDefinition(label) => {
            out.push_str("<section data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\" class=\"footnote\" data-footnote=\"");
            push_escaped_attr(out, label.as_ref());
            out.push_str("\">");
        }
        Tag::MetadataBlock(kind) => {
            out.push_str("<pre data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\" class=\"metadata-block metadata-");
            out.push_str(metadata_block_kind_name(kind));
            out.push_str("\">");
        }
        Tag::Table(_alignments) => open_block_tag(out, "table", line),
        Tag::TableHead => {
            *in_table_head = true;
            out.push_str("<thead>");
        }
        Tag::TableRow => out.push_str("<tr>"),
        Tag::TableCell => {
            if *in_table_head {
                out.push_str("<th>");
            } else {
                out.push_str("<td>");
            }
        }
    }
}

fn render_end_tag(out: &mut String, tag: TagEnd, in_table_head: &mut bool) {
    match tag {
        TagEnd::Paragraph => out.push_str("</p>"),
        TagEnd::Heading(level) => {
            let level = heading_level_number(level);
            out.push_str("</h");
            out.push_str(&level.to_string());
            out.push('>');
        }
        TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
        TagEnd::CodeBlock => out.push_str("</code></pre>"),
        TagEnd::HtmlBlock => out.push_str("</pre>"),
        TagEnd::List(true) => out.push_str("</ol>"),
        TagEnd::List(false) => out.push_str("</ul>"),
        TagEnd::Item => out.push_str("</li>"),
        TagEnd::FootnoteDefinition => out.push_str("</section>"),
        TagEnd::DefinitionList => out.push_str("</dl>"),
        TagEnd::DefinitionListTitle => out.push_str("</dt>"),
        TagEnd::DefinitionListDefinition => out.push_str("</dd>"),
        TagEnd::Table => out.push_str("</table>"),
        TagEnd::TableHead => {
            *in_table_head = false;
            out.push_str("</thead>");
        }
        TagEnd::TableRow => out.push_str("</tr>"),
        TagEnd::TableCell => {
            if *in_table_head {
                out.push_str("</th>");
            } else {
                out.push_str("</td>");
            }
        }
        TagEnd::Emphasis => out.push_str("</em>"),
        TagEnd::Strong => out.push_str("</strong>"),
        TagEnd::Strikethrough => out.push_str("</del>"),
        TagEnd::Superscript => out.push_str("</sup>"),
        TagEnd::Subscript => out.push_str("</sub>"),
        TagEnd::Link => out.push_str("</a>"),
        TagEnd::Image => {}
        TagEnd::MetadataBlock(_) => out.push_str("</pre>"),
    }
}

fn render_image_alt_event(
    out: &mut String,
    image_titles: &mut Vec<Option<String>>,
    event: Event<'_>,
) {
    match event {
        Event::End(TagEnd::Image) => {
            out.push('"');
            if let Some(Some(title)) = image_titles.pop() {
                out.push_str(" title=\"");
                push_escaped_attr(out, &title);
                out.push('"');
            }
            out.push_str(" />");
        }
        Event::Text(text)
        | Event::Code(text)
        | Event::Html(text)
        | Event::InlineHtml(text)
        | Event::InlineMath(text)
        | Event::DisplayMath(text)
        | Event::FootnoteReference(text) => {
            push_escaped_attr(out, text.as_ref());
        }
        Event::SoftBreak | Event::HardBreak => out.push(' '),
        _ => {}
    }
}

fn open_block_tag(out: &mut String, tag: &str, line: usize) {
    out.push('<');
    out.push_str(tag);
    out.push_str(" data-line=\"");
    out.push_str(&line.to_string());
    out.push_str("\">");
}

fn line_start_indices(markdown: &str) -> Vec<usize> {
    let mut starts = Vec::with_capacity(markdown.lines().count() + 1);
    starts.push(0);
    for (idx, byte) in markdown.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_for_offset(offset: usize, starts: &[usize]) -> usize {
    match starts.binary_search(&offset) {
        Ok(idx) => idx + 1,
        Err(0) => 1,
        Err(idx) => idx,
    }
}

fn collect_heading_ids(markdown: &str, options: Options) -> Vec<String> {
    let mut ids = Vec::new();
    let mut used_ids = HashSet::new();
    let mut next_suffixes: HashMap<String, usize> = HashMap::new();
    let mut heading_aliases = collect_internal_heading_aliases(markdown, options);

    let mut heading_text: Option<String> = None;
    let mut explicit_heading_id: Option<String> = None;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Heading {
                level: _,
                id,
                classes: _,
                attrs: _,
            }) => {
                heading_text = Some(String::new());
                explicit_heading_id = normalize_heading_id(id.as_deref());
            }
            Event::End(TagEnd::Heading(_)) => {
                let text = heading_text.take().unwrap_or_default();
                let base = if let Some(explicit) = explicit_heading_id.take() {
                    explicit
                } else if let Some(alias) = take_heading_alias(&mut heading_aliases, &text) {
                    alias
                } else {
                    slugify_heading(&text)
                };
                let unique = unique_heading_id(base, &mut used_ids, &mut next_suffixes);
                ids.push(unique);
            }
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                if let Some(current) = heading_text.as_mut() {
                    current.push_str(text.as_ref());
                }
            }
            Event::FootnoteReference(text) => {
                if let Some(current) = heading_text.as_mut() {
                    current.push_str(text.as_ref());
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(current) = heading_text.as_mut() {
                    if !current.ends_with(' ') {
                        current.push(' ');
                    }
                }
            }
            _ => {}
        }
    }

    ids
}

fn collect_internal_heading_aliases(
    markdown: &str,
    options: Options,
) -> HashMap<String, VecDeque<String>> {
    let mut aliases: HashMap<String, VecDeque<String>> = HashMap::new();

    let mut active_fragment: Option<String> = None;
    let mut active_text = String::new();

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Link {
                link_type: _,
                dest_url,
                title: _,
                id: _,
            }) => {
                active_fragment = internal_fragment_id(dest_url.as_ref());
                active_text.clear();
            }
            Event::End(TagEnd::Link) => {
                if let Some(fragment) = active_fragment.take() {
                    let key = normalize_heading_lookup_text(&active_text);
                    if !key.is_empty() {
                        aliases.entry(key).or_default().push_back(fragment);
                    }
                }
                active_text.clear();
            }
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                if active_fragment.is_some() {
                    active_text.push_str(text.as_ref());
                }
            }
            Event::FootnoteReference(text) => {
                if active_fragment.is_some() {
                    active_text.push_str(text.as_ref());
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if active_fragment.is_some() && !active_text.ends_with(' ') {
                    active_text.push(' ');
                }
            }
            _ => {}
        }
    }

    aliases
}

fn take_heading_alias(
    aliases: &mut HashMap<String, VecDeque<String>>,
    heading_text: &str,
) -> Option<String> {
    let key = normalize_heading_lookup_text(heading_text);
    if key.is_empty() {
        return None;
    }

    let queue = aliases.get_mut(&key)?;
    queue.pop_front()
}

fn internal_fragment_id(dest: &str) -> Option<String> {
    let trimmed = dest.trim();
    let fragment = trimmed.strip_prefix('#')?.trim();
    if fragment.is_empty() {
        None
    } else {
        Some(fragment.to_string())
    }
}

fn normalize_heading_lookup_text(text: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            pending_space = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_space = true;
        }
    }

    output
}

fn normalize_heading_id(id: Option<&str>) -> Option<String> {
    let trimmed = id?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn slugify_heading(text: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            continue;
        }

        if ch.is_whitespace() || ch == '-' || ch == '_' {
            pending_dash = true;
        }
    }

    if slug.is_empty() {
        String::from("section")
    } else {
        slug
    }
}

fn unique_heading_id(
    base: String,
    used_ids: &mut HashSet<String>,
    next_suffixes: &mut HashMap<String, usize>,
) -> String {
    if used_ids.insert(base.clone()) {
        next_suffixes.entry(base.clone()).or_insert(1);
        return base;
    }

    let mut suffix = *next_suffixes.get(&base).unwrap_or(&1);
    loop {
        let candidate = format!("{base}-{suffix}");
        suffix += 1;
        if used_ids.insert(candidate.clone()) {
            next_suffixes.insert(base.clone(), suffix);
            return candidate;
        }
    }
}

fn block_quote_kind_name(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "note",
        BlockQuoteKind::Tip => "tip",
        BlockQuoteKind::Important => "important",
        BlockQuoteKind::Warning => "warning",
        BlockQuoteKind::Caution => "caution",
    }
}

fn block_quote_kind_title(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    }
}

fn block_quote_kind_icon_path(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => {
            "M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
        }
        BlockQuoteKind::Tip => {
            "M8 1.5c-2.363 0-4 1.69-4 3.75 0 .984.424 1.625.984 2.304l.214.253c.223.264.47.556.673.848.284.411.537.896.621 1.49a.75.75 0 0 1-1.484.211c-.04-.282-.163-.547-.37-.847a8.456 8.456 0 0 0-.542-.68c-.084-.1-.173-.205-.268-.32C3.201 7.75 2.5 6.766 2.5 5.25 2.5 2.31 4.863 0 8 0s5.5 2.31 5.5 5.25c0 1.516-.701 2.5-1.328 3.259-.095.115-.184.22-.268.319-.207.245-.383.453-.541.681-.208.3-.33.565-.37.847a.751.751 0 0 1-1.485-.212c.084-.593.337-1.078.621-1.489.203-.292.45-.584.673-.848.075-.088.147-.173.213-.253.561-.679.985-1.32.985-2.304 0-2.06-1.637-3.75-4-3.75ZM5.75 12h4.5a.75.75 0 0 1 0 1.5h-4.5a.75.75 0 0 1 0-1.5ZM6 15.25a.75.75 0 0 1 .75-.75h2.5a.75.75 0 0 1 0 1.5h-2.5a.75.75 0 0 1-.75-.75Z"
        }
        BlockQuoteKind::Important => {
            "M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v9.5A1.75 1.75 0 0 1 14.25 13H8.06l-2.573 2.573A1.458 1.458 0 0 1 3 14.543V13H1.75A1.75 1.75 0 0 1 0 11.25Zm1.75-.25a.25.25 0 0 0-.25.25v9.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h6.5a.25.25 0 0 0 .25-.25v-9.5a.25.25 0 0 0-.25-.25Zm7 2.25v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 9a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
        }
        BlockQuoteKind::Warning => {
            "M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575Zm1.763.707a.25.25 0 0 0-.44 0L1.698 13.132a.25.25 0 0 0 .22.368h12.164a.25.25 0 0 0 .22-.368Zm.53 3.996v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 11a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
        }
        BlockQuoteKind::Caution => {
            "M4.47.22A.749.749 0 0 1 5 0h6c.199 0 .389.079.53.22l4.25 4.25c.141.14.22.331.22.53v6a.749.749 0 0 1-.22.53l-4.25 4.25A.749.749 0 0 1 11 16H5a.749.749 0 0 1-.53-.22L.22 11.53A.749.749 0 0 1 0 11V5c0-.199.079-.389.22-.53Zm.84 1.28L1.5 5.31v5.38l3.81 3.81h5.38l3.81-3.81V5.31L10.69 1.5ZM8 4a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
        }
    }
}

fn render_alert_title(out: &mut String, kind: BlockQuoteKind) {
    out.push_str("<p class=\"markdown-alert-title\"><svg class=\"octicon markdown-alert-icon\" viewBox=\"0 0 16 16\" width=\"16\" height=\"16\" aria-hidden=\"true\"><path d=\"");
    out.push_str(block_quote_kind_icon_path(kind));
    out.push_str("\"></path></svg>");
    out.push_str(block_quote_kind_title(kind));
    out.push_str("</p>");
}

fn metadata_block_kind_name(kind: MetadataBlockKind) -> &'static str {
    match kind {
        MetadataBlockKind::YamlStyle => "yaml",
        MetadataBlockKind::PlusesStyle => "pluses",
    }
}

fn sanitize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::from("#");
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
    {
        return String::from("#");
    }

    trimmed.to_string()
}

fn sanitize_image_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::from("#");
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("vbscript:") {
        return String::from("#");
    }
    if lower.starts_with("data:") && !lower.starts_with("data:image/") {
        return String::from("#");
    }

    trimmed.to_string()
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn push_escaped_html(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

fn push_escaped_attr(out: &mut String, text: &str) {
    push_escaped_html(out, text);
}

#[cfg(test)]
mod tests {
    use super::MarkdownRenderer;

    #[test]
    fn renders_common_markdown_blocks() {
        let renderer = MarkdownRenderer::default();
        let markdown = "# Heading\n\n- one\n- two\n\n`code`";
        let html = renderer.render(markdown);

        assert!(html.contains("<h1 data-line=\"1\" id=\"heading\">Heading</h1>"));
        assert!(html.contains("<li data-line=\"3\">one</li>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn renders_gfm_alert_block_quotes_with_titles_and_icons() {
        let renderer = MarkdownRenderer::default();
        let markdown = "> [!NOTE]\n> note body\n\n> [!TIP]\n> tip body\n\n> [!IMPORTANT]\n> important body\n\n> [!WARNING]\n> warning body\n\n> [!CAUTION]\n> caution body";
        let html = renderer.render(markdown);

        assert!(html.contains("class=\"markdown-alert markdown-alert-note\""));
        assert!(html.contains("class=\"markdown-alert markdown-alert-tip\""));
        assert!(html.contains("class=\"markdown-alert markdown-alert-important\""));
        assert!(html.contains("class=\"markdown-alert markdown-alert-warning\""));
        assert!(html.contains("class=\"markdown-alert markdown-alert-caution\""));

        assert!(html.contains(
            "<p class=\"markdown-alert-title\"><svg class=\"octicon markdown-alert-icon\""
        ));
        assert!(html.contains("</svg>Note</p>"));
        assert!(html.contains("</svg>Tip</p>"));
        assert!(html.contains("</svg>Important</p>"));
        assert!(html.contains("</svg>Warning</p>"));
        assert!(html.contains("</svg>Caution</p>"));
    }

    #[test]
    fn keeps_regular_block_quotes_without_alert_chrome() {
        let renderer = MarkdownRenderer::default();
        let markdown = "> plain quote";
        let html = renderer.render(markdown);

        assert!(html.contains(
            "<blockquote data-line=\"1\"><p data-line=\"1\">plain quote</p></blockquote>"
        ));
        assert!(!html.contains("markdown-alert-title"));
    }

    #[test]
    fn strips_dangerous_links() {
        let renderer = MarkdownRenderer::default();
        let markdown = "[x](javascript:alert(1))";
        let html = renderer.render(markdown);
        assert!(html.contains("href=\"#\""));
    }

    #[test]
    fn sanitizes_image_urls_for_browser_rendering() {
        let renderer = MarkdownRenderer::default();
        let markdown = "![ok](images/diagram.png) ![data](data:image/png;base64,AAAA) ![bad](javascript:alert(1))";
        let html = renderer.render(markdown);

        assert!(html.contains("src=\"images/diagram.png\""));
        assert!(html.contains("src=\"data:image/png;base64,AAAA\""));
        assert!(html.contains("src=\"#\""));
    }

    #[test]
    fn keeps_data_line_markers_monotonic() {
        let renderer = MarkdownRenderer::default();
        let markdown = "line 1\n\nline 3\n\nline 5";
        let html = renderer.render(markdown);

        let mut last = 0usize;
        for part in html.split("data-line=\"").skip(1) {
            let current: usize = part
                .split('"')
                .next()
                .expect("line marker")
                .parse()
                .expect("valid marker");
            assert!(current >= last);
            last = current;
        }
    }

    #[test]
    fn adds_heading_ids_for_internal_links() {
        let renderer = MarkdownRenderer::default();
        let markdown = "# Overview\n## Inline HTML";
        let html = renderer.render(markdown);

        assert!(html.contains("<h1 data-line=\"1\" id=\"overview\">Overview</h1>"));
        assert!(html.contains("<h2 data-line=\"2\" id=\"inline-html\">Inline HTML</h2>"));
    }

    #[test]
    fn keeps_explicit_heading_ids() {
        let renderer = MarkdownRenderer::default();
        let markdown =
            "## Inline HTML {#html}\n## Automatic Escaping for Special Characters {#autoescape}";
        let html = renderer.render(markdown);

        assert!(html.contains("<h2 data-line=\"1\" id=\"html\">Inline HTML</h2>"));
        assert!(html.contains(
            "<h2 data-line=\"2\" id=\"autoescape\">Automatic Escaping for Special Characters</h2>"
        ));
    }

    #[test]
    fn deduplicates_heading_ids() {
        let renderer = MarkdownRenderer::default();
        let markdown = "## Section\n## Section\n## Section-1\n## Section";
        let html = renderer.render(markdown);

        assert!(html.contains("<h2 data-line=\"1\" id=\"section\">Section</h2>"));
        assert!(html.contains("<h2 data-line=\"2\" id=\"section-1\">Section</h2>"));
        assert!(html.contains("<h2 data-line=\"3\" id=\"section-1-1\">Section-1</h2>"));
        assert!(html.contains("<h2 data-line=\"4\" id=\"section-2\">Section</h2>"));
    }

    #[test]
    fn infers_heading_ids_from_internal_toc_links() {
        let renderer = MarkdownRenderer::default();
        let markdown = "- [Inline HTML](#html)\n- [Automatic Escaping for Special Characters](#autoescape)\n\n## Inline HTML\n## Automatic Escaping for Special Characters";
        let html = renderer.render(markdown);

        assert!(html.contains("<h2 data-line=\"4\" id=\"html\">Inline HTML</h2>"));
        assert!(html.contains(
            "<h2 data-line=\"5\" id=\"autoescape\">Automatic Escaping for Special Characters</h2>"
        ));
    }
}
