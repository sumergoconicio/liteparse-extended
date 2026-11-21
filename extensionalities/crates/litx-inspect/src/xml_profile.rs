//! Namespace-aware structural profile of standalone XML, accumulated from the
//! bounded `quick_xml::NsReader` event stream without retaining a document
//! tree or any document values.
use crate::{InspectOptions, InspectReport, binary::Source, markdown, xml};
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

/// Maximum standalone XML input bytes, checked before allocation.
const INPUT_CAP: u64 = 16 << 20;
/// Maximum element starts across the document.
const ELEMENT_CAP: u64 = 200_000;
/// Maximum open-element depth; the root is depth 1.
const DEPTH_CAP: usize = 128;
/// Maximum attributes across the document, including xmlns declarations.
const ATTRIBUTE_CAP: u64 = 200_000;
/// Maximum unique expanded element/attribute names.
const NAME_CAP: usize = 4_096;
/// Maximum unique parent-linked paths.
const PATH_CAP: usize = 4_096;
/// Maximum aggregate (path, attribute) and (parent, child) statistic pairs.
const PAIR_CAP: usize = 8_192;
/// Maximum retained local-name plus namespace-URI bytes.
const RETAINED_BYTE_CAP: usize = 1 << 20;
/// Maximum bytes of one local name, checked before copying.
const LOCAL_NAME_CAP: usize = 1 << 10;
/// Maximum bytes of one namespace URI, checked before copying.
const NAMESPACE_CAP: usize = 4 << 10;
/// Maximum aggregate live sibling-counter entries across the open stack.
const LIVE_COUNTER_CAP: usize = 32_768;
/// Maximum serialized XML profile bytes, counted with a bounded writer.
const PROFILE_BYTE_CAP: usize = 1 << 20;
/// Maximum structural summary characters; stricter rendering limits win.
const SUMMARY_CAP: usize = 64 << 10;

/// Structural profile of one standalone XML document. Contains names,
/// hierarchy, counts and patterns only; never document values.
#[derive(Debug, Default, Serialize)]
pub struct XmlProfile {
    pub root_path_id: usize,
    pub total_elements: u64,
    pub max_depth: usize,
    pub max_children: u64,
    pub namespaces: Vec<String>,
    pub names: Vec<XmlName>,
    pub paths: Vec<XmlPath>,
    pub repeated_paths: Vec<usize>,
}

/// Expanded element or attribute name: namespace identity by index, if any.
#[derive(Debug, Serialize)]
pub struct XmlName {
    pub namespace: Option<usize>,
    pub local: String,
}

/// One observed slash-path with occurrence counters. `min_occurs`/`max_occurs`
/// are observed per-parent ranges; zero minima mean some parent instance
/// lacked the child, never that a schema requires or forbids it.
#[derive(Debug, Serialize)]
pub struct XmlPath {
    pub id: usize,
    pub parent_id: Option<usize>,
    pub name_id: usize,
    pub count: u64,
    pub parents_seen: u64,
    pub min_occurs: u64,
    pub max_occurs: u64,
    pub attributes: BTreeMap<usize, u64>,
    pub text_instances: u64,
    pub children_instances: u64,
    pub mixed_instances: u64,
    #[serde(skip)]
    children: Vec<usize>,
}

/// One open element on the bounded stack: identity plus this instance's
/// observed child occurrences.
struct Frame {
    path_id: usize,
    children: HashMap<usize, u64>,
    children_total: u64,
    has_text: bool,
}

/// Event-driven accumulator. Interned dictionaries and shared budgets are
/// charged before growth; no element, attribute value or text is retained.
struct Accumulator {
    profile: XmlProfile,
    namespace_index: HashMap<String, usize>,
    name_index: HashMap<(Option<usize>, String), usize>,
    path_index: HashMap<(Option<usize>, usize), usize>,
    completed: Vec<u64>,
    retained_bytes: usize,
    stat_pairs: usize,
    live_counters: usize,
    live_cap: usize,
    total_attributes: u64,
    stack: Vec<Frame>,
    root: Option<usize>,
}

