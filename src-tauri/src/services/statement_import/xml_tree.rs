//! A small DOM over quick-xml events: the CAMT parser walks it by local element name.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use serde_json::json;

use crate::error::{AppError, Result};

/// Real statements nest about a dozen levels; a deeper document is refused instead of
/// building a tree whose recursive drop could overflow the stack.
const MAX_DEPTH: usize = 128;

/// A statement file is at most 50 MB, so a tree of this many elements is far beyond any real
/// statement and would only be built to exhaust memory (`<a/>` is four bytes).
const MAX_NODES: usize = 2_000_000;

/// One element: local name (no prefix), decoded attributes, its own text and children.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The first match at every step of the path.
    pub fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |node, name| node.child(name))
    }

    /// Trimmed text of the node at `path`; `None` when it is missing or blank.
    pub fn text_at(&self, path: &[&str]) -> Option<&str> {
        let text = self.at(path)?.text.trim();
        (!text.is_empty()).then_some(text)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Parses a whole document into its root element. Any XML error, a document nested deeper
/// than [`MAX_DEPTH`] or with more than [`MAX_NODES`] elements is `validation.statementMalformed`
/// (detail: the open element, else `xml`, cut to [`MAX_DETAIL_CHARS`] characters); a document
/// type declaration is `validation.statementDoctype`.
pub fn parse_xml(text: &str) -> Result<Node> {
    parse_xml_with_limits(text, MAX_DEPTH, MAX_NODES)
}

fn parse_xml_with_limits(text: &str, max_depth: usize, max_nodes: usize) -> Result<Node> {
    let mut reader = Reader::from_str(text);
    let mut open: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut nodes = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| malformed(open_name(&open)))?;
        match event {
            Event::Start(start) => {
                if (open.is_empty() && root.is_some()) || open.len() >= max_depth {
                    return Err(malformed(&start_name(&start)));
                }
                count_node(&mut nodes, max_nodes)?;
                open.push(node_from(&start)?);
            }
            Event::Empty(start) => {
                count_node(&mut nodes, max_nodes)?;
                attach(&mut open, &mut root, node_from(&start)?)?;
            }
            Event::End(_) => {
                let node = open.pop().ok_or_else(|| malformed("xml"))?;
                attach(&mut open, &mut root, node)?;
            }
            Event::Text(text) => {
                if let Some(top) = open.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(data) => {
                if let Some(top) = open.last_mut() {
                    top.text.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(character)) => character.to_string(),
                    Ok(None) => resolve_predefined_entity(&reference)
                        .map(str::to_string)
                        .ok_or_else(|| malformed(&format!("entity {}", &*reference)))?,
                    Err(_) => return Err(malformed(&format!("entity {}", &*reference))),
                };
                if let Some(top) = open.last_mut() {
                    top.text.push_str(&resolved);
                }
            }
            Event::DocType(_) => {
                return Err(AppError::Validation("validation.statementDoctype".into()))
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !open.is_empty() {
        return Err(malformed(open_name(&open)));
    }
    root.ok_or_else(|| malformed("xml"))
}

/// Longest `detail` of a malformed-statement error, in characters. Element and entity names
/// come from the file and can be megabytes long.
const MAX_DETAIL_CHARS: usize = 64;

pub(super) fn malformed(detail: &str) -> AppError {
    let detail: String = detail.chars().take(MAX_DETAIL_CHARS).collect();
    AppError::validation_with("validation.statementMalformed", json!({ "detail": detail }))
}

/// Counts one more element and refuses the document past `max_nodes`.
fn count_node(nodes: &mut usize, max_nodes: usize) -> Result<()> {
    *nodes += 1;
    if *nodes > max_nodes {
        return Err(malformed(&format!(
            "too many elements (more than {max_nodes})"
        )));
    }
    Ok(())
}

fn open_name(open: &[Node]) -> &str {
    open.last().map_or("xml", |node| node.name.as_str())
}

fn start_name(start: &BytesStart) -> String {
    start.local_name().as_ref().to_string()
}

fn node_from(start: &BytesStart) -> Result<Node> {
    let name = start_name(start);
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|_| malformed(&name))?;
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|_| malformed(&name))?;
        attrs.push((
            attr.key.local_name().as_ref().to_string(),
            value.into_owned(),
        ));
    }
    Ok(Node {
        name,
        attrs,
        ..Node::default()
    })
}

