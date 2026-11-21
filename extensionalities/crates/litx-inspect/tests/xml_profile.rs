//! Wave-1 (XML-CORE) behavioral tests for the standalone XML structural profile.
//! Assertions run against dynamic `serde_json::Value` so a missing `xml`
//! report field fails at runtime instead of becoming a compile error.
use litx_inspect::{InspectOptions, inspect};

/// Synthetic fixture bytes for the first red/green slice: nested and repeated
/// elements, one namespaced child, an attribute, an optional child and mixed
/// text, with unmistakable sentinel data values.
const CATALOG: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<catalog>\n\
\x20 <item id=\"SENTINEL-ID-7F31\">\n\
\x20   <title>SENTINEL-TITLE-QX48</title>\n\
\x20   <meta:note xmlns:meta=\"urn:example:meta\">SENTINEL-NOTE-ZX29</meta:note>\n\
\x20 </item>\n\
\x20 <item id=\"SENTINEL-ID-7F32\">\n\
\x20   <title>SENTINEL-TITLE-QX49</title>\n\
\x20   SENTINEL-MIXED-VT63 <extra>SENTINEL-EXTRA-KQ05</extra>\n\
\x20 </item>\n\
</catalog>\n";

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "xml-profile-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn write_fixture(directory: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn xml_profiles_nested_repeated_structure_without_values() {
    let directory = unique_dir("catalog");
    let path = write_fixture(&directory, "catalog.xml", CATALOG);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert!(xml.is_object(), "xml profile must be published: {v}");
    assert_eq!(xml["total_elements"], 7);
    assert_eq!(xml["max_depth"], 3, "root depth is 1");
    assert_eq!(xml["max_children"], 2);
    assert_eq!(xml["root_path_id"], 0);
    assert_eq!(xml["namespaces"], serde_json::json!(["urn:example:meta"]));
    assert_eq!(
        xml["names"],
        serde_json::json!([
            {"namespace": null, "local": "catalog"},
            {"namespace": null, "local": "item"},
            {"namespace": null, "local": "id"},
            {"namespace": null, "local": "title"},
            {"namespace": 0, "local": "note"},
            {"namespace": null, "local": "extra"}
        ])
    );
    let paths = &xml["paths"];
    // Root: occurrence one, no parent.
    assert_eq!(paths[0]["parent_id"], serde_json::Value::Null);
    assert_eq!(paths[0]["name_id"], 0);
    assert_eq!(paths[0]["count"], 1);
    assert_eq!(paths[0]["min_occurs"], 1);
    assert_eq!(paths[0]["max_occurs"], 1);
    // Repeated sibling group: two item elements under one catalog.
    assert_eq!(paths[1]["parent_id"], 0);
    assert_eq!(paths[1]["count"], 2);
    assert_eq!(paths[1]["parents_seen"], 1);
    assert_eq!(paths[1]["min_occurs"], 2);
    assert_eq!(paths[1]["max_occurs"], 2);
    assert_eq!(paths[1]["attributes"]["2"], 2, "id on both items");
    // Present in every item instance.
    assert_eq!(paths[2]["count"], 2);
    assert_eq!(paths[2]["parents_seen"], 2);
    assert_eq!(paths[2]["min_occurs"], 1);
    assert_eq!(paths[2]["max_occurs"], 1);
    // Optional children: observed zero minimum, never schema-required.
    assert_eq!(paths[3]["count"], 1, "namespaced note");
    assert_eq!(paths[3]["min_occurs"], 0);
    assert_eq!(paths[3]["max_occurs"], 1);
    assert_eq!(paths[4]["count"], 1, "extra");
    assert_eq!(paths[4]["min_occurs"], 0);
    assert_eq!(paths[4]["max_occurs"], 1);
    assert_eq!(xml["repeated_paths"], serde_json::json!([1]));
    // Structure only: sentinel data values must appear nowhere.
    let serialized = serde_json::to_string(&r).unwrap();
    assert!(!serialized.contains("SENTINEL"), "{serialized}");
}

const ALIAS_A: &[u8] = b"<r xmlns:m=\"urn:example:meta\"><m:a><m:b/></m:a></r>";
const ALIAS_B: &[u8] = b"<r xmlns:q=\"urn:example:meta\"><q:a><q:b/></q:a></r>";
const ALIAS_REF: &[u8] = b"<r xmlns:m=\"urn:example:m&#101;ta\"><m:a><m:b/></m:a></r>";
const DEFAULT_VS_ATTRIBUTE: &[u8] =
    b"<d:root xmlns:d=\"urn:example:def\" xmlns=\"urn:example:def\">\n\
\x20 <d:child d:attr=\"SENTINEL-NS-V11\" attr=\"SENTINEL-UQ-V22\"/>\n\
\x20 <plain/>\n\
</d:root>\n";

#[test]
fn xml_namespace_aliases_and_reference_forms_are_equivalent() {
    let directory = unique_dir("alias");
    let mut profiles = Vec::new();
    for (name, bytes) in [
        ("a.xml", ALIAS_A),
        ("b.xml", ALIAS_B),
        ("ref.xml", ALIAS_REF),
    ] {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
        profiles.push(serde_json::to_value(&r).unwrap()["xml"].clone());
    }
    assert_eq!(
        profiles[0], profiles[1],
        "prefix aliases must be equivalent"
    );
    assert_eq!(
        profiles[0], profiles[2],
        "character references in namespace names must normalize away"
    );
    assert_eq!(
        profiles[0]["namespaces"],
        serde_json::json!(["urn:example:meta"])
    );
}