impl Default for Accumulator {
    /// Production construction pins every limit at its design constant.
    fn default() -> Self {
        Self {
            profile: XmlProfile::default(),
            namespace_index: HashMap::new(),
            name_index: HashMap::new(),
            path_index: HashMap::new(),
            completed: Vec::new(),
            retained_bytes: 0,
            stat_pairs: 0,
            live_counters: 0,
            live_cap: LIVE_COUNTER_CAP,
            total_attributes: 0,
            stack: Vec::new(),
            root: None,
        }
    }
}

impl Accumulator {
    /// Reduced-limit seam for internal unit tests only; production limits are
    /// unreachable at test scale, never weakened at runtime.
    #[cfg(test)]
    fn with_live_counter_cap(live_cap: usize) -> Self {
        Self {
            live_cap,
            ..Self::default()
        }
    }
}

impl Accumulator {
    /// Charges `bytes` against the shared retained-name budget before growth.
    ///
    /// Accepts: the byte length about to be retained.
    /// Returns: `Ok(())` when the charge fits the 1 MiB budget.
    /// Errors: static "XML retained-name byte cap exceeded" when it does not.
    fn charge_retained(&mut self, bytes: usize) -> xml::Result<()> {
        self.retained_bytes = self
            .retained_bytes
            .checked_add(bytes)
            .ok_or("XML retained-name byte cap exceeded")?;
        if self.retained_bytes > RETAINED_BYTE_CAP {
            return Err("XML retained-name byte cap exceeded".into());
        }
        Ok(())
    }

    /// Charges one aggregate statistic pair before insertion.
    ///
    /// Accepts: nothing; the counter is internal.
    /// Returns: `Ok(())` when the pair budget has room.
    /// Errors: static "XML statistic-pair cap exceeded" at the 8,192 cap.
    fn charge_pair(&mut self) -> xml::Result<()> {
        self.stat_pairs = self
            .stat_pairs
            .checked_add(1)
            .ok_or("XML statistic-pair cap exceeded")?;
        if self.stat_pairs > PAIR_CAP {
            return Err("XML statistic-pair cap exceeded".into());
        }
        Ok(())
    }

    /// Interns a used namespace URI after shared identity normalization, so
    /// aliases cannot cause false structural differences.
    ///
    /// Accepts: the raw non-normalized namespace string from the resolver.
    /// Returns: the deterministic namespace index in `profile.namespaces`.
    /// Errors: URI over 4 KiB, unsupported reference, illegal character, or
    /// byte/count cap.
    fn intern_namespace(&mut self, raw: &str) -> xml::Result<usize> {
        if raw.len() > NAMESPACE_CAP {
            return Err("XML namespace URI cap exceeded".into());
        }
        let normalized = normalize_namespace_identity(raw)?;
        if normalized.len() > NAMESPACE_CAP {
            return Err("XML namespace URI cap exceeded".into());
        }
        if let Some(&id) = self.namespace_index.get(&normalized) {
            return Ok(id);
        }
        if self.profile.namespaces.len() >= NAME_CAP {
            return Err("XML namespace cap exceeded".into());
        }
        self.charge_retained(normalized.len())?;
        let id = self.profile.namespaces.len();
        self.profile.namespaces.push(normalized.clone());
        self.namespace_index.insert(normalized, id);
        Ok(id)
    }

    /// Interns an expanded element or attribute name as namespace index plus
    /// local name, never a concatenated string key.
    ///
    /// Accepts: the optional namespace index and the local name.
    /// Returns: the deterministic name index in `profile.names`.
    /// Errors: local name over 1 KiB, or unique-name/byte cap exceeded.
    fn intern_name(&mut self, namespace: Option<usize>, local: &str) -> xml::Result<usize> {
        if local.len() > LOCAL_NAME_CAP {
            return Err("XML local-name cap exceeded".into());
        }
        let key = (namespace, local.to_string());
        if let Some(&id) = self.name_index.get(&key) {
            return Ok(id);
        }
        if self.profile.names.len() >= NAME_CAP {
            return Err("XML unique-name cap exceeded".into());
        }
        self.charge_retained(local.len())?;
        let id = self.profile.names.len();
        self.profile.names.push(XmlName {
            namespace,
            local: local.into(),
        });
        self.name_index.insert(key, id);
        Ok(id)
    }

