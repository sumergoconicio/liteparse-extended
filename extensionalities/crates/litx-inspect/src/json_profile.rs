//! Array-normalized scalar telemetry, streamed without constructing a JSON DOM.
use crate::{InspectOptions, InspectReport, binary::Source, xml};
use serde::{
    Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::{Hash, Hasher},
    path::Path,
};

#[derive(Debug, Default, Serialize)]
pub struct JsonProfile {
    pub total_leaves: usize,
    pub max_depth: usize,
    pub leaf_stats: BTreeMap<String, LeafStats>,
    pub loops: Vec<Loop>,
    #[serde(skip)]
    arrays: BTreeMap<String, usize>,
    #[serde(skip)]
    nodes: usize,
    #[serde(skip)]
    distinct: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct LeafStats {
    pub count: usize,
    pub value_type: String,
    pub total_bytes: usize,
    pub distinct_count: usize,
    pub sample_values: Vec<String>,
    #[serde(skip)]
    hashes: BTreeSet<u64>,
}

#[derive(Debug, Serialize)]
pub struct Loop {
    pub path: String,
    pub count: usize,
    pub fields: Vec<String>,
    pub fields_truncated: bool,
}

/// Total `Loop.fields` entries emitted across all array depths.
const LOOP_FIELDS_CAP: usize = 1_024;
/// Total `Loop.fields` string bytes emitted across all array depths.
const LOOP_FIELD_BYTES_CAP: usize = 64 << 10;

struct Seed<'a> {
    profile: &'a mut JsonProfile,
    path: String,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.profile.nodes += 1;
        self.profile.max_depth = self.profile.max_depth.max(self.depth);
        if self.depth > 64 || self.path.len() > 8192 || self.profile.nodes > 2_000_000 {
            return Err(de::Error::custom("JSON depth/path/node cap exceeded"));
        }
        deserializer.deserialize_any(self)
    }
}

impl Seed<'_> {
    fn scalar<E: de::Error>(self, value: &str, kind: &str) -> Result<(), E> {
        if !self.profile.leaf_stats.contains_key(&self.path)
            && self.profile.leaf_stats.len() >= 16_384
        {
            return Err(E::custom("JSON tracked-path cap exceeded"));
        }
        let stat = self.profile.leaf_stats.entry(self.path).or_default();
        stat.count += 1;
        stat.total_bytes += value.len();
        if stat.value_type.is_empty() {
            stat.value_type = kind.into();
        } else if stat.value_type != kind {
            stat.value_type = "mixed".into();
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        kind.hash(&mut hash);
        value.hash(&mut hash);
        let hash = hash.finish();
        if !stat.hashes.contains(&hash) {
            if self.profile.distinct >= 524_288 {
                return Err(E::custom("JSON distinct-value cap exceeded"));
            }
            stat.hashes.insert(hash);
            self.profile.distinct += 1;
            stat.distinct_count += 1;
            if stat.sample_values.len() < 3 {
                stat.sample_values.push(value.chars().take(128).collect());
            }
        }
        self.profile.total_leaves += 1;
        Ok(())
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = ();
    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("a JSON value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<(), E> {
        self.scalar(if v { "true" } else { "false" }, "bool")
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<(), E> {
        self.scalar(&v.to_string(), "number")
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<(), E> {
        self.scalar(&v.to_string(), "number")
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<(), E> {
        self.scalar(&v.to_string(), "number")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<(), E> {
        self.scalar(v, "string")
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.scalar("null", "null")
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        self.scalar("null", "null")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        while let Some(key) = map.next_key::<String>()? {
            let simple = !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_');
            let path = if simple {
                if self.path.is_empty() {
                    key
                } else {
                    format!("{}.{}", self.path, key)
                }
            } else {
                format!(
                    "{}[{}]",
                    self.path,
                    serde_json::to_string(&key).map_err(de::Error::custom)?
                )
            };
            map.next_value_seed(Seed {
                profile: self.profile,
                path,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
        let path = format!("{}[]", self.path);
        if !self.profile.arrays.contains_key(&path) && self.profile.arrays.len() >= 16_384 {
            return Err(de::Error::custom("JSON array-path cap exceeded"));
        }
        self.profile.arrays.entry(path.clone()).or_default();
        while seq
            .next_element_seed(Seed {
                profile: self.profile,
                path: path.clone(),
                depth: self.depth + 1,
            })?
            .is_some()
        {
            *self.profile.arrays.get_mut(&path).unwrap() += 1;
        }
        Ok(())
    }
}

pub(crate) fn inspect(path: &Path, report: &mut InspectReport, options: &InspectOptions) {
    let result = (|| -> xml::Result<()> {
        let mut source = Source::open(path)?;
        let bytes = source.range(0, source.len as usize)?;
        let mut profile = JsonProfile::default();
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        Seed {
            profile: &mut profile,
            path: String::new(),
            depth: 0,
        }
        .deserialize(&mut decoder)?;
        decoder.end()?;
        let mut field_entries = 0usize;
        let mut field_bytes = 0usize;
        for (path, count) in &profile.arrays {
            if *count < 2 {
                continue;
            }
            let prefix = format!("{path}.");
            let mut fields = Vec::new();
            let mut fields_truncated = false;
            for key in profile.leaf_stats.keys() {
                let field = if key == path {
                    "<value>"
                } else if let Some(suffix) = key.strip_prefix(&prefix) {
                    suffix
                } else {
                    continue;
                };
                if field_entries >= LOOP_FIELDS_CAP
                    || field_bytes + field.len() > LOOP_FIELD_BYTES_CAP
                {
                    fields_truncated = true;
                    break;
                }
                field_entries += 1;
                field_bytes += field.len();
                fields.push(field.to_string());
            }
            profile.loops.push(Loop {
                path: path.clone(),
                count: *count,
                fields,
                fields_truncated,
            });
        }
        let mut markdown = format!(
            "# JSON structure\n\n- Scalar leaves: {}\n- Maximum depth: {}\n\n| Leaf path | Type | Occurrences | Distinct |\n| --- | --- | --- | --- |\n",
            profile.total_leaves, profile.max_depth
        );
        for (path, stat) in &profile.leaf_stats {
            markdown.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                crate::markdown::escape(path).replace('|', "\\|"),
                stat.value_type,
                stat.count,
                stat.distinct_count
            ));
            if markdown.len() > options.max_chars.saturating_mul(4) {
                break;
            }
        }
        crate::markdown::append(report, &markdown, options, "native_structure");
        report.append_text(std::str::from_utf8(&bytes)?, options.max_chars);
        report.json = Some(profile);
        Ok(())
    })();
    if let Err(error) = result {
        report.error("json", error);
    }
}