#[test]
fn xml_default_namespace_does_not_apply_to_unqualified_attributes() {
    let directory = unique_dir("default-ns");
    let path = write_fixture(&directory, "doc.xml", DEFAULT_VS_ATTRIBUTE);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert_eq!(
        xml["namespaces"],
        serde_json::json!(["urn:example:def"]),
        "element default and prefix share one identity"
    );
    let names = xml["names"].as_array().unwrap();
    let qualified = names
        .iter()
        .position(|n| n["namespace"] == 0 && n["local"] == "attr")
        .expect("namespaced attribute name");
    let unqualified = names
        .iter()
        .position(|n| n["namespace"].is_null() && n["local"] == "attr")
        .expect("unqualified attribute name must stay distinct");
    assert_ne!(qualified, unqualified);
    let child = xml["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| xml["names"][p["name_id"].as_u64().unwrap() as usize]["local"] == "child")
        .unwrap();
    assert_eq!(child["attributes"][qualified.to_string()], 1);
    assert_eq!(child["attributes"][unqualified.to_string()], 1);
    // The unprefixed element inherits the default namespace.
    let plain = names
        .iter()
        .find(|n| n["local"] == "plain")
        .expect("plain element name");
    assert_eq!(plain["namespace"], 0);
    assert!(!serde_json::to_string(&r).unwrap().contains("SENTINEL"));
}

#[test]
fn xml_long_namespace_reuse_does_not_amplify_storage() {
    let directory = unique_dir("long-ns");
    let uri = format!("urn:example:{}", "a".repeat(3_900));
    let mut document = format!("<m:r xmlns:m=\"{uri}\">");
    for _ in 0..50 {
        document.push_str("<m:row/>");
    }
    document.push_str("</m:r>");
    let path = write_fixture(&directory, "rows.xml", document.as_bytes());
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert_eq!(xml["total_elements"], 51);
    assert_eq!(xml["max_children"], 50);
    assert_eq!(xml["namespaces"].as_array().unwrap().len(), 1);
    assert_eq!(xml["namespaces"][0].as_str().unwrap().len(), uri.len());
    assert_eq!(
        xml["names"].as_array().unwrap().len(),
        2,
        "repeated use must not grow the name dictionary"
    );
    assert_eq!(
        xml["paths"].as_array().unwrap().len(),
        2,
        "repeated use must not grow the path table"
    );
    assert_eq!(xml["repeated_paths"], serde_json::json!([1]));
    assert_eq!(xml["paths"][1]["count"], 50);
    assert_eq!(xml["paths"][1]["min_occurs"], 50);
    assert_eq!(xml["paths"][1]["max_occurs"], 50);
}

#[test]
fn xml_profile_is_deterministic_across_runs() {
    let directory = unique_dir("determinism");
    let path = write_fixture(&directory, "catalog.xml", CATALOG);
    let first =
        serde_json::to_value(inspect(&path, &InspectOptions::default())).unwrap()["xml"].clone();
    let second =
        serde_json::to_value(inspect(&path, &InspectOptions::default())).unwrap()["xml"].clone();
    assert_eq!(first, second);
}

fn path_by_local<'v>(xml: &'v serde_json::Value, local: &str) -> &'v serde_json::Value {
    xml["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| xml["names"][p["name_id"].as_u64().unwrap() as usize]["local"] == local)
        .unwrap_or_else(|| panic!("path for {local}: {xml}"))
}

#[test]
fn xml_whitespace_only_text_is_not_text_presence() {
    let directory = unique_dir("whitespace");
    let path = write_fixture(
        &directory,
        "doc.xml",
        b"<a>\n   <b>SENTINEL-BODY-PL19</b>\n</a>\n",
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    let a = path_by_local(xml, "a");
    assert_eq!(a["text_instances"], 0, "whitespace-only text is absent");
    assert_eq!(a["children_instances"], 1);
    assert_eq!(a["mixed_instances"], 0);
    let b = path_by_local(xml, "b");
    assert_eq!(b["text_instances"], 1);
    assert_eq!(b["children_instances"], 0);
    assert_eq!(b["mixed_instances"], 0);
    assert!(!serde_json::to_string(&r).unwrap().contains("SENTINEL"));
}

#[test]
fn xml_mixed_content_needs_text_and_children_in_one_instance() {
    let directory = unique_dir("mixed");
    let path = write_fixture(
        &directory,
        "doc.xml",
        b"<p>lead <b>SENTINEL-BOLD-DK82</b> tail</p>",
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let p = path_by_local(&v["xml"], "p");
    assert_eq!(p["text_instances"], 1);
    assert_eq!(p["children_instances"], 1);
    assert_eq!(p["mixed_instances"], 1);
    let b = path_by_local(&v["xml"], "b");
    assert_eq!(b["mixed_instances"], 0);
    assert!(!serde_json::to_string(&r).unwrap().contains("SENTINEL"));
}

#[test]
fn xml_cdata_and_character_references_count_as_text_presence() {
    let directory = unique_dir("references");
    let cases: [(&str, &[u8], u64); 4] = [
        ("cdata.xml", b"<a><![CDATA[SENTINEL-CDATA-WQ77]]></a>", 1),
        ("charref.xml", b"<a>&#65;</a>", 1),
        ("whitespace-refs.xml", b"<a>&#32;&#9;</a>", 0),
        ("predefined.xml", b"<a>&amp;</a>", 1),
    ];
    for (name, bytes, expected) in cases {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            path_by_local(&v["xml"], "a")["text_instances"],
            expected,
            "{name}"
        );
        assert!(
            !serde_json::to_string(&r).unwrap().contains("SENTINEL"),
            "{name}"
        );
    }
}

#[test]
fn xml_unsupported_entity_fails_closed_without_values() {
    let directory = unique_dir("entity");
    let path = write_fixture(&directory, "doc.xml", b"<a>SENTINEL-ENT-ZM41&nbsp;</a>");
    let r = inspect(&path, &InspectOptions::default());
    let error = r
        .errors
        .iter()
        .find(|e| e.stage == "xml")
        .unwrap_or_else(|| panic!("xml stage error required: {:?}", r.errors));
    assert!(
        !error.message.contains("nbsp") && !error.message.contains("SENTINEL"),
        "content-free diagnostics only: {}",
        error.message
    );
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["xml"].is_null(), "no profile on failure: {v}");
    assert!(r.markdown.truncated);
}