    /// Interns a parent-linked path and records one element instance count.
    ///
    /// Accepts: the parent path index (`None` for the root) and the name index.
    /// Returns: the deterministic path index in `profile.paths`.
    /// Errors: unique-path cap exceeded.
    fn intern_path(&mut self, parent_id: Option<usize>, name_id: usize) -> xml::Result<usize> {
        let key = (parent_id, name_id);
        if let Some(&id) = self.path_index.get(&key) {
            return Ok(id);
        }
        if self.profile.paths.len() >= PATH_CAP {
            return Err("XML unique-path cap exceeded".into());
        }
        let id = self.profile.paths.len();
        // A child first observed after some parent instances already completed
        // was absent from those instances: its observed minimum is zero.
        let min_occurs = match parent_id {
            Some(parent) if self.completed[parent] > 0 => 0,
            _ => u64::MAX,
        };
        self.profile.paths.push(XmlPath {
            id,
            parent_id,
            name_id,
            count: 0,
            parents_seen: 0,
            min_occurs,
            max_occurs: 0,
            attributes: BTreeMap::new(),
            text_instances: 0,
            children_instances: 0,
            mixed_instances: 0,
            children: vec![],
        });
        self.completed.push(0);
        self.path_index.insert(key, id);
        Ok(id)
    }

    /// Handles one element start: budgets, name/path identity, attribute-name
    /// counts and the parent instance's child occurrence.
    ///
    /// Accepts: the reader (for namespace resolution) and the start event.
    /// Returns: `Ok(())` after the open frame is pushed.
    /// Errors: any cap overflow, undeclared prefix, or duplicate expanded
    /// attribute, all as static content-free messages.
    fn start(
        &mut self,
        reader: &NsReader<&[u8]>,
        event: &quick_xml::events::BytesStart,
    ) -> xml::Result<()> {
        self.profile.total_elements = self
            .profile
            .total_elements
            .checked_add(1)
            .ok_or("XML element cap exceeded")?;
        if self.profile.total_elements > ELEMENT_CAP {
            return Err("XML element cap exceeded".into());
        }
        let depth = self
            .stack
            .len()
            .checked_add(1)
            .ok_or("XML depth cap exceeded")?;
        if depth > DEPTH_CAP {
            return Err("XML depth cap exceeded".into());
        }
        self.profile.max_depth = self.profile.max_depth.max(depth);
        if self.stack.is_empty() && self.root.is_some() {
            return Err("multiple XML roots".into());
        }
        let (local_part, prefix) = event.name().decompose();
        validate_xml_name(local_part.as_ref(), false)?;
        if let Some(prefix) = prefix {
            validate_xml_name(prefix.as_ref(), false)?;
        }
        let (ns, local) = reader.resolver().resolve_element(event.name());
        let namespace = self.resolve_namespace(ns)?;
        let name_id = self.intern_name(namespace, local.as_ref())?;
        let parent_id = self.stack.last().map(|frame| frame.path_id);
        let path_id = self.intern_path(parent_id, name_id)?;
        self.profile.paths[path_id].count = self.profile.paths[path_id]
            .count
            .checked_add(1)
            .ok_or("XML element count overflow")?;
        let mut seen_attributes = HashSet::new();
        for attribute in event.attributes() {
            let attribute = attribute?;
            self.total_attributes = self
                .total_attributes
                .checked_add(1)
                .ok_or("XML attribute cap exceeded")?;
            if self.total_attributes > ATTRIBUTE_CAP {
                return Err("XML attribute cap exceeded".into());
            }
            let (local_part, prefix) = attribute.key.decompose();
            validate_xml_name(local_part.as_ref(), false)?;
            if attribute.key.as_ref() == "xmlns" || attribute.key.as_ref().starts_with("xmlns:") {
                // Every declaration is validated against the shared identity
                // contract, used or unused; used identities are interned on
                // resolution, so nothing is retained here.
                normalize_namespace_identity(attribute.value.as_ref())?;
                continue;
            }
            if let Some(prefix) = prefix {
                validate_xml_name(prefix.as_ref(), false)?;
            }
            // Values are examined transiently for validity, never retained.
            validate_attribute_value(attribute.value.as_ref())?;
            let (ns, local) = reader.resolver().resolve_attribute(attribute.key);
            let namespace = self.resolve_namespace(ns)?;
            let attribute_name = self.intern_name(namespace, local.as_ref())?;
            if !seen_attributes.insert(attribute_name) {
                return Err("duplicate expanded XML attribute".into());
            }
            if !self.profile.paths[path_id]
                .attributes
                .contains_key(&attribute_name)
            {
                self.charge_pair()?;
            }
            let count = self.profile.paths[path_id]
                .attributes
                .entry(attribute_name)
                .or_insert(0);
            *count = count.checked_add(1).ok_or("XML attribute count overflow")?;
        }
        if let Some(parent_path) = self.stack.last().map(|frame| frame.path_id) {
            let top = self.stack.len() - 1;
            let new_child = !self.stack[top].children.contains_key(&path_id);
            if new_child {
                self.live_counters = self
                    .live_counters
                    .checked_add(1)
                    .ok_or("XML live sibling-counter cap exceeded")?;
                if self.live_counters > self.live_cap {
                    return Err("XML live sibling-counter cap exceeded".into());
                }
                self.stack[top].children.insert(path_id, 0);
                if !self.profile.paths[parent_path].children.contains(&path_id) {
                    self.charge_pair()?;
                    self.profile.paths[parent_path].children.push(path_id);
                }
            }
            let parent = &mut self.stack[top];
            let occurrences = parent.children.get_mut(&path_id).expect("charged above");
            *occurrences = occurrences
                .checked_add(1)
                .ok_or("XML child count overflow")?;
            parent.children_total = parent
                .children_total
                .checked_add(1)
                .ok_or("XML child count overflow")?;
        }
        self.stack.push(Frame {
            path_id,
            children: HashMap::new(),
            children_total: 0,
            has_text: false,
        });
        Ok(())
    }

