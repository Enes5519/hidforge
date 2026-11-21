//! HID report descriptor analysis.
//!
//! Per the HID spec, global items (Report Size, Report Count, Report ID, Logical Min/Max, ...)
//! keep their value until redefined, also across top-level collections. Windows honours that.
//! Wine's hidparse splits the descriptor into top-level collections and parses each slice with
//! a fresh global state, so a collection that relies on a Report Size set by an earlier one ends
//! up with zero-sized fields. Its reports shrink to the bare report ID and writes fail with
//! EINVAL: the application sees the device but settings (DPI, RGB, ...) never reach it.
//!
//! The fix is to repeat the inherited globals at the start of each affected collection, which
//! does not change the meaning of the descriptor for a spec-compliant parser.

use std::fmt;

const MAIN_INPUT: u8 = 0x80;
const MAIN_OUTPUT: u8 = 0x90;
const MAIN_FEATURE: u8 = 0xb0;
const MAIN_COLLECTION: u8 = 0xa0;
const MAIN_END_COLLECTION: u8 = 0xc0;
const GLOBAL_PUSH: u8 = 0xa4;
const GLOBAL_POP: u8 = 0xb4;
const LONG_ITEM: u8 = 0xfe;

/// Globals a main item depends on, in the order they are re-emitted.
const INHERITABLE: [(u8, &str); 10] = [
    (0x04, "Usage Page"),
    (0x84, "Report ID"),
    (0x74, "Report Size"),
    (0x94, "Report Count"),
    (0x14, "Logical Minimum"),
    (0x24, "Logical Maximum"),
    (0x34, "Physical Minimum"),
    (0x44, "Physical Maximum"),
    (0x54, "Unit Exponent"),
    (0x64, "Unit"),
];
const N_GLOBALS: usize = INHERITABLE.len();

fn global_index(tag: u8) -> Option<usize> {
    INHERITABLE.iter().position(|(t, _)| *t == tag)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

impl fmt::Display for ReportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ReportKind::Input => "input",
            ReportKind::Output => "output",
            ReportKind::Feature => "feature",
        })
    }
}

/// A top-level collection that silently depends on globals from an earlier collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// Index of the top-level collection (0-based).
    pub collection: usize,
    /// Names of the globals Wine loses, e.g. `["Report Size"]`.
    pub missing: Vec<&'static str>,
    /// Reports in that collection, deduplicated, in descriptor order.
    pub reports: Vec<(ReportKind, u8)>,
}

/// Bytes to insert into the descriptor at `offset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insert {
    pub offset: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub issues: Vec<Issue>,
    pub inserts: Vec<Insert>,
}

impl Analysis {
    pub fn needs_fix(&self) -> bool {
        !self.issues.is_empty()
    }
}

/// Raw encoding of the last definition of each global (`None` = never set).
type GlobalState = [Option<Vec<u8>>; N_GLOBALS];

/// Value of an encoded short item, ignoring its prefix byte.
fn item_value(raw: &[u8]) -> u32 {
    raw[1..]
        .iter()
        .rev()
        .fold(0u32, |acc, &b| (acc << 8) | b as u32)
}

/// Find collections Wine would parse with lost globals. Returns an empty analysis for
/// malformed descriptors.
pub fn analyze(desc: &[u8]) -> Analysis {
    analyze_inner(desc).unwrap_or_default()
}

fn analyze_inner(desc: &[u8]) -> Option<Analysis> {
    let mut out = Analysis::default();
    let mut state: GlobalState = Default::default();
    let mut stack: Vec<GlobalState> = Vec::new();

    let mut depth = 0usize;
    let mut tlc = 0usize;
    // Per top-level-collection slice:
    let mut defined = [false; N_GLOBALS];
    let mut needed: [Option<Vec<u8>>; N_GLOBALS] = Default::default();
    let mut reports: Vec<(ReportKind, u8)> = Vec::new();
    let mut insert_at: Option<usize> = None;

    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == LONG_ITEM {
            i += 3 + *desc.get(i + 1)? as usize;
            continue;
        }
        let size = [0, 1, 2, 4][(prefix & 3) as usize];
        let raw = desc.get(i..i + 1 + size)?;
        let tag = prefix & 0xfc;
        match tag {
            MAIN_INPUT | MAIN_OUTPUT | MAIN_FEATURE => {
                for (g, slot) in state.iter().enumerate() {
                    // Wine starts each slice from all-zero globals; a zero value loses nothing.
                    if let Some(v) = slot
                        && !defined[g]
                        && needed[g].is_none()
                        && item_value(v) != 0
                    {
                        needed[g] = Some(v.clone());
                    }
                }
                let kind = match tag {
                    MAIN_INPUT => ReportKind::Input,
                    MAIN_OUTPUT => ReportKind::Output,
                    _ => ReportKind::Feature,
                };
                // A Report ID above 255 is malformed: give up like any other bad descriptor.
                let id = u8::try_from(state[global_index(0x84)?].as_deref().map_or(0, item_value))
                    .ok()?;
                if !reports.contains(&(kind, id)) {
                    reports.push((kind, id));
                }
            }
            MAIN_COLLECTION => {
                if depth == 0 {
                    insert_at = Some(i + 1 + size);
                }
                depth += 1;
            }
            MAIN_END_COLLECTION => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let missing: Vec<usize> =
                        (0..N_GLOBALS).filter(|&g| needed[g].is_some()).collect();
                    if !missing.is_empty() {
                        out.inserts.push(Insert {
                            offset: insert_at?,
                            bytes: missing
                                .iter()
                                .flat_map(|&g| needed[g].clone().unwrap())
                                .collect(),
                        });
                        out.issues.push(Issue {
                            collection: tlc,
                            missing: missing.iter().map(|&g| INHERITABLE[g].1).collect(),
                            reports: std::mem::take(&mut reports),
                        });
                    }
                    tlc += 1;
                    defined = [false; N_GLOBALS];
                    needed = Default::default();
                    reports.clear();
                    insert_at = None;
                }
            }
            GLOBAL_PUSH => stack.push(state.clone()),
            GLOBAL_POP => state = stack.pop()?,
            _ => {
                if let Some(g) = global_index(tag) {
                    state[g] = Some(raw.to_vec());
                    defined[g] = true;
                }
            }
        }
        i += 1 + size;
    }
    (i == desc.len() && depth == 0).then_some(out)
}