#[test]
fn xml_summary_describes_structure_without_raw_markup_or_values() {
    let directory = unique_dir("summary");
    let path = write_fixture(&directory, "catalog.xml", CATALOG);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!r.markdown.truncated, "{:?}", r.markdown);
    let markdown = &r.markdown.content;
    assert!(markdown.contains("# XML structure"), "{markdown}");
    assert!(markdown.contains("catalog"), "{markdown}");
    assert!(markdown.contains("item"), "{markdown}");
    assert!(markdown.contains("ns0:note"), "{markdown}");
    assert!(markdown.contains("occurs 2–2"), "{markdown}");
    assert!(markdown.contains("ns0: urn:example:meta"), "{markdown}");
    assert!(markdown.contains("mixed"), "{markdown}");
    assert!(!markdown.contains("SENTINEL"), "{markdown}");
    assert!(!markdown.contains("<catalog"), "{markdown}");
    assert!(!markdown.contains("<item"), "{markdown}");
    let text = &r.text.content;
    assert!(text.contains("XML structure"), "{text}");
    assert!(text.contains("catalog"), "{text}");
    assert!(!text.contains("SENTINEL"), "{text}");
    assert!(!text.contains("<catalog"), "{text}");
}

#[test]
fn xml_summary_truncation_is_honest_and_the_profile_stays_complete() {
    let directory = unique_dir("clipped");
    let mut document = String::from("<r>");
    for index in 0..60 {
        document.push_str(&format!("<row{index}/>"));
    }
    document.push_str("</r>");
    let path = write_fixture(&directory, "rows.xml", document.as_bytes());
    let options = InspectOptions {
        max_chars: 300,
        ..Default::default()
    };
    let r = inspect(&path, &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.markdown.truncated, "clipped summary must set the flag");
    assert!(r.text.truncated, "clipped text summary must set the flag");
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(
        v["xml"]["total_elements"], 61,
        "summary budgets never clip the structural profile"
    );
    assert_eq!(v["xml"]["paths"].as_array().unwrap().len(), 61);
}

#[test]
fn xml_profiles_empty_root_and_empty_sibling_groups() {
    let directory = unique_dir("empty");
    let path = write_fixture(&directory, "root.xml", b"<root/>");
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert_eq!(xml["total_elements"], 1);
    assert_eq!(xml["max_depth"], 1);
    assert_eq!(xml["max_children"], 0);
    assert_eq!(xml["paths"].as_array().unwrap().len(), 1);
    assert_eq!(xml["paths"][0]["count"], 1);
    assert_eq!(xml["paths"][0]["min_occurs"], 1);
    assert_eq!(xml["paths"][0]["max_occurs"], 1);
    assert_eq!(xml["paths"][0]["text_instances"], 0);
    assert_eq!(xml["paths"][0]["children_instances"], 0);
    assert_eq!(xml["repeated_paths"], serde_json::json!([]));

    let path = write_fixture(&directory, "siblings.xml", b"<root><a/><a/><a/></root>");
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert_eq!(xml["total_elements"], 4);
    assert_eq!(xml["max_children"], 3);
    assert_eq!(xml["paths"][1]["count"], 3);
    assert_eq!(xml["paths"][1]["min_occurs"], 3);
    assert_eq!(xml["paths"][1]["max_occurs"], 3);
    assert_eq!(xml["repeated_paths"], serde_json::json!([1]));
}

#[test]
fn xml_accepts_utf8_bom() {
    let directory = unique_dir("bom");
    let path = write_fixture(&directory, "bom.xml", b"\xEF\xBB\xBF<root/>");
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["total_elements"], 1);
}

#[test]
fn xml_malformed_and_multiple_roots_fail_closed() {
    let directory = unique_dir("invalid");
    for (name, bytes) in [
        ("truncated.xml", &b"<root><unclosed>"[..]),
        ("roots.xml", &b"<a/><b/>"[..]),
        ("stray.xml", &b"<a/>stray text"[..]),
    ] {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        let error = r
            .errors
            .iter()
            .find(|e| e.stage == "xml")
            .unwrap_or_else(|| panic!("{name}: xml stage error required: {:?}", r.errors));
        assert!(
            !error.message.contains("unclosed")
                && !error.message.contains("stray")
                && !error.message.contains("<a"),
            "{name}: content-free diagnostics only: {}",
            error.message
        );
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["xml"].is_null(), "{name}: no profile on failure");
        assert!(r.markdown.truncated, "{name}");
    }
}