    /// Records transient text presence for the open element without retaining
    /// any value.
    ///
    /// Accepts: the decoded text, CDATA or resolved reference content.
    /// Returns: `Ok(())` after updating the open frame's presence flag.
    /// Errors: "illegal XML character" for content outside the XML 1.0
    /// character production, and "XML text outside root" for non-whitespace
    /// content outside the root element.
    fn text(&mut self, value: &str) -> xml::Result<()> {
        validate_xml_chars(value)?;
        let present = value.chars().any(|c| !c.is_whitespace());
        if let Some(frame) = self.stack.last_mut() {
            frame.has_text |= present;
        } else if present {
            return Err("XML text outside root".into());
        }
        Ok(())
    }

    /// Resolves a namespace result into an interned identity.
    ///
    /// Accepts: the resolver's bound/unbound/unknown result.
    /// Returns: `Some(index)` for a bound namespace, `None` when unbound.
    /// Errors: "undeclared XML namespace prefix" for unknown prefixes, or any
    /// interning cap from `intern_namespace`.
    fn resolve_namespace(&mut self, result: ResolveResult<'_>) -> xml::Result<Option<usize>> {
        match result {
            ResolveResult::Bound(ns) => Ok(Some(self.intern_namespace(ns.as_ref())?)),
            ResolveResult::Unbound => Ok(None),
            ResolveResult::Unknown(_) => Err("undeclared XML namespace prefix".into()),
        }
    }