/// Appends a finished element to its parent, or makes it the root (one root only).
fn attach(open: &mut [Node], root: &mut Option<Node>, node: Node) -> Result<()> {
    if let Some(parent) = open.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err(malformed("xml"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(error: &AppError) -> String {
        let (key, params) = error.keyed().expect("keyed");
        assert_eq!(key, "validation.statementMalformed");
        params["detail"].as_str().expect("detail").to_string()
    }

    #[test]
    fn builds_the_tree_by_local_name_with_decoded_attributes() {
        let root = parse_xml(
            r#"<?xml version="1.0"?><ns:Doc xmlns:ns="urn:x"><ns:A ns:Ccy="CZ&amp;K"> 12.50 </ns:A><B/><ns:A>second</ns:A></ns:Doc>"#,
        )
        .unwrap();
        assert_eq!(root.name, "Doc");
        assert_eq!(root.children("A").count(), 2);
        assert_eq!(root.child("A").unwrap().attr("Ccy"), Some("CZ&K"));
        assert_eq!(root.text_at(&["A"]), Some("12.50"));
        assert_eq!(root.at(&["B"]).map(|b| b.children.len()), Some(0));
        assert_eq!(root.text_at(&["B"]), None);
        assert_eq!(root.text_at(&["Missing"]), None);
        assert_eq!(root.attr("Ccy"), None);
    }

    #[test]
    fn at_follows_the_first_match_at_every_step() {
        let root = parse_xml("<R><A><B>one</B></A><A><B>two</B><C>c</C></A></R>").unwrap();
        assert_eq!(root.text_at(&["A", "B"]), Some("one"));
        assert_eq!(root.text_at(&["A", "C"]), None);
    }

    #[test]
    fn resolves_entities_character_references_and_cdata() {
        let root =
            parse_xml("<A>a &amp; b &lt;&#65;&#x42;&apos;&quot;&gt; <![CDATA[<raw> & more]]></A>")
                .unwrap();
        assert_eq!(root.text, "a & b <AB'\"> <raw> & more");
    }

    #[test]
    fn an_unknown_entity_is_malformed() {
        let error = parse_xml("<A>a&nbsp;b</A>").unwrap_err();
        assert_eq!(detail(&error), "entity nbsp");
    }

    #[test]
    fn a_doctype_is_refused() {
        let error = parse_xml("<!DOCTYPE A><A/>").unwrap_err();
        assert!(error.to_string().contains("validation.statementDoctype"));
    }

    #[test]
    fn broken_documents_are_malformed() {
        assert_eq!(detail(&parse_xml("<A><B>").unwrap_err()), "B");
        assert_eq!(detail(&parse_xml("<A></B>").unwrap_err()), "A");
        assert_eq!(detail(&parse_xml("").unwrap_err()), "xml");
        assert_eq!(detail(&parse_xml("just text").unwrap_err()), "xml");
        assert_eq!(detail(&parse_xml("<A/><B/>").unwrap_err()), "xml");
        assert_eq!(detail(&parse_xml("<A x=1/>").unwrap_err()), "A");
    }

    #[test]
    fn absurd_nesting_is_refused_instead_of_overflowing_the_stack() {
        let deep = "<a>".repeat(10_000) + &"</a>".repeat(10_000);
        assert_eq!(detail(&parse_xml(&deep).unwrap_err()), "a");
    }

    #[test]
    fn a_document_with_too_many_elements_is_refused() {
        // Start tags and empty elements both count; the root is one element.
        let nested = "<r><a>x</a><b/><c><d/></c></r>"; // r a b c d = 5 elements
        assert!(parse_xml_with_limits(nested, MAX_DEPTH, 5).is_ok());
        let error = parse_xml_with_limits(nested, MAX_DEPTH, 4).unwrap_err();
        assert!(
            detail(&error).contains("too many elements"),
            "{}",
            detail(&error)
        );
        assert!(detail(&error).contains('4'), "the detail names the cap");

        let empties = format!("<r>{}</r>", "<e/>".repeat(100));
        assert!(parse_xml_with_limits(&empties, MAX_DEPTH, 101).is_ok());
        assert!(parse_xml_with_limits(&empties, MAX_DEPTH, 100).is_err());
        assert_eq!(MAX_NODES, 2_000_000);
        assert!(parse_xml(&empties).is_ok());
    }

    #[test]
    fn file_controlled_names_are_cut_in_an_error_detail() {
        let long = "a".repeat(10_000);
        // A mismatched end tag reports the open element.
        let open = detail(&parse_xml(&format!("<{long}></b>")).unwrap_err());
        assert_eq!(open, "a".repeat(64));
        // An unknown entity reports its name.
        let entity = detail(&parse_xml(&format!("<A>&{long};</A>")).unwrap_err());
        assert!(entity.starts_with("entity aaaa"));
        assert_eq!(entity.chars().count(), 64);
        // A bad attribute reports the element.
        let attribute = detail(&parse_xml(&format!("<{long} x=1/>")).unwrap_err());
        assert_eq!(attribute, "a".repeat(64));
        // The cut is on a character boundary.
        let wide = "é".repeat(10_000);
        let cut = detail(&parse_xml(&format!("<{wide}></b>")).unwrap_err());
        assert_eq!(cut, "é".repeat(64));
        // A short detail is untouched.
        assert_eq!(detail(&parse_xml("<A></B>").unwrap_err()), "A");
    }
}