#[test]
fn xml_rejects_unsupported_encodings() {
    let directory = unique_dir("encoding");
    let path = write_fixture(
        &directory,
        "latin.xml",
        b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a/>",
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.iter().any(|e| e.stage == "xml"), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["xml"].is_null(), "no profile on unsupported encoding");
}

#[test]
fn xml_extension_does_not_override_byte_verified_formats() {
    let directory = unique_dir("dispatch");
    // Byte-verified PNG bytes inside an .xml name keep image dispatch.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0u8; 64]);
    let path = write_fixture(&directory, "data.xml", &png);
    let r = inspect(&path, &InspectOptions::default());
    let v = serde_json::to_value(&r).unwrap();
    assert!(
        v["xml"].is_null(),
        "XML must not steal byte-verified bytes: {v}"
    );
    assert!(!r.errors.iter().any(|e| e.stage == "xml"), "{:?}", r.errors);
    // SVG is a specialized format, never generic XML input.
    let path = write_fixture(
        &directory,
        "image.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"></svg>",
    );
    let r = inspect(&path, &InspectOptions::default());
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["xml"].is_null(), "XML must not claim SVG: {v}");
    assert!(!r.errors.iter().any(|e| e.stage == "xml"), "{:?}", r.errors);
}

/// Asserts the fail-closed contract for one invalid input: an xml-stage
/// error with a content-free message, no published profile, and honest
/// truncation flags.
fn assert_fails_closed(r: &litx_inspect::InspectReport, label: &str, sentinel: &str) {
    let error = r
        .errors
        .iter()
        .find(|e| e.stage == "xml")
        .unwrap_or_else(|| panic!("{label}: xml stage error required: {:?}", r.errors));
    assert!(
        !error.message.contains(sentinel) && !error.message.contains('<'),
        "{label}: content-free diagnostics only: {}",
        error.message
    );
    let v = serde_json::to_value(r).unwrap();
    assert!(v["xml"].is_null(), "{label}: no profile on failure");
    assert!(
        !serde_json::to_string(r).unwrap().contains(sentinel),
        "{label}"
    );
    assert!(r.markdown.truncated, "{label}");
}

#[test]
fn xml_dtds_internal_and_external_are_prohibited() {
    let directory = unique_dir("dtd");
    let cases: [(&str, &[u8]); 2] = [
        (
            "internal.xml",
            b"<!DOCTYPE a [<!ENTITY x \"SENTINEL-DTD-E1\">]><a>&x;</a>",
        ),
        (
            "external.xml",
            b"<!DOCTYPE a SYSTEM \"http://example.invalid/x.dtd\"><a>SENTINEL-DTD-E2</a>",
        ),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        assert_fails_closed(
            &inspect(&path, &InspectOptions::default()),
            name,
            "SENTINEL",
        );
    }
}

#[test]
fn xml_namespace_and_attribute_integrity_fail_closed() {
    let directory = unique_dir("integrity");
    let cases: [(&str, &[u8]); 2] = [
        ("unknown-prefix.xml", b"<u:a>SENTINEL-NS-U1</u:a>"),
        (
            "duplicate-expanded.xml",
            b"<a xmlns:p=\"urn:z\" xmlns:q=\"urn:z\" p:x=\"SENTINEL-DA-1\" q:x=\"SENTINEL-DA-2\"/>",
        ),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        assert_fails_closed(
            &inspect(&path, &InspectOptions::default()),
            name,
            "SENTINEL",
        );
    }
}

#[test]
fn xml_bad_references_and_illegal_characters_fail_closed() {
    let directory = unique_dir("badref");
    let cases: [(&str, &[u8]); 5] = [
        ("surrogate.xml", b"<a>&#xD800;</a>"),
        ("null-ref.xml", b"<a>&#0;</a>"),
        ("control.xml", b"<a>\x01</a>"),
        ("control-attr.xml", b"<a b=\"\x01\">x</a>"),
        ("utf16.xml", b"\xFF\xFE<\x00a\x00/\x00>\x00"),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        assert_fails_closed(
            &inspect(&path, &InspectOptions::default()),
            name,
            "SENTINEL",
        );
    }
}

#[test]
fn xml_rejects_declared_utf16_encoding() {
    let directory = unique_dir("utf16-decl");
    let path = write_fixture(
        &directory,
        "decl.xml",
        b"<?xml version=\"1.0\" encoding=\"UTF-16\"?><a/>",
    );
    assert_fails_closed(
        &inspect(&path, &InspectOptions::default()),
        "utf16-decl",
        "SENTINEL",
    );
}

#[test]
fn xml_session_recovers_after_invalid_input_without_state_leakage() {
    let directory = unique_dir("recovery");
    let invalid = write_fixture(&directory, "bad.xml", b"<a>SENTINEL-LEAK-R1&oopsy;</a>");
    let valid = write_fixture(&directory, "good.xml", CATALOG);
    let mut inspector = litx_inspect::Inspector::default();
    let first = inspector.inspect(&invalid, &InspectOptions::default());
    assert_fails_closed(&first, "first-invalid", "SENTINEL");
    let second = inspector.inspect(&valid, &InspectOptions::default());
    assert!(second.errors.is_empty(), "{:?}", second.errors);
    let v = serde_json::to_value(&second).unwrap();
    assert_eq!(v["xml"]["total_elements"], 7);
    assert!(!serde_json::to_string(&second).unwrap().contains("SENTINEL"));
}