    /// Handles one element end: merges this instance's child occurrences into
    /// the observed per-parent ranges and releases live counters.
    ///
    /// Accepts: nothing; the top open frame is completed.
    /// Returns: `Ok(())` after the merge.
    /// Errors: "unbalanced XML" when no element is open.
    fn end(&mut self) -> xml::Result<()> {
        let frame = self.stack.pop().ok_or("unbalanced XML")?;
        self.live_counters -= frame.children.len();
        self.profile.max_children = self.profile.max_children.max(frame.children_total);
        self.completed[frame.path_id] = self.completed[frame.path_id]
            .checked_add(1)
            .ok_or("XML parent count overflow")?;
        let entry = &mut self.profile.paths[frame.path_id];
        if frame.has_text {
            entry.text_instances += 1;
        }
        if frame.children_total > 0 {
            entry.children_instances += 1;
        }
        if frame.has_text && frame.children_total > 0 {
            entry.mixed_instances += 1;
        }
        for index in 0..self.profile.paths[frame.path_id].children.len() {
            let child = self.profile.paths[frame.path_id].children[index];
            let observed = frame.children.get(&child).copied().unwrap_or(0);
            let entry = &mut self.profile.paths[child];
            entry.max_occurs = entry.max_occurs.max(observed);
            entry.min_occurs = entry.min_occurs.min(observed);
        }
        if self.stack.is_empty() {
            self.root = Some(frame.path_id);
        }
        Ok(())
    }

    /// Completes the profile after a clean end of input: root identity,
    /// parents_seen from completed parent instances and repeated paths.
    ///
    /// Accepts: nothing; consumes the accumulator.
    /// Returns: the finished `XmlProfile`.
    /// Errors: "missing XML root" when no root closed, "truncated XML" when
    /// elements remain open.
    fn finish(mut self) -> xml::Result<XmlProfile> {
        if !self.stack.is_empty() {
            return Err("truncated XML".into());
        }
        let root = self.root.ok_or("missing XML root")?;
        self.profile.root_path_id = root;
        for id in 0..self.profile.paths.len() {
            let parent = self.profile.paths[id].parent_id;
            self.profile.paths[id].parents_seen = match parent {
                Some(parent) => self.completed[parent],
                None => 1,
            };
        }
        self.profile.paths[root].min_occurs = 1;
        self.profile.paths[root].max_occurs = 1;
        self.profile.repeated_paths = self
            .profile
            .paths
            .iter()
            .filter(|path| path.max_occurs >= 2)
            .map(|path| path.id)
            .collect();
        Ok(self.profile)
    }
}

/// Validates one processing instruction: the target must be a legal XML Name
/// other than the reserved "xml" in any case, and the content must satisfy
/// the XML 1.0 character production. Targets are examined transiently.
///
/// Accepts: the processing-instruction event from the bounded reader.
/// Returns: `Ok(())` for a well-formed processing instruction.
/// Errors: static "reserved XML processing-instruction target", "illegal XML
/// name", or "illegal XML character".
fn validate_pi(event: &quick_xml::events::BytesPI<'_>) -> xml::Result<()> {
    let target = event.target();
    if target.eq_ignore_ascii_case("xml") {
        return Err("reserved XML processing-instruction target".into());
    }
    validate_xml_name(target, true)?;
    validate_xml_chars(event.content())
}

/// Validates one name component against the XML 1.0 (Fifth Edition) Name
/// production, including non-ASCII legal characters. `allow_colon` is set
/// only for processing-instruction targets, where the grammar permits it;
/// element and attribute components are namespace-aware NCNames where the
/// resolver owns the single prefix/local split.
///
/// Accepts: the raw local name, prefix or PI target; never retained.
/// Returns: `Ok(())` for a well-formed name.
/// Errors: static "illegal XML name" for empty names, illegal start
/// characters, or illegal subsequent characters.
fn validate_xml_name(name: &str, allow_colon: bool) -> xml::Result<()> {
    let mut chars = name.chars();
    let first = chars.next().ok_or("illegal XML name")?;
    if !is_name_start_char(first, allow_colon) || !chars.all(|c| is_name_char(c, allow_colon)) {
        return Err("illegal XML name".into());
    }
    Ok(())
}

/// XML 1.0 (Fifth Edition) NameStartChar production.
fn is_name_start_char(c: char, allow_colon: bool) -> bool {
    matches!(
        c,
        'A'..='Z' | 'a'..='z' | '_' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}'
    ) || (allow_colon && c == ':')
}

/// XML 1.0 (Fifth Edition) NameChar production.
fn is_name_char(c: char, allow_colon: bool) -> bool {
    is_name_start_char(c, allow_colon)
        || matches!(
            c,
            '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'
        )
}