/// Apply inserts (sorted by offset) to produce the fixed descriptor.
pub fn apply(desc: &[u8], inserts: &[Insert]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(desc.len() + inserts.iter().map(|x| x.bytes.len()).sum::<usize>());
    let mut pos = 0;
    for ins in inserts {
        out.extend_from_slice(&desc[pos..ins.offset]);
        out.extend_from_slice(&ins.bytes);
        pos = ins.offset;
    }
    out.extend_from_slice(&desc[pos..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Interface 2 of the Beken 1d57:fa60 receiver (Attack Shark X3), as read from sysfs.
    /// Collection 3 (vendor feature reports) never sets Report Size; it inherits 8 from
    /// collection 2.
    const BEKEN_IF2: &str = "05010980a101850105011981298315002501950375018106950175058101c0050c0901a1018502150026800319002a8003751095018100c0050a0900a10185031900290015002500750895048101c0050b0900a10185049533b1018505950cb10185069508b10185079507b1018508953ab1018509953fb101850a9507b101850b9507b101850c9505b10185109507b10185a09507b10185229583b10185239583b10185249507b1018525950cb10185269508b10185279507b10185289583b10185299580b101852a9583b101852b9507b101852c9503b101852d9583b10185239582b101852e9504b101c0";

    #[test]
    fn detects_report_size_inherited_across_collections() {
        let desc = unhex(BEKEN_IF2);
        assert_eq!(desc.len(), 236);
        let a = analyze(&desc);
        assert_eq!(a.issues.len(), 1);
        let issue = &a.issues[0];
        assert_eq!(issue.collection, 3);
        assert_eq!(issue.missing, ["Report Size"]);
        assert_eq!(issue.reports.first(), Some(&(ReportKind::Feature, 4)));
        assert_eq!(issue.reports.len(), 24); // 0x23 is declared twice
        // Inserted right after "a1 01" of collection 3.
        assert_eq!(
            a.inserts,
            [Insert {
                offset: 0x55,
                bytes: vec![0x75, 0x08]
            }]
        );

        let fixed = apply(&desc, &a.inserts);
        assert_eq!(fixed.len(), 238);
        assert_eq!(&fixed[0x4f..0x5b], &unhex("050b0900a1017508 85049533"));
        assert!(!analyze(&fixed).needs_fix());
    }

    #[test]
    fn self_contained_collections_are_fine() {
        // Mouse + keyboard LEDs, each collection sets its own globals; padding is Constant.
        let desc = unhex(
            "05010902a101 950375018102 950175058101 c0
             05010906a101 8502 950575019102 950175039101 c0",
        );
        assert!(!analyze(&desc).needs_fix());
    }

    #[test]
    fn zero_valued_globals_are_not_reported() {
        // Logical Minimum 0 is Wine's default, so not re-setting it loses nothing.
        let desc = unhex("1500 750895018102 a101 c0 a101 750895018102 c0");
        assert!(!analyze(&desc).needs_fix());
    }

    #[test]
    fn malformed_descriptor_is_ignored() {
        assert!(!analyze(&[0x05, 0x01, 0x26]).needs_fix());
        assert!(!analyze(&unhex("a101 a101 c0")).needs_fix()); // unbalanced
        // Report ID 0x100 (2-byte item) would otherwise be truncated to 0.
        assert_eq!(
            analyze(&unhex("750895018102 a101 860001 8102 c0")),
            Analysis::default()
        );
    }
}