/// Asserts one over-limit input fails closed with the expected static cap
/// message, no profile, and no retained values.
fn assert_cap_error(r: &litx_inspect::InspectReport, label: &str, message_part: &str) {
    let error = r
        .errors
        .iter()
        .find(|e| e.stage == "xml")
        .unwrap_or_else(|| panic!("{label}: xml stage error required: {:?}", r.errors));
    assert!(
        error.message.contains(message_part),
        "{label}: expected '{message_part}': {}",
        error.message
    );
    assert!(
        !error.message.contains('<'),
        "{label}: content-free diagnostics: {}",
        error.message
    );
    let v = serde_json::to_value(r).unwrap();
    assert!(v["xml"].is_null(), "{label}: no profile over the cap");
    assert!(r.markdown.truncated, "{label}");
}

#[test]
fn xml_input_byte_cap_at_and_over() {
    let directory = unique_dir("cap-input");
    let build = |target: usize| -> Vec<u8> {
        let mut doc = Vec::with_capacity(target);
        doc.extend_from_slice(b"<root>");
        doc.resize(target - b"</root>".len(), b' ');
        doc.extend_from_slice(b"</root>");
        doc
    };
    let at = write_fixture(&directory, "at.xml", &build(16 << 20));
    let r = inspect(&at, &InspectOptions::default());
    assert!(r.errors.is_empty(), "at-limit must pass: {:?}", r.errors);
    let over = write_fixture(&directory, "over.xml", &build((16 << 20) + 1));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "input-over",
        "input byte cap",
    );
}

#[test]
fn xml_depth_cap_at_and_over() {
    let directory = unique_dir("cap-depth");
    let nested = |depth: usize| -> Vec<u8> {
        let mut doc = b"<a>".repeat(depth);
        doc.extend_from_slice(&b"</a>".repeat(depth));
        doc
    };
    let at = write_fixture(&directory, "at.xml", &nested(128));
    let r = inspect(&at, &InspectOptions::default());
    assert!(r.errors.is_empty(), "depth 128 must pass: {:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["max_depth"], 128);
    let over = write_fixture(&directory, "over.xml", &nested(129));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "depth-over",
        "depth cap",
    );
}

#[test]
fn xml_element_cap_at_and_over() {
    let directory = unique_dir("cap-elements");
    let doc = |children: usize| -> Vec<u8> {
        let mut doc = b"<r>".to_vec();
        doc.extend_from_slice(&b"<e/>".repeat(children));
        doc.extend_from_slice(b"</r>");
        doc
    };
    let at = write_fixture(&directory, "at.xml", &doc(199_999));
    let r = inspect(&at, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "200,000 elements must pass: {:?}",
        r.errors
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["total_elements"], 200_000);
    let over = write_fixture(&directory, "over.xml", &doc(200_000));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "element-over",
        "element cap",
    );
}

#[test]
fn xml_attribute_cap_at_and_over() {
    let directory = unique_dir("cap-attributes");
    let doc = |elements: usize| -> Vec<u8> {
        let mut doc = b"<r>".to_vec();
        doc.extend_from_slice(&b"<e a=\"1\" b=\"2\"/>".repeat(elements));
        doc.extend_from_slice(b"</r>");
        doc
    };
    let at = write_fixture(&directory, "at.xml", &doc(100_000));
    let r = inspect(&at, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "200,000 attributes must pass: {:?}",
        r.errors
    );
    let over = write_fixture(&directory, "over.xml", &doc(100_001));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "attribute-over",
        "attribute cap",
    );
}

#[test]
fn xml_unique_name_cap_at_and_over() {
    let directory = unique_dir("cap-names");
    let doc = |children: usize| -> Vec<u8> {
        let mut doc = b"<r>".to_vec();
        for index in 0..children {
            doc.extend_from_slice(format!("<n{index}/>").as_bytes());
        }
        doc.extend_from_slice(b"</r>");
        doc
    };
    // 4,095 distinct child names plus the root: exactly 4,096 names and paths.
    let at = write_fixture(&directory, "at.xml", &doc(4_095));
    let r = inspect(&at, &InspectOptions::default());
    assert!(r.errors.is_empty(), "4,096 names must pass: {:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["names"].as_array().unwrap().len(), 4_096);
    assert_eq!(v["xml"]["paths"].as_array().unwrap().len(), 4_096);
    let over = write_fixture(&directory, "over.xml", &doc(4_096));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "name-over",
        "unique-name cap",
    );
}

#[test]
fn xml_namespace_cap_at_and_over() {
    let directory = unique_dir("cap-namespaces");
    let doc = |children: usize| -> Vec<u8> {
        let mut doc = b"<p0:r xmlns:p0=\"urn:n0\">".to_vec();
        for index in 1..=children {
            doc.extend_from_slice(
                format!("<p{index}:e xmlns:p{index}=\"urn:n{index}\"/>").as_bytes(),
            );
        }
        doc.extend_from_slice(b"</p0:r>");
        doc
    };
    // Root namespace plus 4,095 child namespaces: exactly 4,096.
    let at = write_fixture(&directory, "at.xml", &doc(4_095));
    let r = inspect(&at, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "4,096 namespaces must pass: {:?}",
        r.errors
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["namespaces"].as_array().unwrap().len(), 4_096);
    let over = write_fixture(&directory, "over.xml", &doc(4_096));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "namespace-over",
        "namespace cap",
    );
}