/// Normalizes a namespace identity with XML attribute-value semantics —
/// reference expansion and literal-whitespace mapping — and validates the
/// XML 1.0 character production on the result. Used for every declaration,
/// used or unused, and again when a used identity is interned; the transient
/// normalized form is retained only by `Accumulator::intern_namespace`.
///
/// Accepts: the raw non-normalized namespace string from the declaration.
/// Returns: the normalized identity when every expanded character is legal.
/// Errors: static "unsupported XML entity reference in namespace name" for
/// unknown or malformed references, and "illegal XML character" when the
/// normalized identity contains a character outside the production.
fn normalize_namespace_identity(raw: &str) -> xml::Result<String> {
    let expanded = if raw.contains('&') {
        quick_xml::escape::unescape(raw)
            .map_err(|_| "unsupported XML entity reference in namespace name")?
            .into_owned()
    } else {
        raw.to_string()
    };
    let normalized: String = expanded
        .chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    validate_xml_chars(&normalized)?;
    Ok(normalized)
}

/// Validates one ordinary attribute value in the normalized form XML gives
/// semantics to: references are expanded first, then the XML 1.0 character
/// production is checked on the expanded value. The value is examined
/// transiently and never retained.
///
/// Accepts: the raw non-normalized attribute value from the lexer.
/// Returns: `Ok(())` when the expanded value is legal XML 1.0 content.
/// Errors: static "unsupported XML entity reference in attribute value" for
/// unknown or malformed references, and "illegal XML character" when the
/// expanded value contains a character outside the production.
fn validate_attribute_value(raw: &str) -> xml::Result<()> {
    let expanded = if raw.contains('&') {
        quick_xml::escape::unescape(raw)
            .map_err(|_| "unsupported XML entity reference in attribute value")?
    } else {
        std::borrow::Cow::Borrowed(raw)
    };
    validate_xml_chars(&expanded)
}

/// Validates the XML 1.0 character production (tab, LF, CR, U+0020..U+D7FF,
/// U+E000..U+FFFD, U+10000..U+10FFFF) on transiently examined content.
///
/// Accepts: decoded text, CDATA, resolved references or raw attribute values.
/// Returns: `Ok(())` when every character is legal XML 1.0.
/// Errors: static "illegal XML character" on the first illegal character.
fn validate_xml_chars(value: &str) -> xml::Result<()> {
    if value.chars().any(|c| {
        !matches!(
            c,
            '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
        )
    }) {
        return Err("illegal XML character".into());
    }
    Ok(())
}

/// Bounded sink that counts serialized bytes and fails past the profile cap,
/// so the serialized size is validated without an unbounded buffer.
struct CountingWriter {
    remaining: usize,
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if buffer.len() > self.remaining {
            return Err(std::io::Error::other(
                "XML profile serialization cap exceeded",
            ));
        }
        self.remaining -= buffer.len();
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Parses the bounded input and accumulates the structural profile.
///
/// Accepts: the UTF-8 (or UTF-8 BOM) document bytes.
/// Returns: the validated `XmlProfile`.
/// Errors: static content-free messages for malformed input, DTDs,
/// unsupported encodings/entities, multiple/truncated roots and every cap.
fn profile(bytes: &[u8]) -> xml::Result<XmlProfile> {
    accumulate(bytes, Accumulator::default())
}

/// Event loop over the bounded input into the supplied accumulator; split
/// from `profile` so reduced-limit unit seams share the exact parse path.
fn accumulate(bytes: &[u8], mut accumulator: Accumulator) -> xml::Result<XmlProfile> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let version = quick_xml::XmlVersion::default();
    // The prolog declaration is legal only as the very first event.
    let mut decl_allowed = true;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => accumulator.start(&reader, &event)?,
            Ok(Event::End(_)) => accumulator.end()?,
            Ok(Event::Text(event)) => accumulator.text(&event.xml_content(version))?,
            Ok(Event::CData(event)) => accumulator.text(&event.xml_content(version))?,
            Ok(Event::GeneralRef(event)) => {
                let value = if let Some(ch) = event
                    .resolve_char_ref()
                    .map_err(|_| "malformed XML character reference")?
                {
                    ch.to_string()
                } else {
                    match event.xml_content(version).as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err("unsupported XML entity reference".into()),
                    }
                    .to_string()
                };
                accumulator.text(&value)?;
            }
            Ok(Event::DocType(_)) => return Err("XML DTDs are prohibited".into()),
            Ok(Event::Decl(event)) => {
                if !decl_allowed {
                    return Err("misplaced XML declaration".into());
                }
                validate_decl(&event)?;
            }
            Ok(Event::Comment(event)) => {
                validate_xml_chars(&event.xml_content(version))?;
            }
            Ok(Event::PI(event)) => validate_pi(&event)?,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(format!(
                    "malformed XML near byte offset {}",
                    reader.buffer_position()
                )
                .into());
            }
        }
        decl_allowed = false;
    }
    accumulator.finish()
}

