//! Small bounded XML tree. No DTD expansion, external resources, or namespace guessing.
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use std::collections::BTreeMap;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
pub(crate) enum Content {
    Text(String),
    Element(Node),
}

#[derive(Debug)]
pub(crate) struct Node {
    pub ns: String,
    pub name: String,
    pub attrs: BTreeMap<(String, String), String>,
    pub content: Vec<Content>,
}

impl Node {
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attr_ns("", name)
    }
    pub fn attr_ns(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs
            .get(&(ns.to_string(), name.to_string()))
            .map(String::as_str)
    }
    pub fn children(&self) -> impl Iterator<Item = &Node> {
        self.content.iter().filter_map(|c| match c {
            Content::Element(n) => Some(n),
            _ => None,
        })
    }
    pub fn descendants<'a>(&'a self, ns: &str, name: &str) -> Vec<&'a Node> {
        let mut found = vec![];
        self.visit(&mut |n| {
            if n.is(ns, name) {
                found.push(n);
            }
        });
        found
    }
    pub fn visit<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for n in self.children() {
            n.visit(f);
        }
    }
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.append_text(&mut out);
        out
    }
    fn append_text(&self, out: &mut String) {
        for item in &self.content {
            match item {
                Content::Text(t) => out.push_str(t),
                Content::Element(n) => n.append_text(out),
            }
        }
    }
}

fn namespace(value: ResolveResult<'_>) -> Result<String> {
    match value {
        ResolveResult::Bound(ns) => Ok(ns.as_ref().to_string()),
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Unknown(_) => Err("undeclared XML namespace prefix".into()),
    }
}

fn text(stack: &mut [Node], value: String) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        if let Some(Content::Text(existing)) = parent.content.last_mut() {
            existing.push_str(&value);
        } else {
            parent.content.push(Content::Text(value));
        }
    } else if !value.trim().is_empty() {
        return Err("XML text outside root".into());
    }
    Ok(())
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Node> {
    parse_inner(bytes, false)
}

/// XHTML commonly declares an external DTD. Ignore that declaration, never load it.
pub(crate) fn parse_xhtml(bytes: &[u8]) -> Result<Node> {
    parse_inner(bytes, true)
}

fn parse_inner(bytes: &[u8], xhtml: bool) -> Result<Node> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    let mut nodes = 0;
    let mut version = quick_xml::XmlVersion::default();
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                nodes += 1;
                if nodes > 200_000 || stack.len() >= 128 {
                    return Err("XML node/depth cap exceeded".into());
                }
                let (ns, name) = reader.resolver().resolve_element(e.name());
                let mut node = Node {
                    ns: namespace(ns)?,
                    name: name.as_ref().into(),
                    attrs: BTreeMap::new(),
                    content: vec![],
                };
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == "xmlns" || attr.key.as_ref().starts_with("xmlns:") {
                        continue;
                    }
                    let (ns, name) = reader.resolver().resolve_attribute(attr.key);
                    let key = (namespace(ns)?, name.as_ref().to_string());
                    let value = attr.normalized_value(version)?.into_owned();
                    if node.attrs.insert(key, value).is_some() {
                        return Err("duplicate expanded XML attribute".into());
                    }
                }
                stack.push(node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("unbalanced XML")?;
                if let Some(parent) = stack.last_mut() {
                    parent.content.push(Content::Element(node));
                } else if root.replace(node).is_some() {
                    return Err("multiple XML roots".into());
                }
            }
            Event::Text(e) => text(&mut stack, e.xml_content(version).into_owned())?,
            Event::CData(e) => text(&mut stack, e.xml_content(version).into_owned())?,
            Event::GeneralRef(e) => {
                let value = if let Some(ch) = e.resolve_char_ref()? {
                    ch.to_string()
                } else {
                    match e.xml_content(version).as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        "nbsp" if xhtml => " ",
                        _ => return Err("unsupported XML entity reference".into()),
                    }
                    .to_string()
                };
                text(&mut stack, value)?;
            }
            Event::DocType(_) if !xhtml => return Err("XML DTDs are prohibited".into()),
            Event::Decl(e) => version = e.xml_version()?,
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("truncated XML".into());
    }
    root.ok_or_else(|| "missing XML root".into())
}