#[test]
fn xml_unique_path_cap_at_and_over_with_shared_names() {
    let directory = unique_dir("cap-paths");
    // Full binary tree over two shared names: 2^(d+1)-1 paths with 3 names,
    // proving the path cap is reachable independently of the name cap.
    let tree = |depth: usize| -> Vec<u8> {
        fn open(out: &mut Vec<u8>, name: &str, depth: usize) {
            out.extend_from_slice(format!("<{name}>").as_bytes());
            if depth > 0 {
                open(out, "a", depth - 1);
                open(out, "b", depth - 1);
            }
            out.extend_from_slice(format!("</{name}>").as_bytes());
        }
        let mut doc = Vec::new();
        open(&mut doc, "a", depth);
        doc
    };
    // Depth 11: 4,095 paths, 4,095 elements.
    let at = write_fixture(&directory, "at.xml", &tree(11));
    let r = inspect(&at, &InspectOptions::default());
    assert!(r.errors.is_empty(), "4,095 paths must pass: {:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["paths"].as_array().unwrap().len(), 4_095);
    assert_eq!(v["xml"]["names"].as_array().unwrap().len(), 2);
    // Depth 12: 8,191 paths exceed the 4,096-path cap.
    let over = write_fixture(&directory, "over.xml", &tree(12));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "path-over",
        "unique-path cap",
    );
}

#[test]
fn xml_retained_byte_cap_below_and_over() {
    let directory = unique_dir("cap-retained");
    let doc = |names: usize| -> Vec<u8> {
        let mut doc = b"<r>".to_vec();
        for index in 0..names {
            let name = format!("n{index:0>1022}");
            doc.extend_from_slice(format!("<{name}/>").as_bytes());
        }
        doc.extend_from_slice(b"</r>");
        doc
    };
    // 800 x 1,023-byte names plus root: 818,401 retained bytes, below both
    // the retained cap and the separate 1 MiB serialized-profile cap.
    let below = write_fixture(&directory, "below.xml", &doc(800));
    let r = inspect(&below, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "below the 1 MiB retained cap: {:?}",
        r.errors
    );
    // 1,026 x 1,023-byte names plus root: 1,049,599 retained bytes.
    let over = write_fixture(&directory, "over.xml", &doc(1_026));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "retained-over",
        "retained-name byte cap",
    );
}

#[test]
fn xml_individual_name_and_namespace_caps_at_and_over() {
    let directory = unique_dir("cap-individual");
    let local = |len: usize| -> Vec<u8> {
        let name = format!("n{:0>width$}", "", width = len - 1);
        format!("<{name}/>").into_bytes()
    };
    let at = write_fixture(&directory, "name-at.xml", &local(1_024));
    let r = inspect(&at, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "1 KiB local name must pass: {:?}",
        r.errors
    );
    let over = write_fixture(&directory, "name-over.xml", &local(1_025));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "local-name-over",
        "local-name cap",
    );

    let uri_doc = |len: usize| -> Vec<u8> {
        let uri = format!("urn:{:0>width$}", "", width = len - 4);
        format!("<p:a xmlns:p=\"{uri}\"/>").into_bytes()
    };
    let at = write_fixture(&directory, "uri-at.xml", &uri_doc(4_096));
    let r = inspect(&at, &InspectOptions::default());
    assert!(
        r.errors.is_empty(),
        "4 KiB namespace URI must pass: {:?}",
        r.errors
    );
    let over = write_fixture(&directory, "uri-over.xml", &uri_doc(4_097));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "uri-over",
        "namespace URI cap",
    );
}

#[test]
fn xml_serialized_profile_cap_fails_closed() {
    let directory = unique_dir("cap-serialized");
    // 4,095 distinct 256-byte names: retained bytes and name/path caps all
    // hold, but the serialized JSON profile exceeds the 1 MiB output cap.
    let mut doc = b"<r>".to_vec();
    for index in 0..4_095usize {
        let name = format!("m{index:0>252}");
        doc.extend_from_slice(format!("<{name}/>").as_bytes());
    }
    doc.extend_from_slice(b"</r>");
    let path = write_fixture(&directory, "over.xml", &doc);
    assert_cap_error(
        &inspect(&path, &InspectOptions::default()),
        "serialized-over",
        "profile serialization cap",
    );
}

#[test]
fn xml_summary_cap_clips_honestly_above_the_render_budget() {
    let directory = unique_dir("cap-summary");
    let mut doc = b"<r>".to_vec();
    // 24-byte names keep every line near 30 chars: 4,096 paths render about
    // 123K chars of summary, above the 64 KiB structural-summary cap.
    for index in 0..4_095usize {
        doc.extend_from_slice(format!("<n{index:0>23}/>").as_bytes());
    }
    doc.extend_from_slice(b"</r>");
    let path = write_fixture(&directory, "wide.xml", &doc);
    // A render budget above the 64 KiB structural-summary cap isolates the
    // summary cap itself: flags must still report honest truncation and the
    // profile must stay complete.
    let options = InspectOptions {
        max_chars: 1_000_000,
        ..Default::default()
    };
    let r = inspect(&path, &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.markdown.truncated, "64 KiB summary cap must set the flag");
    assert!(r.text.truncated);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["paths"].as_array().unwrap().len(), 4_096);
}