/// Validates the prolog declaration against the exact XML 1.0 grammar: the
/// required 1.0 version attribute first, then optional UTF-8 encoding, then
/// optional yes/no standalone, with no unknown, duplicated or misordered
/// attributes. Iterates the built-in declaration attributes; all failures
/// are static and content-free.
///
/// Accepts: the declaration event from the bounded reader.
/// Returns: `Ok(())` for exactly one well-formed XML 1.0 declaration.
/// Errors: static messages for unknown, duplicated or misordered attributes,
/// a missing or non-1.0 version, an unsupported encoding, or an invalid
/// standalone value.
fn validate_decl(event: &quick_xml::events::BytesDecl<'_>) -> xml::Result<()> {
    // Ranks encode the required attribute order: version, encoding, standalone.
    // The declaration content is attribute syntax; the built-in start-tag
    // iterator parses it, so no declaration-specific lexer is introduced.
    // The reader wraps declaration content including the `xml` target name
    // (name_len 3) and the closing `?`; skip both before attribute iteration.
    let raw: &str = event.as_ref();
    let decl = raw.strip_suffix('?').unwrap_or(raw).trim_end();
    let content = quick_xml::events::BytesStart::from_content(decl, 3);
    let mut last_rank = -1;
    let mut seen = [false; 3];
    for attribute in content.attributes() {
        let attribute = attribute.map_err(|_| "malformed XML declaration")?;
        let rank = match attribute.key.as_ref() {
            "version" => 0,
            "encoding" => 1,
            "standalone" => 2,
            _ => return Err("unexpected XML declaration attribute".into()),
        };
        if seen[rank] {
            return Err("duplicate XML declaration attribute".into());
        }
        if (rank as i64) < last_rank {
            return Err("misordered XML declaration attributes".into());
        }
        seen[rank] = true;
        last_rank = rank as i64;
        match rank {
            0 if attribute.value.as_ref() != "1.0" => {
                return Err("unsupported XML version".into());
            }
            1 if !attribute.value.as_ref().eq_ignore_ascii_case("utf-8") => {
                return Err("unsupported XML encoding".into());
            }
            2 if !matches!(attribute.value.as_ref(), "yes" | "no") => {
                return Err("invalid XML standalone value".into());
            }
            _ => {}
        }
    }
    if !seen[0] {
        return Err("missing XML version".into());
    }
    Ok(())
}

/// Renders the display form of an interned name: a synthetic `ns{i}:` marker
/// for namespaced names, otherwise the bare local name.
///
/// Accepts: the profile dictionaries and the name index.
/// Returns: the value-free display string.
/// Errors: none; indexes are valid by construction.
fn display_name(profile: &XmlProfile, name_id: usize) -> String {
    let name = &profile.names[name_id];
    match name.namespace {
        Some(namespace) => format!("ns{namespace}:{}", name.local),
        None => name.local.clone(),
    }
}

