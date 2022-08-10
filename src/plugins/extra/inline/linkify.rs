//! Find urls and emails, and turn them into links
use std::cmp::Ordering;

use linkify::{LinkFinder, LinkKind};
use once_cell::sync::Lazy;
use regex::Regex;
use crate::parser::core::{CoreRule, Root};
use crate::parser::inline::builtin::InlineParserRule;
use crate::parser::inline::{InlineRule, InlineState, Text};
use crate::{MarkdownIt, Node, NodeValue, Renderer};

static SCHEME_RE : Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z0-9.+-])([a-z][a-z0-9.+-]*)$").unwrap()
});

#[derive(Debug)]
pub struct Linkified {
    pub url: String,
}

impl NodeValue for Linkified {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("href", self.url.clone()));

        fmt.open("a", &attrs);
        fmt.contents(&node.children);
        fmt.close("a");
    }
}

pub fn add(md: &mut MarkdownIt) {
    md.add_rule::<LinkifyPrescan>()
        .before::<InlineParserRule>();

    md.inline.add_rule::<LinkifyScanner>();
}

type LinkifyState = Vec<LinkifyPosition>;

#[derive(Debug, Clone, Copy)]
struct LinkifyPosition {
    start: usize,
    end:   usize,
    //email: bool,
}

#[doc(hidden)]
pub struct LinkifyPrescan;
impl CoreRule for LinkifyPrescan {
    fn run(root: &mut Node, _: &MarkdownIt) {
        let root_data = root.cast_mut::<Root>().unwrap();
        let source = root_data.content.as_str();
        let finder = LinkFinder::new();
        let positions = finder.links(source).filter_map(|link| {
            if *link.kind() == LinkKind::Url {
                Some(LinkifyPosition {
                    start: link.start(),
                    end:   link.end(),
                    //email: *link.kind() == LinkKind::Email,
                })
            } else {
                None
            }
        }).collect::<Vec<_>>();
        root_data.env.insert(positions);
    }
}

#[doc(hidden)]
pub struct LinkifyScanner;
impl InlineRule for LinkifyScanner {
    const MARKER: char = ':';

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        let mut chars = state.src[state.pos..state.pos_max].chars();
        if chars.next().unwrap() != ':' { return None; }
        if state.link_level > 0 { return None; }

        let trailing = state.trailing_text_get();
        if !SCHEME_RE.is_match(trailing) { return None; }

        let map = state.get_map(state.pos, state.pos_max)?;
        let (start, _) = map.get_byte_offsets();

        let positions = state.root_env.get::<LinkifyState>().unwrap();

        let found_idx = positions.binary_search_by(|x| {
            if x.start >= start {
                Ordering::Greater
            } else if x.end <= start {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }).ok()?;

        let found = positions[found_idx];
        let proto_size = start - found.start;
        if proto_size > trailing.len() { return None; }

        debug_assert_eq!(
            &trailing[trailing.len()-proto_size..],
            &state.src[state.pos-proto_size..state.pos]
        );

        let url_start = state.pos - proto_size;
        let url_end = state.pos - proto_size + found.end - found.start;
        if url_end > state.pos_max { return None; }

        let url = &state.src[url_start..url_end];
        let full_url = (state.md.normalize_link)(url);

        if !(state.md.validate_link)(&full_url) { return None; }

        let content = (state.md.normalize_link_text)(url);

        let mut inner_node = Node::new(Text { content });
        inner_node.srcmap = state.get_map(url_start, url_end);

        let mut node = Node::new(Linkified { url: full_url });
        node.children.push(inner_node);

        state.trailing_text_pop(proto_size);
        state.pos -= proto_size;
        Some((node, url_end - url_start))
    }
}