#[test]
fn xml_nested_wide_document_does_not_amplify_paths_or_names() {
    let directory = unique_dir("nested-wide");
    // 64 levels of <a>, each holding 63 repeated <b> siblings: the same two
    // names recur, so paths grow only linearly with depth.
    let mut doc = String::from("<r>");
    for _ in 0..64 {
        doc.push_str("<a>");
        doc.push_str(&"<b/>".repeat(63));
    }
    doc.push_str(&"</a>".repeat(64));
    doc.push_str("</r>");
    let path = write_fixture(&directory, "nested.xml", doc.as_bytes());
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let xml = &v["xml"];
    assert_eq!(xml["total_elements"], 1 + 64 + 64 * 63);
    assert_eq!(xml["max_depth"], 66);
    assert_eq!(xml["names"].as_array().unwrap().len(), 3);
    assert_eq!(
        xml["paths"].as_array().unwrap().len(),
        1 + 2 * 64,
        "paths stay linear in depth despite 4,032 repeated siblings"
    );
}

#[test]
fn xml_token_and_unit_limits_clip_summaries_only() {
    let directory = unique_dir("clipping");
    let path = write_fixture(&directory, "catalog.xml", CATALOG);
    let tokens = InspectOptions {
        max_tokens: Some(10),
        ..Default::default()
    };
    let r = inspect(&path, &tokens);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.text.truncated, "token budget clips the text summary");
    assert!(
        r.markdown.truncated,
        "token budget clips the markdown summary"
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["total_elements"], 7, "profile stays complete");
    // A page-unit limit never implies a word budget (LITX-AUD-008).
    let pages = InspectOptions {
        max_tokens: None,
        max_pages: Some(1),
        ..Default::default()
    };
    let r = inspect(&path, &pages);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!r.text.truncated, "page limits never clip XML summaries");
    assert!(!r.markdown.truncated);
}

#[test]
fn xml_stat_pair_cap_below_and_over() {
    let directory = unique_dir("cap-pairs");
    // Every child charges one parent-link pair plus four attribute pairs:
    // 1,638 children total 8,190 pairs, inside the 8,192 budget.
    let doc = |children: usize| -> Vec<u8> {
        let mut doc = b"<r>".to_vec();
        for index in 0..children {
            doc.extend_from_slice(
                format!("<c{index} a=\"1\" b=\"2\" d=\"3\" e=\"4\"/>").as_bytes(),
            );
        }
        doc.extend_from_slice(b"</r>");
        doc
    };
    let below = write_fixture(&directory, "below.xml", &doc(1_638));
    let r = inspect(&below, &InspectOptions::default());
    assert!(r.errors.is_empty(), "8,190 pairs must pass: {:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["paths"].as_array().unwrap().len(), 1_639);
    // 1,639 children total 8,195 pairs: the 8,193rd charge fails closed.
    let over = write_fixture(&directory, "over.xml", &doc(1_639));
    assert_cap_error(
        &inspect(&over, &InspectOptions::default()),
        "pair-over",
        "statistic-pair cap",
    );
}

#[test]
fn xml_attribute_values_are_reference_expanded_before_character_validation() {
    let directory = unique_dir("attr-normalize");
    // Positive: predefined references, character references and literal
    // whitespace are legal attribute content once expanded.
    let ok = write_fixture(
        &directory,
        "ok.xml",
        b"<a b=\"x &amp; y &#65; &lt;tag&gt;\tc\n SENTINEL-POS\"/>",
    );
    let r = inspect(&ok, &InspectOptions::default());
    assert!(r.errors.is_empty(), "valid escaped value: {:?}", r.errors);
    assert!(!serde_json::to_string(&r).unwrap().contains("SENTINEL"));
    // Negative: the expanded value, never the raw source spelling, decides
    // legality. Expansion failures and illegal expanded characters fail
    // closed without echoing the offending reference.
    let cases: [(&str, &[u8]); 5] = [
        ("nul-ref.xml", b"<a b=\"SENTINEL-N&#x0;\"/>"),
        ("unknown-ref.xml", b"<a b=\"SENTINEL-X&xxe;\"/>"),
        ("naked-amp.xml", b"<a b=\"SENTINEL-A& raw\"/>"),
        ("empty-ref.xml", b"<a b=\"SENTINEL-E&#;\"/>"),
        ("surrogate-ref.xml", b"<a b=\"SENTINEL-S&#xD800;\"/>"),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        let error = r
            .errors
            .iter()
            .find(|e| e.stage == "xml")
            .unwrap_or_else(|| panic!("{name}: xml stage error required: {:?}", r.errors));
        assert!(
            !error.message.contains("SENTINEL") && !error.message.contains("xxe"),
            "{name}: content-free diagnostics: {}",
            error.message
        );
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["xml"].is_null(), "{name}");
    }
}

