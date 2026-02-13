use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};

#[derive(Debug, Clone)]
pub struct MarkdownRenderer {
    options: Options,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_FOOTNOTES);
        Self { options }
    }
}

impl MarkdownRenderer {
    pub fn render(&self, markdown: &str) -> String {
        let mut output = String::with_capacity(markdown.len().saturating_mul(2) + 128);
        output.push_str("<article id=\"md-root\">");

        let line_starts = line_start_indices(markdown);
        let parser = Parser::new_ext(markdown, self.options).into_offset_iter();

        let mut last_line = 1usize;
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
                Event::Html(raw) => push_escaped_html(&mut output, raw.as_ref()),
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
    image_titles: &mut Vec<Option<String>>,
    in_table_head: &mut bool,
) {
    match tag {
        Tag::Paragraph => open_block_tag(out, "p", line),
        Tag::Heading(level, _id, _classes) => {
            let level = heading_level_number(level);
            out.push_str("<h");
            out.push_str(&level.to_string());
            out.push_str(" data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\">");
        }
        Tag::BlockQuote => open_block_tag(out, "blockquote", line),
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
        Tag::Item => open_block_tag(out, "li", line),
        Tag::Emphasis => out.push_str("<em>"),
        Tag::Strong => out.push_str("<strong>"),
        Tag::Strikethrough => out.push_str("<del>"),
        Tag::Link(_kind, dest, title) => {
            out.push_str("<a href=\"");
            push_escaped_attr(out, &sanitize_url(dest.as_ref()));
            out.push('"');
            if !title.is_empty() {
                out.push_str(" title=\"");
                push_escaped_attr(out, title.as_ref());
                out.push('"');
            }
            out.push('>');
        }
        Tag::Image(_kind, dest, title) => {
            out.push_str("<img src=\"");
            push_escaped_attr(out, &sanitize_url(dest.as_ref()));
            out.push_str("\" alt=\"");
            if title.is_empty() {
                image_titles.push(None);
            } else {
                image_titles.push(Some(title.to_string()));
            }
        }
        Tag::FootnoteDefinition(label) => {
            out.push_str("<section data-line=\"");
            out.push_str(&line.to_string());
            out.push_str("\" class=\"footnote\" data-footnote=\"");
            push_escaped_attr(out, label.as_ref());
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

fn render_end_tag(out: &mut String, tag: Tag<'_>, in_table_head: &mut bool) {
    match tag {
        Tag::Paragraph => out.push_str("</p>"),
        Tag::Heading(level, _id, _classes) => {
            let level = heading_level_number(level);
            out.push_str("</h");
            out.push_str(&level.to_string());
            out.push('>');
        }
        Tag::BlockQuote => out.push_str("</blockquote>"),
        Tag::CodeBlock(_) => out.push_str("</code></pre>"),
        Tag::List(Some(_)) => out.push_str("</ol>"),
        Tag::List(None) => out.push_str("</ul>"),
        Tag::Item => out.push_str("</li>"),
        Tag::Emphasis => out.push_str("</em>"),
        Tag::Strong => out.push_str("</strong>"),
        Tag::Strikethrough => out.push_str("</del>"),
        Tag::Link(..) => out.push_str("</a>"),
        Tag::Image(..) => {}
        Tag::FootnoteDefinition(_) => out.push_str("</section>"),
        Tag::Table(_) => out.push_str("</table>"),
        Tag::TableHead => {
            *in_table_head = false;
            out.push_str("</thead>");
        }
        Tag::TableRow => out.push_str("</tr>"),
        Tag::TableCell => {
            if *in_table_head {
                out.push_str("</th>");
            } else {
                out.push_str("</td>");
            }
        }
    }
}

fn render_image_alt_event(
    out: &mut String,
    image_titles: &mut Vec<Option<String>>,
    event: Event<'_>,
) {
    match event {
        Event::End(Tag::Image(..)) => {
            out.push('"');
            if let Some(Some(title)) = image_titles.pop() {
                out.push_str(" title=\"");
                push_escaped_attr(out, &title);
                out.push('"');
            }
            out.push_str(" />");
        }
        Event::Text(text) | Event::Code(text) | Event::Html(text) => {
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

        assert!(html.contains("<h1 data-line=\"1\">Heading</h1>"));
        assert!(html.contains("<li data-line=\"3\">one</li>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn strips_dangerous_links() {
        let renderer = MarkdownRenderer::default();
        let markdown = "[x](javascript:alert(1))";
        let html = renderer.render(markdown);
        assert!(html.contains("href=\"#\""));
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
}