/// Builds the compact structural hierarchy/count summary from the finished
/// profile. Names and namespace URIs are structural metadata; no document
/// values ever enter the output. The result is clipped to the 64 KiB summary
/// budget and the flag reports that clip honestly.
///
/// Accepts: the validated profile.
/// Returns: the summary string plus whether the summary budget clipped it.
/// Errors: none; the walk is iterative over the bounded path table.
fn summary(profile: &XmlProfile) -> (String, bool) {
    let mut out = format!(
        "# XML structure\n\n- Elements: {}\n- Maximum depth: {}\n- Maximum children: {}\n- Namespaces used: {}\n",
        profile.total_elements,
        profile.max_depth,
        profile.max_children,
        profile.namespaces.len()
    );
    if !profile.namespaces.is_empty() {
        out.push_str("\n## Namespaces\n\n");
        for (index, uri) in profile.namespaces.iter().enumerate() {
            out.push_str(&format!("- ns{index}: {}\n", markdown::escape(uri)));
        }
    }
    out.push('\n');
    let mut walk = vec![(profile.root_path_id, 0usize)];
    // Track the running size so accumulation stops as soon as the summary
    // budget is exceeded; only the in-budget prefix is ever materialized.
    let mut chars = out.chars().count();
    if chars > SUMMARY_CAP {
        return (out.chars().take(SUMMARY_CAP).collect(), true);
    }
    let mut clipped = false;
    while let Some((path_id, depth)) = walk.pop() {
        let path = &profile.paths[path_id];
        let mut line = format!(
            "{}- {} \u{d7}{}",
            "  ".repeat(depth),
            markdown::escape(&display_name(profile, path.name_id)),
            path.count
        );
        if path.min_occurs != 1 || path.max_occurs != 1 {
            line.push_str(&format!(
                " [occurs {}\u{2013}{}]",
                path.min_occurs, path.max_occurs
            ));
        }
        if !path.attributes.is_empty() {
            line.push_str(&format!(" [attrs:{}]", path.attributes.len()));
        }
        if path.mixed_instances > 0 {
            line.push_str(" [mixed]");
        } else if path.text_instances > 0 {
            line.push_str(" [text]");
        }
        line.push('\n');
        chars += line.chars().count();
        if chars > SUMMARY_CAP {
            clipped = true;
            break;
        }
        out.push_str(&line);
        for &child in path.children.iter().rev() {
            walk.push((child, depth + 1));
        }
    }
    if clipped {
        return (out, true);
    }
    (out, false)
}

/// Inspects a standalone XML file: profiles structure only and publishes
/// `report.xml` solely after the full parse and serialized-size check pass.
/// The bounded structural summary renders through the existing text and
/// Markdown mechanisms; their limits clip the summary, never the profile.
///
/// Accepts: the file path, the report under construction and inspect options
/// whose `max_chars` bounds the rendered summaries.
/// Returns: nothing; mutates `report`.
/// Errors: recorded as a content-free "xml" stage error; no profile and no
/// document values are published on failure.
pub(crate) fn inspect(path: &Path, report: &mut InspectReport, options: &InspectOptions) {
    let result = (|| -> xml::Result<()> {
        let mut source = Source::open(path)?;
        if source.len > INPUT_CAP {
            return Err("XML input byte cap exceeded".into());
        }
        let bytes = source.range(0, source.len as usize)?;
        let profile = profile(&bytes)?;
        let mut writer = CountingWriter {
            remaining: PROFILE_BYTE_CAP,
        };
        serde_json::to_writer(&mut writer, &profile)
            .map_err(|_| "XML profile serialization cap exceeded")?;
        let (summary, clipped) = summary(&profile);
        markdown::append(report, &summary, options, "native_structure");
        report.append_text(&summary, options.max_chars);
        report.markdown.truncated |= clipped;
        report.text.truncated |= clipped;
        report.xml = Some(profile);
        Ok(())
    })();
    if let Err(error) = result {
        report.error("xml", error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live sibling-counter cap is unreachable at production scale: open
    /// frames hold distinct child-path counters, bounded by total links, which
    /// the 4,096-path cap keeps at 4,095. A reduced seam proves the guard
    /// itself without touching the production 32,768 limit.
    #[test]
    fn live_sibling_counter_cap_fires_under_reduced_seam() {
        let result = accumulate(
            b"<r><a/><b/><c/><d/></r>",
            Accumulator::with_live_counter_cap(3),
        );
        let error = result.expect_err("fourth distinct sibling must fail");
        assert_eq!(error.to_string(), "XML live sibling-counter cap exceeded");
    }
}