#[test]
fn xml_namespace_declarations_are_validated_used_or_unused() {
    let directory = unique_dir("xmlns-validate");
    // Positive: a used declaration whose URI carries a legal expanded
    // reference, an unused but valid declaration, and a default-namespace
    // declaration all remain accepted.
    let ok = write_fixture(
        &directory,
        "ok.xml",
        b"<a xmlns=\"urn:default\" xmlns:p=\"urn:x&amp;y\" xmlns:unused=\"urn:ok\"><p:b/></a>",
    );
    let r = inspect(&ok, &InspectOptions::default());
    assert!(r.errors.is_empty(), "valid declarations: {:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["xml"]["namespaces"].as_array().unwrap().len(), 2);
    // Negative: malformed references and illegal expanded characters fail
    // closed whether or not the declared prefix is ever used, and the
    // illegal identity never reaches serialized output.
    let cases: [(&str, &[u8]); 4] = [
        ("unused-evil.xml", b"<a xmlns:p=\"urn:&evil;\"/>"),
        ("used-nul.xml", b"<p:a xmlns:p=\"urn:SENTINEL-U&#x1;\"/>"),
        ("unused-control.xml", b"<a xmlns:q=\"urn:SENTINEL-C\x01\"/>"),
        ("default-evil.xml", b"<a xmlns=\"urn:&evil;\"/>"),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        let error = r
            .errors
            .iter()
            .find(|e| e.stage == "xml")
            .unwrap_or_else(|| panic!("{name}: xml stage error required: {:?}", r.errors));
        assert!(
            !error.message.contains("SENTINEL") && !error.message.contains("evil"),
            "{name}: content-free diagnostics: {}",
            error.message
        );
        let serialized = serde_json::to_string(&r).unwrap();
        assert!(
            !serialized.contains("\\u0001") && !serialized.contains('\u{1}'),
            "{name}: no U+0001 reaches serialized output"
        );
        assert!(!serialized.contains("SENTINEL"), "{name}");
    }
}

#[test]
fn xml_prolog_declaration_is_exactly_one_wellformed_xml10_decl() {
    let directory = unique_dir("decl-rules");
    // Positive: version alone, the full legal ordering, and standalone="no".
    let positives: [(&str, &[u8]); 3] = [
        ("version-only.xml", b"<?xml version=\"1.0\"?><a/>"),
        (
            "full.xml",
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><a/>",
        ),
        (
            "standalone-no.xml",
            b"<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?><a/>",
        ),
    ];
    for (name, bytes) in positives {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
    }
    // Negative: anything beyond exactly one well-formed XML 1.0 declaration
    // fails closed, and declaration failures never echo attribute names.
    let negatives: [(&str, &[u8]); 8] = [
        (
            "unknown-attr.xml",
            b"<?xml version=\"1.0\" SENTINELATTR=\"x\"?><a/>",
        ),
        (
            "duplicate-attr.xml",
            b"<?xml version=\"1.0\" version=\"1.0\"?><a/>",
        ),
        (
            "wrong-order.xml",
            b"<?xml encoding=\"UTF-8\" version=\"1.0\"?><a/>",
        ),
        ("missing-version.xml", b"<?xml encoding=\"UTF-8\"?><a/>"),
        (
            "bad-standalone.xml",
            b"<?xml version=\"1.0\" standalone=\"maybe\"?><a/>",
        ),
        ("version-11.xml", b"<?xml version=\"1.1\"?><a/>"),
        (
            "second-decl.xml",
            b"<?xml version=\"1.0\"?><?xml version=\"1.0\"?><a/>",
        ),
        (
            "late-decl.xml",
            b"<?xml version=\"1.0\"?><a/><?xml version=\"1.0\"?>",
        ),
    ];
    for (name, bytes) in negatives {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        let error = r
            .errors
            .iter()
            .find(|e| e.stage == "xml")
            .unwrap_or_else(|| panic!("{name}: xml stage error required: {:?}", r.errors));
        assert!(
            !error.message.contains("SENTINELATTR")
                && !error.message.contains("version=")
                && !error.message.contains('<'),
            "{name}: static content-free declaration diagnostics: {}",
            error.message
        );
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["xml"].is_null(), "{name}");
    }
}

#[test]
fn xml_comments_pi_and_names_are_validated() {
    let directory = unique_dir("lexical");
    // Positive: legitimate comments, processing instructions (including the
    // conventional xml-stylesheet target), non-ASCII names and dotted names.
    let ok = write_fixture(
        &directory,
        "ok.xml",
        "<r><!--legit comment--><?target some data?><?xml-stylesheet href=\"x\"?><caf\u{e9} a.b-c_d=\"1\">ok</caf\u{e9}></r>".as_bytes(),
    );
    let r = inspect(&ok, &InspectOptions::default());
    assert!(r.errors.is_empty(), "valid lexical forms: {:?}", r.errors);
    // Negative: malformed comments, reserved or illegal PI targets, illegal
    // comment/PI characters and non-well-formed names all fail closed with
    // static content-free messages.
    let cases: [(&str, &[u8]); 9] = [
        ("comment-dash.xml", b"<a><!--x--y--></a>"),
        ("comment-control.xml", b"<a><!--SENTINEL-K\x01--></a>"),
        ("pi-reserved.xml", b"<?XML SENTINEL-T?><a/>"),
        ("pi-target-digit.xml", b"<?1bad x?><a/>"),
        ("pi-content-control.xml", b"<a><?pi SENTINEL-P\x01?></a>"),
        ("name-control.xml", b"<a\x01/>"),
        ("name-digit.xml", b"<1a/>"),
        ("name-empty-local.xml", b"<a:/>"),
        ("attr-name-control.xml", b"<a SENTINEL-N\x01=\"1\"/>"),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(&directory, name, bytes);
        let r = inspect(&path, &InspectOptions::default());
        let error = r
            .errors
            .iter()
            .find(|e| e.stage == "xml")
            .unwrap_or_else(|| panic!("{name}: xml stage error required: {:?}", r.errors));
        assert!(
            !error.message.contains("SENTINEL") && !error.message.contains('\u{1}'),
            "{name}: content-free diagnostics: {}",
            error.message
        );
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["xml"].is_null(), "{name}");
    }
}
